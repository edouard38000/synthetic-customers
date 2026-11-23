//! The full report end to end, through the binary, with the fake Respondent
//! (canned answers) and the fake Scorer: ranking, breakdowns, quotes, limits,
//! HTML, and re-reporting a saved run without asking anyone.

use assert_cmd::Command;
use serde_json::Value;
use std::path::PathBuf;
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

[customer_mix.region]
"Midwest" = 0.5
"South" = 0.5
"#;

const CONCEPTS: &str = r#"
[[concepts]]
name = "Mint toothpaste"
text = "A toothpaste with real mint leaves."

[[concepts]]
name = "Mint twin"
text = "The same toothpaste with another box."

[[concepts]]
name = "Charcoal toothpaste"
text = "A black toothpaste with activated charcoal."
"#;

const LONG: &str = "5 I would buy this at once because the mint taste sounds fresh and natural, my whole family brushes twice a day and we run out of toothpaste every few weeks, so a better tasting one that is also gentle on the gums would be an easy choice for our bathroom shelf. Really.";

fn answers() -> String {
    let mint = serde_json::json!([
        "5 I love real mint, I would buy it for my family.",
        "4 Sounds fresh and I like natural ingredients.",
        "3 Maybe, if the price is fair.",
        "2 Not sure I need another toothpaste.",
        "1 Too expensive for a toothpaste, I would never pay that.",
        LONG,
    ]);
    serde_json::json!({
        "Mint toothpaste": mint,
        "Mint twin": mint,
        "Charcoal toothpaste": ["1 Black toothpaste looks dirty, no thanks.", "2 I do not trust charcoal on my teeth <script>alert('x')</script> & more."],
    })
    .to_string()
}

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new(run_file: &str) -> Self {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("run.toml"), run_file).unwrap();
        std::fs::write(dir.path().join("answers.json"), answers()).unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn run(&self, extra: &[&str]) -> std::process::Output {
        Command::cargo_bin("synthetic-customers")
            .unwrap()
            .arg("run")
            .arg(self.path("run.toml"))
            .arg("--out")
            .arg(self.path("out"))
            .arg("--fake-answers")
            .arg(self.path("answers.json"))
            .arg("--fake-calls-log")
            .arg(self.path("calls.log"))
            .args(extra)
            .output()
            .unwrap()
    }

    fn report_cmd(&self, extra: &[&str]) -> std::process::Output {
        Command::cargo_bin("synthetic-customers")
            .unwrap()
            .arg("report")
            .arg(self.path("out"))
            .args(extra)
            .output()
            .unwrap()
    }

    fn calls(&self) -> usize {
        std::fs::read_to_string(self.path("calls.log"))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    fn md(&self) -> String {
        std::fs::read_to_string(self.path("out/report.md")).unwrap()
    }
}

fn full_run_file() -> String {
    format!("scorer = \"fake\"\nseed = 5\ncustomers_per_concept = 120\n{CONCEPTS}\n{MIX}")
}

fn ok(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_data(f: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(f.path("out/answers.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn ranking_has_a_band_and_calls_overlapping_concepts_too_close() {
    let f = Fixture::new(&full_run_file());
    ok(&f.run(&[]));
    let md = f.md();
    let rank_row = |name: &str| {
        md.lines()
            .find(|l| {
                l.starts_with('|') && l.contains(&format!("| {name} |")) && l.contains(" to ")
            })
            .unwrap_or_else(|| panic!("no ranking row for {name}: {md}"))
            .to_string()
    };
    // Same answers, same scores: the bands overlap completely.
    assert!(
        rank_row("Mint toothpaste").contains("too close to call with Mint twin"),
        "{md}"
    );
    assert!(
        rank_row("Mint twin").contains("too close to call with Mint toothpaste"),
        "{md}"
    );
    // Charcoal averages 1.5 against about 3.2: clearly last.
    let charcoal = rank_row("Charcoal toothpaste");
    assert!(charcoal.starts_with("| 3 |"), "{charcoal}");
    assert!(charcoal.contains("clear of the others"), "{charcoal}");
    assert!(charcoal.contains("| 1.50 |"), "{charcoal}");
    assert!(
        md.contains("Too close to call for first place: Mint toothpaste, Mint twin."),
        "{md}"
    );
    // Mint: ratings 5, 4, 3, 2, 1, 5 → average 3.333.
    assert!(rank_row("Mint toothpaste").contains("| 3.33 |"), "{md}");
}

#[test]
fn a_clear_winner_is_named_and_the_band_is_deterministic() {
    let run = format!(
        "scorer = \"fake\"\nseed = 5\ncustomers_per_concept = 60\n{}\n{MIX}",
        CONCEPTS.replace(
            "[[concepts]]\nname = \"Mint twin\"\ntext = \"The same toothpaste with another box.\"\n",
            ""
        )
    );
    let f = Fixture::new(&run);
    ok(&f.run(&[]));
    let md = f.md();
    assert!(
        md.contains("Mint toothpaste leads, clear of the others"),
        "{md}"
    );
    // The same saved answers always give the same report, byte for byte.
    ok(&f.report_cmd(&[]));
    assert_eq!(md, f.md());
}

#[test]
fn age_and_income_are_broken_down_and_gender_and_region_are_left_out() {
    let f = Fixture::new(&full_run_file());
    ok(&f.run(&[]));
    let md = f.md();
    assert!(md.contains("### Age group (`age`)"), "{md}");
    assert!(md.contains("### Income (`income`)"), "{md}");
    for group in ["18-34", "35-54", "55+", "low", "middle", "high"] {
        assert!(
            md.lines()
                .any(|l| l.starts_with(&format!("| {group} |")) && l.contains("(n=")),
            "no row for {group}: {md}"
        );
    }
    assert!(!md.contains("### Gender"), "{md}");
    assert!(!md.contains("### Region"), "{md}");
    assert!(
        !md.lines()
            .any(|l| l.starts_with("| woman |") || l.starts_with("| Midwest |")),
        "{md}"
    );
    assert!(
        md.contains("Left out on purpose: `gender`, `region`."),
        "{md}"
    );
    // The age rows add up to the Concept's customers.
    let rows = read_data(&f);
    let n_18_34 = rows
        .iter()
        .filter(|r| r["concept"] == "Mint toothpaste" && r["attributes"]["age"] == "18-34")
        .count();
    let mean: f64 = rows
        .iter()
        .filter(|r| r["concept"] == "Mint toothpaste" && r["attributes"]["age"] == "18-34")
        .map(|r| r["expected_rating"].as_f64().unwrap())
        .sum::<f64>()
        / n_18_34 as f64;
    assert!(
        md.contains(&format!("| 18-34 | {mean:.2} (n={n_18_34}) |")),
        "{md}"
    );
}

#[test]
fn a_mix_without_age_or_income_says_so_and_shows_no_breakdown() {
    let run = format!(
        "scorer = \"fake\"\ncustomers_per_concept = 20\n{CONCEPTS}\n\
         [customer_mix.language]\n\"English\" = 0.5\n\"Spanish\" = 0.5\n\
         [customer_mix.image_style]\n\"a\" = 1.0\n"
    );
    let f = Fixture::new(&run);
    ok(&f.run(&[]));
    let md = f.md();
    assert!(
        md.contains("no age or income attribute, so there is no breakdown"),
        "{md}"
    );
    assert!(
        !md.contains("### Age group") && !md.contains("### Income"),
        "{md}"
    );
    assert!(md.contains("`language`"), "{md}");
}

#[test]
fn quotes_are_real_saved_answers_short_and_on_the_right_side() {
    let f = Fixture::new(&full_run_file());
    ok(&f.run(&[]));
    let md = f.md();
    let rows = read_data(&f);
    let quotes: Vec<&str> = md.lines().filter_map(|l| l.strip_prefix("> \"")).collect();
    assert!(!quotes.is_empty(), "{md}");
    for q in quotes {
        let text = q.split("\"").next().unwrap().trim_end_matches(['…', ' ']);
        let text = text.replace("&lt;", "<").replace("&gt;", ">");
        let text = text.as_str();
        assert!(
            rows.iter()
                .any(|r| r["answer"].as_str().unwrap().contains(text)),
            "not a saved answer: {text}"
        );
        assert!(text.chars().count() <= 221, "too long: {text}");
    }
    let section = |from: &str, to: &str| {
        let start = md.find(from).unwrap_or_else(|| panic!("no {from}: {md}"));
        let rest = &md[start..];
        rest[..rest.find(to).unwrap_or(rest.len())].to_string()
    };
    let mint = section("### Mint toothpaste\n\n**Top reasons", "### Mint twin");
    let reasons = section_between(&mint, "**Top reasons to buy**", "**Top objections**");
    let objections = section_between(&mint, "**Top objections**", "\u{0}");
    assert!(reasons.contains("I love real mint"), "{reasons}");
    assert!(!reasons.contains("Too expensive"), "{reasons}");
    assert!(
        objections.contains("Too expensive for a toothpaste"),
        "{objections}"
    );
    assert!(!objections.contains("I love real mint"), "{objections}");
    // The long answer is cut at a sentence, never reworded.
    assert!(!md.contains("Really."), "{md}");
    assert!(
        md.contains("my whole family brushes twice a day"),
        "long answer quoted: {md}"
    );
    // Who said it: age and income only, never gender or region.
    assert!(
        !reasons.contains("woman") && !reasons.contains("Midwest"),
        "{reasons}"
    );
}

fn section_between(text: &str, from: &str, to: &str) -> String {
    let start = text.find(from).unwrap();
    let rest = &text[start + from.len()..];
    rest[..rest.find(to).unwrap_or(rest.len())].to_string()
}

#[test]
fn limits_credits_and_run_details_are_always_there() {
    // A short sample with one Concept and no age or income: the sparsest report.
    let run = String::from(
        "scorer = \"fake\"\nmodel = \"sonnet\"\nseed = 3\ncustomers_per_concept = 40\n\
         [[concepts]]\nname = \"Only\"\ntext = \"One idea.\"\n\
         [customer_mix.region]\n\"Midwest\" = 1.0\n",
    );
    let f = Fixture::new(&run);
    let mut ans = serde_json::from_str::<Value>(&answers()).unwrap();
    ans["Only"] = serde_json::json!(["3 Maybe."]);
    std::fs::write(f.path("answers.json"), ans.to_string()).unwrap();
    let output = f.run(&["--fake-fail-after", "10", "--html"]);
    assert!(!output.status.success());
    for text in [
        f.md(),
        std::fs::read_to_string(f.path("out/report.html")).unwrap(),
    ] {
        for needle in [
            "Stated intent is not sales",
            "everyday consumer products the AI knows well",
            "comparison means more than one score alone",
            "GPT-4o and",
            "Gemini 2.0 Flash",
            "A local Scorer differs from the paper",
            "OpenAI",
            "text-embedding-3-small",
            "not a replacement for research with real people",
            "arXiv 2510.08338",
            "PyMC Labs",
            "github.com/pymc-labs/semantic-similarity-rating",
            "Apache-2.0",
            "Model playing the customers: sonnet",
            "Scorer: fake",
            "10 of 40 planned",
            "Seed: 3",
            "Short sample",
        ] {
            assert!(text.contains(needle), "missing `{needle}`: {text}");
        }
    }
    let md = f.md();
    let date = md.lines().find_map(|l| l.strip_prefix("- Date: ")).unwrap();
    let parts: Vec<&str> = date.split('-').collect();
    assert_eq!(parts.len(), 3, "{date}");
    assert!(
        parts[0].len() == 4 && parts[0].parse::<u32>().unwrap() >= 2026,
        "{date}"
    );
    assert!(md.contains("nothing to compare with"), "{md}");
}

#[test]
fn html_is_one_offline_file_with_svg_charts_and_escaped_text() {
    let f = Fixture::new(&full_run_file());
    ok(&f.run(&["--html"]));
    let html = std::fs::read_to_string(f.path("out/report.html")).unwrap();
    assert!(html.contains("<svg"), "no charts");
    assert!(
        html.matches("<svg").count() >= 4,
        "ranking, spread, two breakdowns"
    );
    for forbidden in [
        "http://", "https://", "//cdn", "<script", "<link", "src=", "href=", "@import", "url(",
        "<img", "<iframe",
    ] {
        assert!(!html.contains(forbidden), "`{forbidden}` in HTML");
    }
    // Answers come from an AI: their markup must not run.
    assert!(
        html.contains("&lt;script&gt;alert('x')&lt;/script&gt; &amp; more"),
        "escaped quote"
    );
    assert!(html.contains("Mint toothpaste") && html.contains("Charcoal toothpaste"));
    assert!(html.contains("Left out on purpose"));
    // Without --html no HTML is made, and an existing one is kept up to date.
    let g = Fixture::new(&full_run_file());
    ok(&g.run(&[]));
    assert!(!g.path("out/report.html").exists());
    ok(&g.report_cmd(&["--html"]));
    assert!(g.path("out/report.html").exists());
    std::fs::write(g.path("out/report.html"), "stale").unwrap();
    ok(&g.report_cmd(&[]));
    assert!(
        std::fs::read_to_string(g.path("out/report.html"))
            .unwrap()
            .contains("<svg")
    );
}

#[test]
fn re_reporting_and_re_scoring_a_saved_run_ask_nobody() {
    let f = Fixture::new(&full_run_file());
    ok(&f.run(&[]));
    let asked = f.calls();
    assert_eq!(asked, 360);
    let before = f.md();
    let data_before = std::fs::read_to_string(f.path("out/answers.jsonl")).unwrap();

    // Re-report: same report, same data, nobody asked.
    ok(&f.report_cmd(&["--html"]));
    assert_eq!(f.calls(), asked);
    assert_eq!(f.md(), before);
    assert_eq!(
        std::fs::read_to_string(f.path("out/answers.jsonl")).unwrap(),
        data_before
    );

    // Change a saved answer and a Concept's data: the report ignores it until
    // asked to re-score, then follows it.
    let edited = data_before.replace(
        "\"answer\":\"5 I love real mint",
        "\"answer\":\"1 I hate real mint",
    );
    assert_ne!(edited, data_before);
    std::fs::write(f.path("out/answers.jsonl"), &edited).unwrap();
    ok(&f.report_cmd(&[]));
    assert!(
        f.md().contains("| Mint toothpaste | 120 | 3.33 |"),
        "re-report alone keeps the saved scores"
    );
    ok(&f.report_cmd(&["--rescore"]));
    assert_eq!(f.calls(), asked, "re-score asks nobody either");
    // One answer in six now says 1 instead of 5: (1+4+3+2+1+5)/6.
    assert!(
        f.md().contains("| Mint toothpaste | 120 | 2.67 |"),
        "{}",
        f.md()
    );
    let rows = read_data(&f);
    let edited_row = rows
        .iter()
        .find(|r| {
            r["answer"]
                .as_str()
                .unwrap()
                .starts_with("1 I hate real mint")
        })
        .unwrap();
    assert_eq!(edited_row["expected_rating"].as_f64().unwrap(), 1.0);
    assert!(f.md().contains("Scorer: fake"));
}

#[test]
fn re_reporting_something_that_is_not_a_run_folder_fails_clearly() {
    let f = Fixture::new(&full_run_file());
    std::fs::create_dir(f.path("out")).unwrap();
    let output = f.report_cmd(&[]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not the output folder of a run"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
