//! The run command: sample Synthetic customers, ask each, score, report.

use crate::anchors::{AnchorSet, RATINGS};
use crate::customer_mix::{Rng, SyntheticCustomer};
use crate::embedder::Embedder;
use crate::report;
use crate::respondent::Respondent;
use crate::run_file::RunFile;
use crate::ssr::{self, Distribution, SsrScorer};
use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const REPORT_FILE: &str = "report.md";
pub const DATA_FILE: &str = "answers.jsonl";
pub const RUN_FILE_COPY: &str = "run.toml";

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

pub struct RunOutput {
    pub report: PathBuf,
    pub data: PathBuf,
}

/// Everything a run needs besides the run file; tests swap in fakes here.
pub struct RunSetup<'a> {
    pub respondent: &'a dyn Respondent,
    pub embedder: &'a dyn Embedder,
    pub anchor_sets: Vec<AnchorSet>,
    pub epsilon: f64,
    pub seed: u64,
}

pub fn run(
    run_file: &RunFile,
    run_file_text: &str,
    setup: RunSetup<'_>,
    out_dir: &Path,
) -> Result<RunOutput> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create output folder {}", out_dir.display()))?;
    std::fs::write(out_dir.join(RUN_FILE_COPY), run_file_text)
        .context("cannot save a copy of the run file")?;

    let scorer = SsrScorer::new(setup.embedder, &setup.anchor_sets, setup.epsilon)?;
    let data_path = out_dir.join(DATA_FILE);
    let mut data = std::io::BufWriter::new(
        std::fs::File::create(&data_path)
            .with_context(|| format!("cannot write {}", data_path.display()))?,
    );

    let mut rng = Rng::new(setup.seed);
    let mut records = Vec::new();
    for concept in &run_file.concepts {
        let customers: Vec<SyntheticCustomer> = run_file
            .customer_mix
            .sample(run_file.customers_per_concept, &mut rng);
        for (i, customer) in customers.iter().enumerate() {
            let answer = setup
                .respondent
                .answer(concept, i, customer)
                .with_context(|| format!("customer {} of Concept `{}`", i + 1, concept.name))?;
            let distribution = scorer.score(&answer)?;
            let record = AnswerRecord {
                concept: concept.name.clone(),
                customer: i + 1,
                attributes: customer.attributes.clone(),
                answer,
                expected_rating: ssr::expected_rating(&distribution),
                distribution,
            };
            serde_json::to_writer(&mut data, &record)?;
            data.write_all(b"\n")?;
            records.push(record);
        }
    }
    data.flush()?;

    let summaries = summarise(run_file, &records);
    let report_path = out_dir.join(REPORT_FILE);
    let markdown = report::markdown(run_file, setup.embedder.name(), setup.seed, &summaries);
    std::fs::write(&report_path, markdown)
        .with_context(|| format!("cannot write {}", report_path.display()))?;

    Ok(RunOutput {
        report: report_path,
        data: data_path,
    })
}

/// Per-Concept results: the spread is the mean distribution, the average
/// Purchase intent is the mean expected rating.
#[derive(Debug, Clone)]
pub struct ConceptSummary {
    pub concept: String,
    pub customers: usize,
    pub average: f64,
    pub spread: Distribution,
}

pub fn summarise(run_file: &RunFile, records: &[AnswerRecord]) -> Vec<ConceptSummary> {
    run_file
        .concepts
        .iter()
        .map(|c| {
            let rows: Vec<&AnswerRecord> = records.iter().filter(|r| r.concept == c.name).collect();
            let n = rows.len();
            let mut spread = [0.0; RATINGS];
            let mut average = 0.0;
            for r in &rows {
                for (s, p) in spread.iter_mut().zip(&r.distribution) {
                    *s += p;
                }
                average += r.expected_rating;
            }
            if n > 0 {
                spread = spread.map(|s| s / n as f64);
                average /= n as f64;
            }
            ConceptSummary {
                concept: c.name.clone(),
                customers: n,
                average,
                spread,
            }
        })
        .collect()
}
