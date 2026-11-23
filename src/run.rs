//! The run command: sample Synthetic customers, ask each, score, report;
//! resume a stopped run without asking anyone twice.

use crate::anchors::AnchorSet;
use crate::customer_mix::{Rng, SyntheticCustomer};
use crate::embedder::Embedder;
use crate::respondent::Respondent;
use crate::run_file::RunFile;
use crate::ssr::{self, Distribution, SsrScorer};
use crate::{report, report_html};
use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Instant;

pub const REPORT_FILE: &str = "report.md";
pub const HTML_FILE: &str = "report.html";
pub const DATA_FILE: &str = "answers.jsonl";
pub const RUN_FILE_COPY: &str = "run.toml";
/// What a resumed run needs besides the run file: the seed that picked the
/// Synthetic customers, so a resume asks exactly the same ones.
pub const STATE_FILE: &str = "run-state.json";

/// One line of the data file: one Synthetic customer's answer and its score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub concept: String,
    /// 1-based position of the customer within the Concept.
    pub customer: usize,
    pub attributes: IndexMap<String, String>,
    pub answer: String,
    /// Probability of Purchase intent 1 to 5.
    pub distribution: Distribution,
    pub expected_rating: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct RunState {
    seed: u64,
    /// UTC date the run started, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    /// The Scorer that scored the saved answers, when it is not the run
    /// file's (after a re-score with another Scorer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scorer: Option<String>,
}

pub struct RunOutput {
    pub report: PathBuf,
    pub data: PathBuf,
    /// Set when the optional HTML report was written.
    pub html: Option<PathBuf>,
}

/// Everything a run needs besides the run file; tests swap in fakes here.
pub struct RunSetup<'a> {
    pub respondent: &'a dyn Respondent,
    pub embedder: &'a dyn Embedder,
    pub anchor_sets: Vec<AnchorSet>,
    pub epsilon: f64,
    /// Seed for a new run; a resumed run reuses the seed it started with.
    pub seed: u64,
    /// Continue the run already in the output folder, asking only the
    /// Synthetic customers who have not answered yet.
    pub resume: bool,
    /// Also write the self-contained HTML report.
    pub html: bool,
}

/// Ask every Synthetic customer not yet answered, `concurrency` at a time,
/// appending each answer to the data file as it arrives, then write the
/// report. If a customer's call fails (for example a usage limit), the run
/// stops, the report is written from the answers collected so far (marked as
/// a short sample), and the error says how to resume.
pub fn run(
    run_file: &RunFile,
    run_file_text: &str,
    setup: RunSetup<'_>,
    out_dir: &Path,
) -> Result<RunOutput> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create output folder {}", out_dir.display()))?;
    let data_path = out_dir.join(DATA_FILE);
    let state_path = out_dir.join(STATE_FILE);
    let has_answers = std::fs::metadata(&data_path).is_ok_and(|m| m.len() > 0);

    let (seed, date, mut records) = if has_answers {
        if !setup.resume {
            bail!(
                "{} already holds answers from an earlier run: pass --resume to continue \
                 that run, or choose another --out folder",
                out_dir.display()
            );
        }
        let saved = std::fs::read_to_string(out_dir.join(RUN_FILE_COPY)).unwrap_or_default();
        if saved != run_file_text {
            bail!(
                "cannot resume: the run file has changed since the run in {} started \
                 (its copy is {}); undo the change, or choose another --out folder",
                out_dir.display(),
                out_dir.join(RUN_FILE_COPY).display()
            );
        }
        let state: RunState = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .with_context(|| {
                format!(
                    "cannot resume: {} is missing or invalid",
                    state_path.display()
                )
            })?;
        (state.seed, state.date, load_records(&data_path)?)
    } else {
        (setup.seed, Some(today_utc()), Vec::new())
    };

    let scorer = SsrScorer::new(setup.embedder, &setup.anchor_sets, setup.epsilon)?;

    // The same seed always gives the same Synthetic customers, so a resume
    // asks exactly the ones missing from the data file.
    let mut rng = Rng::new(seed);
    let customers: Vec<Vec<SyntheticCustomer>> = run_file
        .concepts
        .iter()
        .map(|_| {
            run_file
                .customer_mix
                .sample(run_file.customers_per_concept, &mut rng)
        })
        .collect();
    let mut done: HashSet<(String, usize)> = HashSet::new();
    for r in &records {
        let expected = run_file
            .concepts
            .iter()
            .position(|c| c.name == r.concept)
            .and_then(|ci| customers[ci].get(r.customer.wrapping_sub(1)));
        match expected {
            Some(c) if c.attributes == r.attributes => {}
            _ => bail!(
                "cannot resume: customer {} of Concept `{}` in {} does not match this run",
                r.customer,
                r.concept,
                data_path.display()
            ),
        }
        if !done.insert((r.concept.clone(), r.customer)) {
            bail!(
                "cannot resume: customer {} of Concept `{}` appears twice in {}",
                r.customer,
                r.concept,
                data_path.display()
            );
        }
    }
    // Only now, with the earlier answers checked, touch the output folder.
    // A report left from an earlier run must not sit next to new answers.
    let old_report = out_dir.join(REPORT_FILE);
    if old_report.exists() {
        std::fs::remove_file(&old_report)
            .with_context(|| format!("cannot remove old {}", old_report.display()))?;
    }
    std::fs::write(out_dir.join(RUN_FILE_COPY), run_file_text)
        .context("cannot save a copy of the run file")?;
    std::fs::write(
        &state_path,
        serde_json::to_string(&RunState {
            seed,
            date: date.clone(),
            scorer: None,
        })?,
    )
    .with_context(|| format!("cannot write {}", state_path.display()))?;
    // Rewrite the kept answers, dropping a line cut short by an interruption.
    write_records(&data_path, &records)?;

    let jobs: Vec<(usize, usize)> = run_file
        .concepts
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| {
            let done = &done;
            (0..run_file.customers_per_concept)
                .filter(move |i| !done.contains(&(c.name.clone(), i + 1)))
                .map(move |i| (ci, i))
        })
        .collect();

    let total = run_file.concepts.len() * run_file.customers_per_concept;
    let already = total - jobs.len();
    let workers = run_file.concurrency.min(jobs.len());
    if jobs.is_empty() {
        eprintln!("All {total} Synthetic customers have already answered.");
    } else {
        let rounds = jobs.len().div_ceil(workers);
        let estimate = rounds as f64 * setup.respondent.seconds_per_answer();
        let resumed = if already > 0 {
            format!(" ({already} already answered)")
        } else {
            String::new()
        };
        eprintln!(
            "Asking {} of {total} Synthetic customers{resumed}, {workers} at a time: \
             {}.",
            jobs.len(),
            human_duration(estimate)
        );
    }

    let mut data = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .append(true)
            .open(&data_path)
            .with_context(|| format!("cannot write {}", data_path.display()))?,
    );
    let mut failure: Option<anyhow::Error> = None;
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<(usize, Result<String>)>();
        for _ in 0..workers {
            let tx = tx.clone();
            let (jobs, next, stop, customers) = (&jobs, &next, &stop, &customers);
            scope.spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let j = next.fetch_add(1, Ordering::SeqCst);
                    let Some(&(ci, i)) = jobs.get(j) else { break };
                    let concept = &run_file.concepts[ci];
                    let answer = setup
                        .respondent
                        .answer(concept, i, &customers[ci][i])
                        .with_context(|| {
                            format!("customer {} of Concept `{}`", i + 1, concept.name)
                        });
                    if tx.send((j, answer)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut answered = 0;
        let mut last_shown = None;
        for (j, answer) in rx {
            let (ci, i) = jobs[j];
            let result = answer.and_then(|answer| {
                let distribution = scorer.score(&answer)?;
                let record = AnswerRecord {
                    concept: run_file.concepts[ci].name.clone(),
                    customer: i + 1,
                    attributes: customers[ci][i].attributes.clone(),
                    answer,
                    expected_rating: ssr::expected_rating(&distribution),
                    distribution,
                };
                serde_json::to_writer(&mut data, &record)?;
                data.write_all(b"\n")?;
                data.flush()?;
                records.push(record);
                Ok(())
            });
            match result {
                Ok(()) => {
                    answered += 1;
                    show_progress(
                        already + answered,
                        total,
                        answered,
                        jobs.len(),
                        started,
                        &mut last_shown,
                    );
                }
                Err(e) => {
                    stop.store(true, Ordering::SeqCst);
                    if failure.is_none() {
                        failure = Some(e);
                    }
                }
            }
        }
    });
    drop(data);

    // Keep the data file in a stable order: by Concept, then customer.
    records.sort_by_key(|r| {
        let ci = run_file.concepts.iter().position(|c| c.name == r.concept);
        (ci, r.customer)
    });
    write_records(&data_path, &records)?;

    let (report_path, html_path) = write_reports(
        out_dir,
        run_file,
        setup.embedder.name(),
        seed,
        date.as_deref(),
        &records,
        setup.html,
    )?;

    if let Some(e) = failure {
        return Err(e.context(format!(
            "the run stopped with {} of {total} Synthetic customers answered; \
             their answers are kept and {} is marked as a short sample. \
             Run the same command with --resume to ask only the missing customers",
            records.len(),
            report_path.display()
        )));
    }
    Ok(RunOutput {
        report: report_path,
        data: data_path,
        html: html_path,
    })
}

/// Write `report.md`, and `report.html` when asked (an old one is removed so
/// it never sits next to a newer report.md).
fn write_reports(
    out_dir: &Path,
    run_file: &RunFile,
    scorer_name: &str,
    seed: u64,
    date: Option<&str>,
    records: &[AnswerRecord],
    html: bool,
) -> Result<(PathBuf, Option<PathBuf>)> {
    let analysis = report::analyse(run_file, scorer_name, seed, date, records);
    let report_path = out_dir.join(REPORT_FILE);
    std::fs::write(&report_path, report::markdown(&analysis))
        .with_context(|| format!("cannot write {}", report_path.display()))?;
    let html_path = out_dir.join(HTML_FILE);
    if html {
        std::fs::write(&html_path, report_html::html(&analysis))
            .with_context(|| format!("cannot write {}", html_path.display()))?;
        Ok((report_path, Some(html_path)))
    } else {
        if html_path.exists() {
            std::fs::remove_file(&html_path)
                .with_context(|| format!("cannot remove old {}", html_path.display()))?;
        }
        Ok((report_path, None))
    }
}

/// Re-score (when `rescore` is given) and re-report a saved output folder
/// from its `answers.jsonl` and `run.toml`, asking no Synthetic customer.
pub fn rebuild_report(
    out_dir: &Path,
    rescore: Option<(&SsrScorer<'_>, &str)>,
    html: bool,
) -> Result<RunOutput> {
    let run_text = std::fs::read_to_string(out_dir.join(RUN_FILE_COPY)).with_context(|| {
        format!(
            "{} has no {RUN_FILE_COPY}: it is not the output folder of a run",
            out_dir.display()
        )
    })?;
    let run_file = RunFile::parse(&run_text)
        .with_context(|| format!("invalid {}", out_dir.join(RUN_FILE_COPY).display()))?;
    let data_path = out_dir.join(DATA_FILE);
    let mut records = load_records(&data_path)?;
    let state_path = out_dir.join(STATE_FILE);
    let mut state: RunState = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .with_context(|| format!("{} is missing or invalid", state_path.display()))?;
    if let Some((scorer, scorer_name)) = rescore {
        for r in &mut records {
            r.distribution = scorer.score(&r.answer)?;
            r.expected_rating = ssr::expected_rating(&r.distribution);
        }
        write_records(&data_path, &records)?;
        state.scorer = Some(scorer_name.to_string());
        std::fs::write(&state_path, serde_json::to_string(&state)?)
            .with_context(|| format!("cannot write {}", state_path.display()))?;
    }
    let scorer_name = state
        .scorer
        .clone()
        .unwrap_or_else(|| run_file.scorer.as_str().to_string());
    let (report, html) = write_reports(
        out_dir,
        &run_file,
        &scorer_name,
        state.seed,
        state.date.as_deref(),
        &records,
        html,
    )?;
    Ok(RunOutput {
        report,
        data: data_path,
        html,
    })
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    civil_date((secs / 86_400) as i64)
}

/// Days since 1970-01-01 to `YYYY-MM-DD` (Howard Hinnant's algorithm).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Read the data file, skipping a last line cut short by an interruption.
fn load_records(path: &Path) -> Result<Vec<AnswerRecord>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut records = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        match serde_json::from_str(line) {
            Ok(r) => records.push(r),
            Err(_) if n + 1 == lines.len() && !text.ends_with('\n') => {}
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "cannot resume: line {} of {} is invalid",
                        n + 1,
                        path.display()
                    )
                });
            }
        }
    }
    Ok(records)
}

/// Write the data file whole, through a temporary file, so an interruption
/// never leaves it half written.
fn write_records(path: &Path, records: &[AnswerRecord]) -> Result<()> {
    let tmp = path.with_extension("jsonl.tmp");
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?,
    );
    for r in records {
        serde_json::to_writer(&mut out, r)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))
}

/// One progress line on stderr each time another 5% of the run is answered
/// (every answer on small runs), with the time left at the pace so far.
fn show_progress(
    done: usize,
    total: usize,
    answered: usize,
    to_ask: usize,
    started: Instant,
    last_shown: &mut Option<usize>,
) {
    let step = (total / 20).max(1);
    let bucket = done / step;
    if done != total && *last_shown == Some(bucket) {
        return;
    }
    *last_shown = Some(bucket);
    let per_answer = started.elapsed().as_secs_f64() / answered as f64;
    let left = per_answer * (to_ask - answered) as f64;
    let eta = if done == total {
        String::new()
    } else {
        format!(", {} left", human_duration(left))
    };
    eprintln!(
        "Answered {done}/{total} ({}%){eta}",
        done * 100 / total.max(1)
    );
}

fn human_duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match minutes {
        0 => "under a minute".to_string(),
        1 => "about 1 minute".to_string(),
        m if m < 60 => format!("about {m} minutes"),
        m => format!("about {} h {:02} min", m / 60, m % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::civil_date;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(19_723), "2024-01-01");
        assert_eq!(civil_date(11_016), "2000-02-29");
    }
}
