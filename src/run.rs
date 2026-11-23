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

/// A scored answer: one Synthetic customer's answer and its score.
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

/// One line of the data file. An answer is saved as soon as it arrives,
/// before it is scored, so a Scorer failure (network, key, rate limit) never
/// loses an answer already paid for; a resume scores the saved ones.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct SavedAnswer {
    concept: String,
    customer: usize,
    attributes: IndexMap<String, String>,
    answer: String,
    /// Missing until the answer is scored.
    #[serde(flatten)]
    score: Option<Score>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Score {
    distribution: Distribution,
    expected_rating: f64,
}

impl Score {
    fn new(distribution: Distribution) -> Self {
        Self {
            expected_rating: ssr::expected_rating(&distribution),
            distribution,
        }
    }
}

impl SavedAnswer {
    fn record(&self) -> Option<AnswerRecord> {
        let score = self.score.as_ref()?;
        Some(AnswerRecord {
            concept: self.concept.clone(),
            customer: self.customer,
            attributes: self.attributes.clone(),
            answer: self.answer.clone(),
            distribution: score.distribution,
            expected_rating: score.expected_rating,
        })
    }
}

/// The scored answers, for the report.
fn scored(saved: &[SavedAnswer]) -> Vec<AnswerRecord> {
    saved.iter().filter_map(SavedAnswer::record).collect()
}

/// Answers scored per Scorer call during a run: a hosted Scorer then needs
/// one request per this many answers.
const SCORE_BATCH: usize = 32;

/// Score the answers at `pending` (indexes into `saved`) in one call.
fn score_pending(
    scorer: &SsrScorer<'_>,
    saved: &mut [SavedAnswer],
    pending: &[usize],
) -> Result<()> {
    let texts: Vec<&str> = pending.iter().map(|&i| saved[i].answer.as_str()).collect();
    let distributions = scorer.score_many(&texts)?;
    for (&i, d) in pending.iter().zip(distributions) {
        saved[i].score = Some(Score::new(d));
    }
    Ok(())
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
/// appending each answer to the data file as it arrives (before scoring
/// it), score the answers in batches, then write the report. If a customer's
/// call or the Scorer fails (a usage limit, a network error, a bad key), the
/// run stops asking, keeps every answer received, writes the report from the
/// scored ones (marked as a short sample), and the error says how to resume.
/// A resume first scores the saved answers that were not scored yet.
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

    let (seed, date, saved_scorer, mut saved) = if has_answers {
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
        if state
            .scorer
            .as_deref()
            .is_some_and(|s| s != setup.embedder.name())
        {
            bail!(
                "cannot resume: the saved answers were re-scored with the `{}` Scorer, \
                 but this run uses `{}`. Re-score again with `report --rescore` first, or \
                 choose another --out folder",
                state.scorer.unwrap_or_default(),
                setup.embedder.name()
            );
        }
        (
            state.seed,
            state.date,
            state.scorer,
            load_records(&data_path)?,
        )
    } else {
        (setup.seed, Some(today_utc()), None, Vec::new())
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
    for r in &saved {
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
            scorer: saved_scorer,
        })?,
    )
    .with_context(|| format!("cannot write {}", state_path.display()))?;
    // Rewrite the kept answers, dropping a line cut short by an interruption.
    write_records(&data_path, &saved)?;

    // Answers saved by an earlier run but not scored (its Scorer failed)
    // are scored first, without asking those customers again.
    let unscored: Vec<usize> = (0..saved.len())
        .filter(|&i| saved[i].score.is_none())
        .collect();
    if !unscored.is_empty() {
        eprintln!(
            "Scoring {} saved answers that were not scored yet.",
            unscored.len()
        );
        score_pending(&scorer, &mut saved, &unscored).with_context(|| {
            format!(
                "cannot score the {} saved answers that were not scored yet; they stay \
                 saved in {}, so run the same command with --resume again later",
                unscored.len(),
                data_path.display()
            )
        })?;
        write_records(&data_path, &saved)?;
    }

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
    let mut scorer_failed = false;
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

        // Append each answer, unscored, the moment it arrives; append the
        // scored lines once a batch is scored. A reload keeps the scored
        // line of a customer saved twice.
        let mut append = |answers: &[&SavedAnswer]| -> Result<()> {
            for a in answers {
                serde_json::to_writer(&mut data, a)?;
                data.write_all(b"\n")?;
            }
            data.flush()?;
            Ok(())
        };
        let mut pending: Vec<usize> = Vec::new();
        let score_batch = |saved: &mut Vec<SavedAnswer>,
                           pending: &mut Vec<usize>,
                           append: &mut dyn FnMut(&[&SavedAnswer]) -> Result<()>|
         -> Result<()> {
            let batch = std::mem::take(pending);
            score_pending(&scorer, saved, &batch)?;
            let lines: Vec<&SavedAnswer> = batch.iter().map(|&i| &saved[i]).collect();
            append(&lines)
        };
        let mut answered = 0;
        let mut last_shown = None;
        for (j, answer) in rx {
            let (ci, i) = jobs[j];
            let result = answer.and_then(|answer| {
                let line = SavedAnswer {
                    concept: run_file.concepts[ci].name.clone(),
                    customer: i + 1,
                    attributes: customers[ci][i].attributes.clone(),
                    answer,
                    score: None,
                };
                append(&[&line])?;
                saved.push(line);
                pending.push(saved.len() - 1);
                Ok(())
            });
            let result = result.and_then(|()| {
                answered += 1;
                show_progress(
                    already + answered,
                    total,
                    answered,
                    jobs.len(),
                    started,
                    &mut last_shown,
                );
                if !scorer_failed && pending.len() >= SCORE_BATCH {
                    score_batch(&mut saved, &mut pending, &mut append).inspect_err(|_| {
                        scorer_failed = true;
                    })
                } else {
                    Ok(())
                }
            });
            if let Err(e) = result {
                stop.store(true, Ordering::SeqCst);
                if failure.is_none() {
                    failure = Some(e);
                }
            }
        }
        if !scorer_failed
            && !pending.is_empty()
            && let Err(e) = score_batch(&mut saved, &mut pending, &mut append)
        {
            failure.get_or_insert(e);
        }
    });
    drop(data);

    // Keep the data file in a stable order: by Concept, then customer, one
    // line per customer.
    saved.sort_by_key(|r| {
        let ci = run_file.concepts.iter().position(|c| c.name == r.concept);
        (ci, r.customer)
    });
    write_records(&data_path, &saved)?;
    let records = scored(&saved);
    let unscored = saved.len() - records.len();

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
        let not_scored = if unscored > 0 {
            format!(
                " ({unscored} of them saved but not scored yet: the report leaves them out \
                 until a resume scores them)"
            )
        } else {
            String::new()
        };
        return Err(e.context(format!(
            "the run stopped with {} of {total} Synthetic customers answered{not_scored}; \
             their answers are kept and {} is marked as a short sample. \
             Run the same command with --resume to score the saved answers and ask only \
             the missing customers",
            saved.len(),
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
    // An existing HTML page is kept in step with report.md.
    if html || html_path.exists() {
        std::fs::write(&html_path, report_html::html(&analysis))
            .with_context(|| format!("cannot write {}", html_path.display()))?;
        Ok((report_path, Some(html_path)))
    } else {
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
    let mut saved = load_records(&data_path)?;
    let state_path = out_dir.join(STATE_FILE);
    let mut state: RunState = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .with_context(|| format!("{} is missing or invalid", state_path.display()))?;
    if let Some((scorer, scorer_name)) = rescore {
        let all: Vec<usize> = (0..saved.len()).collect();
        score_pending(scorer, &mut saved, &all)?;
        write_records(&data_path, &saved)?;
        state.scorer = Some(scorer_name.to_string());
        std::fs::write(&state_path, serde_json::to_string(&state)?)
            .with_context(|| format!("cannot write {}", state_path.display()))?;
    }
    let scorer_name = state
        .scorer
        .clone()
        .unwrap_or_else(|| run_file.scorer.as_str().to_string());
    let records = scored(&saved);
    if records.len() < saved.len() {
        eprintln!(
            "warning: {} saved answers are not scored yet and are left out of the report; \
             `run --resume` scores them (or `report --rescore` re-scores everything)",
            saved.len() - records.len()
        );
    }
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
/// A customer saved twice (unscored, then scored) keeps one line, the scored
/// one; two different answers for one customer are an error.
fn load_records(path: &Path) -> Result<Vec<SavedAnswer>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut records: Vec<SavedAnswer> = Vec::new();
    let mut seen: std::collections::HashMap<(String, usize), usize> =
        std::collections::HashMap::new();
    for (n, line) in lines.iter().enumerate() {
        let r: SavedAnswer = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) if n + 1 == lines.len() && !text.ends_with('\n') => continue,
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "cannot resume: line {} of {} is invalid",
                        n + 1,
                        path.display()
                    )
                });
            }
        };
        match seen.get(&(r.concept.clone(), r.customer)) {
            Some(&k) => {
                let kept = &mut records[k];
                if kept.answer != r.answer || kept.attributes != r.attributes {
                    bail!(
                        "cannot resume: customer {} of Concept `{}` appears twice in {}",
                        r.customer,
                        r.concept,
                        path.display()
                    );
                }
                if r.score.is_some() {
                    kept.score = r.score;
                }
            }
            None => {
                seen.insert((r.concept.clone(), r.customer), records.len());
                records.push(r);
            }
        }
    }
    Ok(records)
}

/// Write the data file whole, through a temporary file, so an interruption
/// never leaves it half written.
fn write_records(path: &Path, records: &[SavedAnswer]) -> Result<()> {
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
    use super::{civil_date, load_records};

    #[test]
    fn a_customer_saved_unscored_then_scored_loads_once_scored() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("answers.jsonl");
        let raw =
            r#"{"concept":"A","customer":1,"attributes":{"age":"young"},"answer":"I'd buy it"}"#;
        let scored = r#"{"concept":"A","customer":1,"attributes":{"age":"young"},"answer":"I'd buy it","distribution":[0.0,0.0,0.0,0.0,1.0],"expected_rating":5.0}"#;
        let other = r#"{"concept":"A","customer":2,"attributes":{"age":"old"},"answer":"No"}"#;
        std::fs::write(&path, format!("{raw}\n{other}\n{scored}\n{raw}\n")).unwrap();
        let records = load_records(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].customer, 1);
        assert_eq!(records[0].score.as_ref().unwrap().expected_rating, 5.0);
        assert!(records[1].score.is_none());

        let changed = raw.replace("I'd buy it", "Never");
        std::fs::write(&path, format!("{raw}\n{changed}\n")).unwrap();
        let err = load_records(&path).unwrap_err().to_string();
        assert!(err.contains("appears twice"), "{err}");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(19_723), "2024-01-01");
        assert_eq!(civil_date(11_016), "2000-02-29");
    }
}
