//! The Markdown report (basic: spread and average Purchase intent per Concept).

use crate::run::ConceptSummary;
use crate::run_file::RunFile;
use std::fmt::Write;

pub fn markdown(
    run_file: &RunFile,
    scorer_name: &str,
    seed: u64,
    summaries: &[ConceptSummary],
) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Synthetic customers report\n");
    let _ = writeln!(
        md,
        "Purchase intent on a 1–5 scale (1 = would definitely not buy, 5 = would definitely buy), \
         scored with semantic similarity rating (SSR, arXiv 2510.08338).\n"
    );

    let _ = writeln!(md, "## Purchase intent per Concept\n");
    let _ = writeln!(
        md,
        "| Concept | Customers | Average Purchase intent | 1 | 2 | 3 | 4 | 5 |"
    );
    let _ = writeln!(md, "|---|---:|---:|---:|---:|---:|---:|---:|");
    for s in summaries {
        let _ = write!(
            md,
            "| {} | {} | {:.2} |",
            escape_cell(&s.concept),
            s.customers,
            s.average
        );
        for p in s.spread {
            let _ = write!(md, " {:.1}% |", p * 100.0);
        }
        let _ = writeln!(md);
    }
    let _ = writeln!(
        md,
        "\nThe 1–5 columns are the spread: the share of Purchase intent at each rating, \
         averaged over the Synthetic customers.\n"
    );

    let _ = writeln!(md, "## Run details\n");
    let _ = writeln!(
        md,
        "- Model playing the customers: {}",
        run_file.model.as_str()
    );
    let _ = writeln!(md, "- Scorer: {scorer_name}");
    let _ = writeln!(
        md,
        "- Customers per Concept: {}",
        run_file.customers_per_concept
    );
    let _ = writeln!(md, "- Seed: {seed}");
    md
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}
