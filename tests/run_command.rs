//! The run command end to end, through the binary, with the fake Respondent
//! (canned answers) and the fake Scorer.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tempfile::TempDir;

const MIX: &str = r#"
[customer_mix.age]
"18-34" = 0.25
"35-54" = 0.35
"55+" = 0.40

[customer_mix.gender]
"woman" = 0.5
"man" = 0.5

[customer_mix.income]
"low" = 0.333
"middle" = 0.333
"high" = 0.334
"#;

const TWO_CONCEPTS: &str = r#"
[[concepts]]
name = "Mint toothpaste"
text = "A toothpaste with real mint leaves."

[[concepts]]
name = "Charcoal toothpaste"
text = "A black toothpaste with activated charcoal."
"#;

// Mint: 5, 4, "3 or 4" (half 3, half 4), "no idea" (uniform) → expected
// ratings 5, 4, 3.5, 3 → average 3.875. Charcoal: 1, 2 → average 1.5.
const ANSWERS: &str = r#"{
  "Mint toothpaste": ["5", "4", "maybe 3 or 4", "no idea"],
  "Charcoal toothpaste": ["1", "2"]
}"#;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new(run_file: &str) -> Self {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("run.toml"), run_file).unwrap();
        std::fs::write(dir.path().join("answers.json"), ANSWERS).unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    fn run(&self) -> std::process::Output {
        Command::cargo_bin("synthetic-customers")
            .unwrap()
            .arg("run")
            .arg(self.path("run.toml"))
            .arg("--out")
            .arg(self.path("out"))
            .arg("--fake-answers")
            .arg(self.path("answers.json"))
            .output()
            .unwrap()
    }
}

fn run_file(head: &str) -> String {
    format!("{head}\n{TWO_CONCEPTS}\n{MIX}")
}

fn read_data(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn run_writes_report_and_data_file_with_correct_counts_and_averages() {
    let f = Fixture::new(&run_file("scorer = \"fake\"\nseed = 7"));
    let output = f.run();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        !stderr(&output).contains("warning"),
        "default 200 customers should not warn: {}",
        stderr(&output)
    );

    let rows = read_data(&f.path("out/answers.jsonl"));
    assert_eq!(rows.len(), 400, "200 customers per Concept, two Concepts");
    for concept in ["Mint toothpaste", "Charcoal toothpaste"] {
        let rows: Vec<&Value> = rows.iter().filter(|r| r["concept"] == concept).collect();
        assert_eq!(rows.len(), 200);
        let customers: Vec<u64> = rows
            .iter()
            .map(|r| r["customer"].as_u64().unwrap())
            .collect();
        assert_eq!(customers, (1..=200).collect::<Vec<_>>());
        for r in &rows {
            let d: Vec<f64> = r["distribution"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_f64().unwrap())
                .collect();
            assert_eq!(d.len(), 5);
            assert!((d.iter().sum::<f64>() - 1.0).abs() < 1e-9);
            assert!(r["answer"].is_string());
            assert!(r["attributes"]["age"].is_string());
        }
    }
    let mean = |concept: &str| {
        let ratings: Vec<f64> = rows
            .iter()
            .filter(|r| r["concept"] == concept)
            .map(|r| r["expected_rating"].as_f64().unwrap())
            .collect();
        ratings.iter().sum::<f64>() / ratings.len() as f64
    };
    assert!((mean("Mint toothpaste") - 3.875).abs() < 1e-9);
    assert!((mean("Charcoal toothpaste") - 1.5).abs() < 1e-9);

    let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
    // Mint spread: rating 1 = (0+0+0+.2)/4 = 5%, 2 = 5%, 3 = (.5+.2)/4 = 17.5%,
    // 4 = (1+.5+.2)/4 = 42.5%, 5 = (1+.2)/4 = 30%.
    assert!(
        report.contains("| Mint toothpaste | 200 | 3.88 | 5.0% | 5.0% | 17.5% | 42.5% | 30.0% |"),
        "{report}"
    );
    assert!(
        report
            .contains("| Charcoal toothpaste | 200 | 1.50 | 50.0% | 50.0% | 0.0% | 0.0% | 0.0% |"),
        "{report}"
    );
    assert!(report.contains("- Model playing the customers: haiku"));
    assert!(report.contains("- Scorer: fake"));
    assert!(report.contains("- Customers per Concept: 200"));
    assert!(
        f.path("out/run.toml").exists(),
        "run file saved with results"
    );
}

#[test]
fn customer_mix_sampling_matches_the_stated_shares() {
    let f = Fixture::new(&run_file("scorer = \"fake\"\nseed = 1"));
    assert!(f.run().status.success());
    let rows = read_data(&f.path("out/answers.jsonl"));
    for concept in ["Mint toothpaste", "Charcoal toothpaste"] {
        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for r in rows.iter().filter(|r| r["concept"] == concept) {
            for (attribute, value) in r["attributes"].as_object().unwrap() {
                *counts
                    .entry((attribute.clone(), value.as_str().unwrap().to_string()))
                    .or_default() += 1;
            }
        }
        let count = |a: &str, v: &str| counts[&(a.to_string(), v.to_string())];
        // 200 × .25 / .35 / .40
        assert_eq!(count("age", "18-34"), 50);
        assert_eq!(count("age", "35-54"), 70);
        assert_eq!(count("age", "55+"), 80);
        assert_eq!(count("gender", "woman"), 100);
        assert_eq!(count("gender", "man"), 100);
        // 200 × .333 / .333 / .334 = 66.6 / 66.6 / 66.8 → largest remainders
        // give the two extra customers to "high" then "low".
        assert_eq!(count("income", "low"), 67);
        assert_eq!(count("income", "middle"), 66);
        assert_eq!(count("income", "high"), 67);
    }
}

#[test]
fn same_seed_gives_the_same_synthetic_customers() {
    let a = Fixture::new(&run_file("scorer = \"fake\"\nseed = 99"));
    let b = Fixture::new(&run_file("scorer = \"fake\"\nseed = 99"));
    assert!(a.run().status.success());
    assert!(b.run().status.success());
    let data_a = std::fs::read_to_string(a.path("out/answers.jsonl")).unwrap();
    let data_b = std::fs::read_to_string(b.path("out/answers.jsonl")).unwrap();
    assert_eq!(data_a, data_b);
}

#[test]
fn customers_outside_150_to_400_warn_but_run() {
    for (n, warns) in [
        (149, true),
        (150, false),
        (400, false),
        (401, true),
        (10, true),
    ] {
        let f = Fixture::new(&run_file(&format!(
            "scorer = \"fake\"\ncustomers_per_concept = {n}"
        )));
        let output = f.run();
        assert!(output.status.success(), "n = {n}: {}", stderr(&output));
        let err = stderr(&output);
        assert_eq!(
            err.contains("warning: customers_per_concept"),
            warns,
            "n = {n}: {err}"
        );
        if warns {
            assert!(err.contains("150–400"), "{err}");
        }
        assert_eq!(read_data(&f.path("out/answers.jsonl")).len(), 2 * n);
    }
}

fn assert_rejected(run_file: &str, message: &str) {
    let f = Fixture::new(run_file);
    let output = f.run();
    assert!(!output.status.success(), "should fail: {run_file}");
    let err = stderr(&output);
    assert!(err.contains("invalid run file"), "{err}");
    assert!(err.contains(message), "expected `{message}` in: {err}");
    assert!(!f.path("out/report.md").exists());
}

#[test]
fn invalid_run_files_get_clear_errors() {
    assert_rejected(&format!("scorer = \"fake\"\n{MIX}"), "no Concepts");
    assert_rejected(
        &format!("scorer = \"fake\"\n{TWO_CONCEPTS}"),
        "empty Customer mix",
    );
    assert_rejected(
        &format!(
            "scorer = \"fake\"\n{TWO_CONCEPTS}\n[customer_mix.age]\n\"young\" = 25\n\"old\" = 75\n"
        ),
        "shares add up to 100.000, they must add up to 1",
    );
    assert_rejected(
        &format!(
            "scorer = \"fake\"\n{TWO_CONCEPTS}\n[customer_mix.age]\n\"young\" = 0.6\n\"old\" = 0.6\n"
        ),
        "shares add up to 1.200",
    );
    assert_rejected(
        &format!(
            "scorer = \"fake\"\n{TWO_CONCEPTS}\n[customer_mix.age]\n\"young\" = 1.0\n\"old\" = 0\n"
        ),
        "share of `old` must be above 0",
    );
    assert_rejected(
        &run_file("scorer = \"fake\"\ncustomers_per_concept = 0"),
        "`customers_per_concept` must be at least 1",
    );
    assert_rejected(
        &run_file("scorer = \"fake\"\nconcurrency = 0"),
        "`concurrency` must be at least 1",
    );
    assert_rejected(&run_file("scorer = \"fake\"\nmodel = \"gpt-5\""), "gpt-5");
    assert_rejected(&run_file("scorer = \"magic\""), "magic");
    assert_rejected(
        &run_file("scorer = \"fake\"\ncustomer_count = 5"),
        "customer_count",
    );
    assert_rejected(
        &format!(
            "scorer = \"fake\"\n{TWO_CONCEPTS}\n[[concepts]]\nname = \"Mint toothpaste\"\ntext = \"again\"\n{MIX}"
        ),
        "two Concepts are named `Mint toothpaste`",
    );
}

#[test]
fn real_scorers_are_not_available_yet() {
    // No `scorer` line: the default is the local Scorer.
    let f = Fixture::new(&run_file(""));
    let output = f.run();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("scorer `local` is not available in this version yet"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn real_synthetic_customers_are_not_available_yet() {
    let f = Fixture::new(&run_file("scorer = \"fake\""));
    let output = Command::cargo_bin("synthetic-customers")
        .unwrap()
        .arg("run")
        .arg(f.path("run.toml"))
        .arg("--out")
        .arg(f.path("out"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("real Synthetic customers"));
}
