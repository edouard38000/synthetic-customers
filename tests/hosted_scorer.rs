//! The hosted Scorers (OpenAI, Voyage) end to end through the binary, with
//! a local mock of the embeddings API instead of the network.
//!
//! The mock embeds texts like the fake Scorer (anchor r → e_r, an answer →
//! the digits 1–5 it contains), so its scores can be checked by hand. No
//! test here needs a real key or the network.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use synthetic_customers::anchors::anchor_sets;
use synthetic_customers::embedder::{Embedder, FakeEmbedder};
use tempfile::TempDir;

/// A key that must never show up in any output or file.
const KEY: &str = "sk-test-NEVER-WRITE-ME-0123456789abcdef";

const RUN: &str = r#"
[[concepts]]
name = "Mint toothpaste"
text = "A toothpaste with real mint leaves."

[[concepts]]
name = "Charcoal toothpaste"
text = "A black toothpaste with activated charcoal."

[customer_mix.age]
"18-34" = 0.5
"55+" = 0.5
"#;

const ANSWERS: &str = r#"{
  "Mint toothpaste": ["5", "4", "maybe 3 or 4", "no idea"],
  "Charcoal toothpaste": ["1", "2"]
}"#;

/// What the mock answers to request number n (from 0): `None` embeds the
/// texts, `Some((status, body))` fails.
type Failure = Box<dyn Fn(usize) -> Option<(u16, String)> + Send>;

#[derive(Default)]
struct Seen {
    requests: usize,
    auth: Vec<String>,
    models: Vec<String>,
}

struct Mock {
    url: String,
    seen: Arc<Mutex<Seen>>,
    failure: Arc<Mutex<Option<Failure>>>,
}

impl Mock {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/embeddings", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Seen::default()));
        let failure: Arc<Mutex<Option<Failure>>> = Arc::new(Mutex::new(None));
        let (s, f) = (seen.clone(), failure.clone());
        std::thread::spawn(move || {
            let embedder = FakeEmbedder::new(&anchor_sets());
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = serve(stream, &embedder, &s, &f);
            }
        });
        Self { url, seen, failure }
    }

    fn fail_with(&self, failure: impl Fn(usize) -> Option<(u16, String)> + Send + 'static) {
        *self.failure.lock().unwrap() = Some(Box::new(failure));
    }

    fn heal(&self) {
        *self.failure.lock().unwrap() = None;
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().requests
    }
}

/// Answer one HTTP request, then close the connection.
fn serve(
    stream: TcpStream,
    embedder: &FakeEmbedder,
    seen: &Mutex<Seen>,
    failure: &Mutex<Option<Failure>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut length = 0;
    let mut auth = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "authorization" => auth = value.trim().to_string(),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();

    let n = {
        let mut s = seen.lock().unwrap();
        s.requests += 1;
        s.auth.push(auth);
        s.models
            .push(request["model"].as_str().unwrap_or_default().to_string());
        s.requests - 1
    };
    let failed = failure.lock().unwrap().as_ref().and_then(|f| f(n));
    let (status, text) = failed.unwrap_or_else(|| {
        let inputs: Vec<&str> = request["input"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let vectors = embedder.embed(&inputs).unwrap();
        // Out of order on purpose: the Scorer must sort by index.
        let data: Vec<Value> = vectors
            .into_iter()
            .enumerate()
            .rev()
            .map(|(i, v)| serde_json::json!({"object": "embedding", "index": i, "embedding": v}))
            .collect();
        (
            200,
            serde_json::json!({"object": "list", "data": data}).to_string(),
        )
    });
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Retry-After: 0\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new(head: &str) -> Self {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("run.toml"), format!("{head}\n{RUN}")).unwrap();
        std::fs::write(dir.path().join("answers.json"), ANSWERS).unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn command(&self, mock: Option<&Mock>) -> Command {
        let mut c = Command::cargo_bin("synthetic-customers").unwrap();
        c.env_remove("OPENAI_API_KEY")
            .env_remove("VOYAGE_API_KEY")
            .env_remove("SYNTHETIC_CUSTOMERS_SCORER_URL");
        if let Some(m) = mock {
            c.env("SYNTHETIC_CUSTOMERS_SCORER_URL", &m.url);
        }
        c
    }

    /// `run` with the fake Respondent, into `out`.
    fn run(&self, mock: Option<&Mock>, env: &[(&str, &str)], extra: &[&str]) -> Output {
        let mut c = self.command(mock);
        c.arg("run")
            .arg(self.path("run.toml"))
            .arg("--out")
            .arg(self.path("out"))
            .arg("--fake-answers")
            .arg(self.path("answers.json"))
            .arg("--fake-calls-log")
            .arg(self.path("calls.log"))
            .args(extra);
        for (k, v) in env {
            c.env(k, v);
        }
        Output(c.output().unwrap())
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.path("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn data(&self) -> Vec<Value> {
        std::fs::read_to_string(self.path("out/answers.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// The key appears in no file of the fixture folder.
    fn assert_key_nowhere(&self) {
        assert_no_key_in(self.dir.path());
    }
}

fn assert_no_key_in(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            assert_no_key_in(&path);
        } else {
            let bytes = std::fs::read(&path).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(KEY),
                "the key is written in {}",
                path.display()
            );
            assert!(
                !text.contains("NEVER-WRITE-ME"),
                "part of the key is written in {}",
                path.display()
            );
        }
    }
}

struct Output(std::process::Output);

impl Output {
    fn ok(&self) -> bool {
        self.0.status.success()
    }
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.0.stdout).into_owned()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.0.stderr).into_owned()
    }
    fn assert_no_key(&self) {
        for text in [self.stdout(), self.stderr()] {
            assert!(!text.contains(KEY), "the key is printed: {text}");
            assert!(
                !text.contains("NEVER-WRITE-ME"),
                "part of the key is printed: {text}"
            );
        }
    }
}

fn ratings(data: &[Value]) -> Vec<(String, u64, f64)> {
    data.iter()
        .map(|r| {
            (
                r["concept"].as_str().unwrap().to_string(),
                r["customer"].as_u64().unwrap(),
                r["expected_rating"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn hosted_scorers_score_through_the_api_and_never_write_the_key() {
    // The same run with the fake Scorer gives the scores to expect.
    let fake = Fixture::new("scorer = \"fake\"\nseed = 5\ncustomers_per_concept = 6");
    assert!(fake.run(None, &[], &[]).ok());
    let expected = ratings(&fake.data());

    for (scorer, env, model, name) in [
        (
            "openai",
            "OPENAI_API_KEY",
            "text-embedding-3-small",
            "openai (text-embedding-3-small)",
        ),
        ("voyage", "VOYAGE_API_KEY", "voyage-4", "voyage (voyage-4)"),
    ] {
        let mock = Mock::start();
        let f = Fixture::new(&format!(
            "scorer = \"{scorer}\"\nseed = 5\ncustomers_per_concept = 6"
        ));
        let out = f.run(Some(&mock), &[(env, KEY)], &["--html"]);
        assert!(out.ok(), "{scorer}: {}", out.stderr());
        out.assert_no_key();
        f.assert_key_nowhere();

        assert_eq!(ratings(&f.data()), expected, "{scorer}");
        let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
        assert!(report.contains(name), "{scorer}: {report}");

        // One request for all 30 anchor statements, one for the 12 answers.
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.requests, 2, "{scorer}");
        assert!(seen.auth.iter().all(|a| a == &format!("Bearer {KEY}")));
        assert!(seen.models.iter().all(|m| m == model), "{:?}", seen.models);
    }
}

#[test]
fn a_missing_key_fails_before_any_customer_is_asked() {
    for (scorer, env) in [("openai", "OPENAI_API_KEY"), ("voyage", "VOYAGE_API_KEY")] {
        for value in [None, Some("  ")] {
            let mock = Mock::start();
            let f = Fixture::new(&format!("scorer = \"{scorer}\""));
            let env_pairs: Vec<(&str, &str)> = value.map(|v| (env, v)).into_iter().collect();
            let out = f.run(Some(&mock), &env_pairs, &[]);
            assert!(!out.ok());
            let err = out.stderr();
            assert!(err.contains(env), "{err}");
            assert!(err.contains("not set"), "{err}");
            if scorer == "openai" {
                assert!(
                    err.contains(
                        "A ChatGPT subscription (Plus, Pro, Team) is not an OpenAI API key"
                    ),
                    "{err}"
                );
            }
            assert!(
                f.calls().is_empty(),
                "no Synthetic customer should be asked"
            );
            assert_eq!(mock.requests(), 0, "no request without a key");
            assert!(!f.path("out/answers.jsonl").exists());
        }
    }
}

#[test]
fn a_rejected_key_fails_before_any_customer_is_asked_without_echoing_it() {
    let mock = Mock::start();
    mock.fail_with(|_| {
        Some((
            401,
            format!(
                r#"{{"error":{{"message":"Incorrect API key provided: {KEY}. You can find your API key at https://platform.openai.com/account/api-keys."}}}}"#
            ),
        ))
    });
    let f = Fixture::new("scorer = \"openai\"");
    let out = f.run(Some(&mock), &[("OPENAI_API_KEY", KEY)], &[]);
    assert!(!out.ok());
    out.assert_no_key();
    let err = out.stderr();
    assert!(
        err.contains("rejected the key in OPENAI_API_KEY (HTTP 401)"),
        "{err}"
    );
    assert!(
        f.calls().is_empty(),
        "no Synthetic customer should be asked"
    );
    f.assert_key_nowhere();
}

#[test]
fn a_scorer_failure_mid_run_keeps_every_answer_and_resume_scores_them_without_asking_again() {
    let mock = Mock::start();
    // Anchors (request 0) and the first batch of 32 answers (request 1)
    // score; then the key "stops working".
    mock.fail_with(|n| {
        (n >= 2).then(|| {
            (
                401,
                format!(r#"{{"error":{{"message":"Incorrect API key provided: {KEY}."}}}}"#),
            )
        })
    });
    let f =
        Fixture::new("scorer = \"openai\"\nseed = 9\ncustomers_per_concept = 40\nconcurrency = 4");
    let out = f.run(Some(&mock), &[("OPENAI_API_KEY", KEY)], &[]);
    assert!(!out.ok());
    out.assert_no_key();
    let err = out.stderr();
    assert!(err.contains("rejected the key"), "{err}");
    assert!(err.contains("saved but not scored yet"), "{err}");
    assert!(err.contains("--resume"), "{err}");

    // Every customer asked has a saved answer, one line each; 32 are scored.
    let first_calls = f.calls();
    let data = f.data();
    assert_eq!(data.len(), first_calls.len());
    // The fake customers answer instantly, so all 80 may be in before the
    // failing batch; with real ones, the run stops asking at the failure.
    assert!(
        data.len() > 32 && data.len() <= 80,
        "{} answers",
        data.len()
    );
    let scored = data
        .iter()
        .filter(|r| r.get("expected_rating").is_some())
        .count();
    assert_eq!(scored, 32);
    assert!(data.iter().all(|r| r["answer"].as_str().is_some()));
    let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
    assert!(report.to_lowercase().contains("short sample"), "{report}");
    f.assert_key_nowhere();

    // The key works again: the resume scores the saved answers and asks
    // only the customers never asked.
    mock.heal();
    let out = f.run(Some(&mock), &[("OPENAI_API_KEY", KEY)], &["--resume"]);
    assert!(out.ok(), "{}", out.stderr());
    out.assert_no_key();
    assert!(
        out.stderr()
            .contains(&format!("Scoring {} saved answers", data.len() - 32)),
        "{}",
        out.stderr()
    );
    let calls = f.calls();
    assert_eq!(calls.len(), 80, "each customer asked exactly once in all");
    assert_eq!(calls.iter().collect::<HashSet<_>>().len(), 80);
    let data = f.data();
    assert_eq!(data.len(), 80);
    assert!(data.iter().all(|r| r.get("expected_rating").is_some()));
    f.assert_key_nowhere();
}

#[test]
fn a_rate_limit_is_retried() {
    let mock = Mock::start();
    mock.fail_with(|n| {
        (n == 1).then(|| {
            (
                429,
                r#"{"error":{"message":"Rate limit reached"}}"#.to_string(),
            )
        })
    });
    let f = Fixture::new("scorer = \"voyage\"\ncustomers_per_concept = 3");
    let out = f.run(Some(&mock), &[("VOYAGE_API_KEY", KEY)], &[]);
    assert!(out.ok(), "{}", out.stderr());
    assert_eq!(mock.requests(), 3, "anchors, a refused batch, its retry");
    assert!(f.data().iter().all(|r| r.get("expected_rating").is_some()));
}

#[test]
fn the_test_url_only_accepts_a_local_address() {
    let f = Fixture::new("scorer = \"openai\"");
    let out = Output(
        f.command(None)
            .arg("run")
            .arg(f.path("run.toml"))
            .arg("--out")
            .arg(f.path("out"))
            .arg("--fake-answers")
            .arg(f.path("answers.json"))
            .arg("--fake-calls-log")
            .arg(f.path("calls.log"))
            .env("OPENAI_API_KEY", KEY)
            .env(
                "SYNTHETIC_CUSTOMERS_SCORER_URL",
                "https://example.com/v1/embeddings",
            )
            .output()
            .unwrap(),
    );
    assert!(!out.ok());
    assert!(
        out.stderr().contains("only accepts a local http:// URL"),
        "{}",
        out.stderr()
    );
    assert!(f.calls().is_empty());
}

#[test]
fn report_rescore_with_a_hosted_scorer() {
    let f = Fixture::new("scorer = \"fake\"\nseed = 2\ncustomers_per_concept = 4");
    assert!(f.run(None, &[], &[]).ok());
    let before = ratings(&f.data());

    let mock = Mock::start();
    let out = Output(
        f.command(Some(&mock))
            .arg("report")
            .arg(f.path("out"))
            .arg("--rescore")
            .arg("--scorer")
            .arg("voyage")
            .env("VOYAGE_API_KEY", KEY)
            .output()
            .unwrap(),
    );
    assert!(out.ok(), "{}", out.stderr());
    out.assert_no_key();
    assert_eq!(
        ratings(&f.data()),
        before,
        "the mock embeds like the fake Scorer"
    );
    let state = std::fs::read_to_string(f.path("out/run-state.json")).unwrap();
    assert!(state.contains("voyage (voyage-4)"), "{state}");
    let report = std::fs::read_to_string(f.path("out/report.md")).unwrap();
    assert!(report.contains("voyage (voyage-4)"), "{report}");
    f.assert_key_nowhere();
}
