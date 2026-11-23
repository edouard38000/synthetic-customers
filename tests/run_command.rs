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
        "share of `old` must be a finite number above 0",
    );
    assert_rejected(
        &run_file("scorer = \"fake\"\ncustomers_per_concept = 0"),
        "`customers_per_concept` must be at least 1",
    );
    assert_rejected(
        &run_file("scorer = \"fake\"\nconcurrency = 0"),
        "`concurrency` must be at least 1",
    );
    assert_rejected(
        &run_file("scorer = \"fake\"\ncustomers_per_concept = 1000000000000000"),
        "`customers_per_concept` must be at most 10000",
    );
    assert_rejected(
        &run_file("scorer = \"fake\"\nconcurrency = 65"),
        "`concurrency` must be at most 64",
    );
    assert_rejected(
        &format!("scorer = \"fake\"\n{TWO_CONCEPTS}\n[customer_mix.age]\n\"\" = 1.0\n"),
        "value with an empty name",
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

fn run_with(f: &Fixture, extra: &[&str]) -> std::process::Output {
    Command::cargo_bin("synthetic-customers")
        .unwrap()
        .arg("run")
        .arg(f.path("run.toml"))
        .arg("--out")
        .arg(f.path("out"))
        .arg("--fake-answers")
        .arg(f.path("answers.json"))
        .arg("--fake-calls-log")
        .arg(f.path("calls.log"))
        .args(extra)
        .output()
        .unwrap()
}

fn calls(f: &Fixture) -> Vec<String> {
    std::fs::read_to_string(f.path("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_full_run_asks_each_customer_exactly_once_concurrently() {
    let f = Fixture::new(&run_file(
        "scorer = \"fake\"\nseed = 3\ncustomers_per_concept = 30\nconcurrency = 4",
    ));
    let output = run_with(&f, &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    let mut log = calls(&f);
    assert_eq!(log.len(), 60);
    log.sort();
    log.dedup();
    assert_eq!(log.len(), 60, "no customer asked twice");
    let err = stderr(&output);
    assert!(
        err.contains("Asking 60 of 60 Synthetic customers, 4 at a time: under a minute."),
        "{err}"
    );
    assert!(err.contains("Answered 60/60 (100%)"), "{err}");
    let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
    assert!(!report.contains("Short sample"), "{report}");
}

#[test]
fn a_stopped_run_keeps_its_answers_marks_a_short_sample_and_resumes_only_missing_customers() {
    let f = Fixture::new(&run_file(
        "scorer = \"fake\"\nseed = 11\ncustomers_per_concept = 40\nconcurrency = 5",
    ));

    // The fake usage limit stops the run after 25 answers.
    let output = run_with(&f, &["--fake-fail-after", "25"]);
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("fake usage limit reached"), "{err}");
    assert!(
        err.contains("the run stopped with 25 of 80 Synthetic customers answered"),
        "{err}"
    );
    assert!(err.contains("--resume"), "{err}");
    let partial = read_data(&f.path("out/answers.jsonl"));
    assert_eq!(partial.len(), 25, "answers kept");
    let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
    assert!(report.contains("**Short sample.**"), "{report}");
    assert!(
        report.contains("25 of the 80 planned Synthetic customers answered"),
        "{report}"
    );
    let first_calls = calls(&f).len();

    // Running again without --resume must not overwrite paid-for answers.
    let output = run_with(&f, &[]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("pass --resume"),
        "{}",
        stderr(&output)
    );
    assert_eq!(calls(&f).len(), first_calls, "no customer asked");
    assert_eq!(read_data(&f.path("out/answers.jsonl")).len(), 25);

    // Resume asks only the 55 missing customers.
    let output = run_with(&f, &["--resume"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("Asking 55 of 80 Synthetic customers (25 already answered)"),
        "{}",
        stderr(&output)
    );
    let resumed_calls: Vec<String> = calls(&f)[first_calls..].to_vec();
    assert_eq!(resumed_calls.len(), 55);
    let answered_before: Vec<String> = partial
        .iter()
        .map(|r| {
            format!(
                "{}\t{}",
                r["concept"].as_str().unwrap(),
                r["customer"].as_u64().unwrap() - 1
            )
        })
        .collect();
    for call in &resumed_calls {
        assert!(!answered_before.contains(call), "asked again: {call}");
    }

    // The finished data file is complete, ordered, and keeps the first answers.
    let rows = read_data(&f.path("out/answers.jsonl"));
    assert_eq!(rows.len(), 80);
    for concept in ["Mint toothpaste", "Charcoal toothpaste"] {
        let customers: Vec<u64> = rows
            .iter()
            .filter(|r| r["concept"] == concept)
            .map(|r| r["customer"].as_u64().unwrap())
            .collect();
        assert_eq!(customers, (1..=40).collect::<Vec<_>>());
    }
    for r in &partial {
        assert!(rows.contains(r), "kept: {r}");
    }
    let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
    assert!(!report.contains("Short sample"), "{report}");
    assert!(report.contains("| Mint toothpaste | 40 |"), "{report}");

    // The resumed run has the same customers as one that never stopped.
    let g = Fixture::new(&run_file(
        "scorer = \"fake\"\nseed = 11\ncustomers_per_concept = 40\nconcurrency = 5",
    ));
    assert!(run_with(&g, &[]).status.success());
    assert_eq!(
        std::fs::read_to_string(f.path("out/answers.jsonl")).unwrap(),
        std::fs::read_to_string(g.path("out/answers.jsonl")).unwrap()
    );

    // Resuming a finished run asks nobody and rebuilds the report.
    let output = run_with(&f, &["--resume"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(calls(&f).len(), first_calls + 55);
    assert!(f.path("out/report.md").exists());
}

#[test]
fn resume_without_a_seed_reuses_the_first_runs_customers() {
    let f = Fixture::new(&run_file("scorer = \"fake\"\ncustomers_per_concept = 20"));
    assert!(!run_with(&f, &["--fake-fail-after", "7"]).status.success());
    let first_calls = calls(&f).len();
    let output = run_with(&f, &["--resume"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(calls(&f).len(), first_calls + 33);
    assert_eq!(read_data(&f.path("out/answers.jsonl")).len(), 40);
}

#[test]
fn resume_survives_a_last_line_cut_short_by_an_interruption() {
    let f = Fixture::new(&run_file("scorer = \"fake\"\ncustomers_per_concept = 10"));
    assert!(!run_with(&f, &["--fake-fail-after", "5"]).status.success());
    let first_calls = calls(&f).len();
    let data = f.path("out/answers.jsonl");
    let mut text = std::fs::read_to_string(&data).unwrap();
    text.push_str("{\"concept\":\"Mint too");
    std::fs::write(&data, text).unwrap();
    let output = run_with(&f, &["--resume"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(calls(&f).len(), first_calls + 15);
    assert_eq!(read_data(&data).len(), 20);
}

#[test]
fn resume_refuses_a_changed_run_file() {
    let f = Fixture::new(&run_file("scorer = \"fake\"\ncustomers_per_concept = 10"));
    assert!(!run_with(&f, &["--fake-fail-after", "5"]).status.success());
    let first_calls = calls(&f).len();
    std::fs::write(
        f.path("run.toml"),
        run_file("scorer = \"fake\"\ncustomers_per_concept = 12"),
    )
    .unwrap();
    let output = run_with(&f, &["--resume"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("the run file has changed"),
        "{}",
        stderr(&output)
    );
    assert_eq!(calls(&f).len(), first_calls, "no customer asked");
}

/// The real Respondent, with a stand-in `claude` on the PATH that records
/// its arguments and prompt (the real `claude` is never called in tests).
#[cfg(unix)]
#[test]
fn claude_respondent_runs_claude_p_with_the_model_and_the_papers_prompt() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new(&format!(
        "scorer = \"fake\"\nmodel = \"sonnet\"\ncustomers_per_concept = 3\nconcurrency = 2\n\
         [[concepts]]\nname = \"Mint\"\ntext = \"A toothpaste with real mint leaves.\"\nimage = \"mint.png\"\n{MIX}"
    ));
    std::fs::write(f.path("mint.png"), b"not really a png").unwrap();
    let bin = f.path("bin");
    std::fs::create_dir(&bin).unwrap();
    let log = f.path("claude.log");
    let script = format!(
        "#!/bin/sh\necho \"ARGS: $*\" >> '{}'\ncat >> '{}'\necho 'I would probably buy it, 4.'\n",
        log.display(),
        log.display()
    );
    std::fs::write(bin.join("claude"), script).unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::cargo_bin("synthetic-customers")
        .unwrap()
        .arg("run")
        .arg(f.path("run.toml"))
        .arg("--out")
        .arg(f.path("out"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let log = std::fs::read_to_string(&log).unwrap();
    assert_eq!(log.matches("ARGS: -p --model sonnet").count(), 3, "{log}");
    assert_eq!(
        log.matches("How likely are you to purchase the product?")
            .count(),
        3
    );
    assert!(log.contains("Impersonate a consumer with the following attributes"));
    assert!(log.contains("A toothpaste with real mint leaves."));
    assert!(log.contains("--tools Read"), "{log}");
    let image = std::path::absolute(f.path("mint.png")).unwrap();
    assert!(log.contains(&image.display().to_string()), "{log}");

    let rows = read_data(&f.path("out/answers.jsonl"));
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|r| r["answer"] == "I would probably buy it, 4.")
    );
    assert!(
        std::fs::read_to_string(f.path("out/report.md"))
            .unwrap()
            .contains("- Model playing the customers: sonnet")
    );
}

#[cfg(unix)]
#[test]
fn a_failing_claude_stops_the_run_with_its_error() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new(&run_file("scorer = \"fake\"\ncustomers_per_concept = 4"));
    let bin = f.path("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(
        bin.join("claude"),
        "#!/bin/sh\ncat > /dev/null\necho 'Usage limit reached' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::cargo_bin("synthetic-customers")
        .unwrap()
        .arg("run")
        .arg(f.path("run.toml"))
        .arg("--out")
        .arg(f.path("out"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("claude -p failed"), "{err}");
    assert!(err.contains("Usage limit reached"), "{err}");
    assert!(err.contains("0 of 8 Synthetic customers answered"), "{err}");
    assert!(
        std::fs::read_to_string(f.path("out/report.md"))
            .unwrap()
            .contains("**Short sample.**")
    );
}

#[test]
fn codex_ignores_the_model_with_a_warning_and_a_missing_image_is_an_error() {
    let f = Fixture::new(&run_file(
        "scorer = \"fake\"\nrespondent = \"codex\"\nmodel = \"opus\"\ncustomers_per_concept = 2",
    ));
    let output = f.run();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("`model` is ignored"));
    assert!(
        std::fs::read_to_string(f.path("out/report.md"))
            .unwrap()
            .contains("- Model playing the customers: codex")
    );

    let f = Fixture::new(&format!(
        "scorer = \"fake\"\n[[concepts]]\nname = \"A\"\ntext = \"An idea\"\nimage = \"missing.png\"\n{MIX}"
    ));
    let output = f.run();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("the image of Concept `A` is not a file"),
        "{}",
        stderr(&output)
    );
}
