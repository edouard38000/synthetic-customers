//! Compare the local Scorer's candidate models on the gold set.
//!
//! ```text
//! cargo run --release --example compare_scorers            # every candidate
//! cargo run --release --example compare_scorers -- gte-small bge-small-en-v1.5
//! cargo run --release --example compare_scorers -- --clean # then delete non-default models
//! ```
//!
//! Downloads each model into the model cache, measures its agreement with
//! `tests/data/gold_set.json` and its speed, and prints a Markdown table.
//! One model is in memory at a time. With `--clean`, every model except the
//! default is deleted from the cache once measured.

use anyhow::Result;
use std::path::Path;
use std::time::Instant;
use synthetic_customers::anchors::anchor_sets;
use synthetic_customers::embedder::Embedder;
use synthetic_customers::gold;
use synthetic_customers::local_scorer::{self, CANDIDATES, DEFAULT_MODEL, LocalEmbedder};
use synthetic_customers::ssr::{DEFAULT_EPSILON, SsrScorer};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let clean = args.iter().any(|a| a == "--clean");
    let names: Vec<&String> = args.iter().filter(|a| *a != "--clean").collect();
    let cache = local_scorer::cache_dir()?;
    let gold = gold::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gold_set.json"))?;
    let sets = anchor_sets();
    let answers: Vec<&str> = gold.iter().map(|g| g.answer.as_str()).collect();

    println!("| Model | Size (MB) | Spearman | MAE | Load (s) | ms per answer |");
    println!("|---|---:|---:|---:|---:|---:|");
    for model in CANDIDATES {
        if !names.is_empty() && !names.iter().any(|n| *n == model.name) {
            continue;
        }
        model.download(&cache)?;
        let start = Instant::now();
        let embedder = LocalEmbedder::load(model, &cache)?;
        let load = start.elapsed().as_secs_f64();
        let ssr = SsrScorer::new(&embedder, &sets, DEFAULT_EPSILON)?;
        let (agreement, _) = gold::measure(&ssr, &gold)?;
        // Speed: embed the gold answers in batches, as a run does.
        let start = Instant::now();
        embedder.embed(&answers)?;
        let per_answer = start.elapsed().as_secs_f64() * 1000.0 / answers.len() as f64;
        println!(
            "| {} | {} | {:.3} | {:.3} | {:.1} | {:.1} |",
            model.name, model.size_mb, agreement.spearman, agreement.mae, load, per_answer
        );
    }
    // After the loop: two candidates can share one repository.
    if clean {
        for model in CANDIDATES {
            if model.repo != DEFAULT_MODEL.repo {
                model.remove(&cache)?;
            }
        }
    }
    Ok(())
}
