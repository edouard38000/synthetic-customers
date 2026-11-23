//! The Respondent interface: asks one Synthetic customer about one Concept.
//!
//! Real Respondents run one subprocess per Synthetic customer: `claude -p`
//! (Claude Code) or `codex exec` (Codex). They inherit the user's own sign-in
//! from the environment; this tool never reads, passes or logs a key or token.

use crate::customer_mix::SyntheticCustomer;
use crate::run_file::{Concept, Model};
use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Plays one Synthetic customer and returns their answer in their own words.
pub trait Respondent: Send + Sync {
    /// `index` is the customer's position within the Concept (0-based).
    fn answer(
        &self,
        concept: &Concept,
        index: usize,
        customer: &SyntheticCustomer,
    ) -> Result<String>;

    /// Rough seconds one answer takes, for the time estimate shown before a
    /// run starts.
    fn seconds_per_answer(&self) -> f64 {
        10.0
    }
}

/// The question every Synthetic customer is asked, following the paper's
/// purchase-intent prompt (arXiv 2510.08338, App. A.4): impersonate a
/// consumer with these attributes in a product survey, here is the Concept,
/// "How likely are you to purchase the product?", answer briefly in your own
/// words.
pub fn prompt(concept: &Concept, customer: &SyntheticCustomer) -> String {
    let mut p = String::from(
        "You are taking part in a consumer survey about a new product. \
         Impersonate a consumer with the following attributes:\n",
    );
    for (attribute, value) in &customer.attributes {
        p.push_str(&format!("- {attribute}: {value}\n"));
    }
    p.push_str(
        "\nStay in this role for the whole survey and answer as this person would, \
         from their own life, needs and budget.\n\nHere is the product concept:\n\n",
    );
    p.push_str(concept.text.trim());
    p.push('\n');
    if let Some(image) = &concept.image {
        p.push_str(&format!(
            "\nThe concept also has an image, at {}. Look at it before you answer.\n",
            image.display()
        ));
    }
    p.push_str(
        "\nHow likely are you to purchase the product? Answer briefly, in one or two \
         sentences of your own words, as this consumer would. Do not answer with a number \
         or a rating scale.\n",
    );
    p
}

/// Which command-line runner plays the Synthetic customers.
#[derive(Debug, Clone)]
pub enum CliRespondent {
    /// `claude -p --model <model>` (Claude Code).
    Claude(Model),
    /// `codex exec` with the user's default Codex model.
    Codex,
}

/// How long one Synthetic customer may take before the call is stopped.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

impl CliRespondent {
    fn command(&self, concept: &Concept, last_message: &Path) -> Command {
        match self {
            CliRespondent::Claude(model) => {
                let mut cmd = Command::new("claude");
                cmd.args(["-p", "--model", model.as_str(), "--output-format", "text"])
                    .args(["--no-session-persistence", "--disable-slash-commands"])
                    .args(["--strict-mcp-config", "--setting-sources", ""]);
                match concept.image.as_ref().and_then(|i| i.parent()) {
                    // The only tool a customer gets is reading the Concept's image.
                    Some(dir) => {
                        cmd.args(["--tools", "Read", "--allowed-tools", "Read", "--add-dir"])
                            .arg(dir);
                    }
                    None => {
                        cmd.args(["--tools", ""]);
                    }
                }
                cmd
            }
            CliRespondent::Codex => {
                let mut cmd = Command::new("codex");
                cmd.args(["exec", "--skip-git-repo-check", "--sandbox", "read-only"])
                    .args(["--color", "never", "--output-last-message"])
                    .arg(last_message);
                if let Some(image) = &concept.image {
                    cmd.arg("--image").arg(image);
                }
                // The prompt comes on stdin.
                cmd.arg("-");
                cmd
            }
        }
    }

    fn program(&self) -> &'static str {
        match self {
            CliRespondent::Claude(_) => "claude -p",
            CliRespondent::Codex => "codex exec",
        }
    }
}

impl Respondent for CliRespondent {
    fn answer(
        &self,
        concept: &Concept,
        index: usize,
        customer: &SyntheticCustomer,
    ) -> Result<String> {
        // Run outside the user's project so its instructions and settings do
        // not leak into the customer's answer.
        let work = std::env::temp_dir();
        let last_message = work.join(format!(
            "synthetic-customers-{}-{}-{index}.txt",
            std::process::id(),
            sanitize(&concept.name)
        ));
        let mut cmd = self.command(concept, &last_message);
        cmd.current_dir(&work);
        let stdout = run_with_timeout(cmd, &prompt(concept, customer), ANSWER_TIMEOUT)
            .with_context(|| format!("{} failed", self.program()));
        let answer = match self {
            CliRespondent::Claude(_) => stdout,
            CliRespondent::Codex => stdout.and_then(|_| {
                std::fs::read_to_string(&last_message).context("codex exec wrote no answer")
            }),
        };
        let _ = std::fs::remove_file(&last_message);
        let answer = answer?.trim().to_string();
        if answer.is_empty() {
            bail!("{} returned an empty answer", self.program());
        }
        Ok(answer)
    }

    fn seconds_per_answer(&self) -> f64 {
        match self {
            CliRespondent::Claude(Model::Haiku) => 8.0,
            CliRespondent::Claude(Model::Sonnet) => 12.0,
            CliRespondent::Claude(Model::Opus) => 20.0,
            CliRespondent::Codex => 20.0,
        }
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(40)
        .collect()
}

/// Run `cmd` with `input` on stdin; return stdout, or fail with the end of
/// stderr when it exits non-zero or takes longer than `timeout`.
fn run_with_timeout(mut cmd: Command, input: &str, timeout: Duration) -> Result<String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot start it: is it installed and on the PATH?")?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input = input.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let out_reader = std::thread::spawn(move || {
        let mut s = String::new();
        stdout.read_to_string(&mut s).map(|_| s)
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("no answer after {} seconds", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = writer.join();
    let out = out_reader
        .join()
        .map_err(|_| anyhow::anyhow!("output reader panicked"))?
        .context("cannot read its output")?;
    let err = err_reader.join().unwrap_or_default();
    if !status.success() {
        let tail: String = {
            let text = if err.trim().is_empty() { &out } else { &err };
            let chars: Vec<char> = text.trim().chars().collect();
            chars[chars.len().saturating_sub(500)..].iter().collect()
        };
        bail!("exited with {status}: {tail}");
    }
    Ok(out)
}

/// Canned answers for tests, from a JSON file mapping each Concept's name to
/// a list of answers: `{"Mint toothpaste": ["4", "I'd buy it, 5"]}`.
/// Customer `i` of a Concept gets answer `i` modulo the list length.
///
/// For resume tests it can log every call to a file (one `concept<TAB>index`
/// line per call) and fail every call after the first `fail_after`, the way
/// a usage limit stops a real run.
pub struct FakeRespondent {
    answers: IndexMap<String, Vec<String>>,
    calls_log: Option<Mutex<File>>,
    fail_after: Option<usize>,
    calls: AtomicUsize,
}

impl FakeRespondent {
    pub fn load(path: &Path, concepts: &[Concept]) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read fake answers {}", path.display()))?;
        let answers: IndexMap<String, Vec<String>> = serde_json::from_str(&text)
            .with_context(|| format!("invalid fake answers {}", path.display()))?;
        for c in concepts {
            match answers.get(&c.name) {
                None => bail!("fake answers have no entry for Concept `{}`", c.name),
                Some(list) if list.is_empty() => {
                    bail!("fake answers for Concept `{}` are empty", c.name)
                }
                Some(_) => {}
            }
        }
        Ok(Self {
            answers,
            calls_log: None,
            fail_after: None,
            calls: AtomicUsize::new(0),
        })
    }

    /// Append one line per call to `path`.
    pub fn with_calls_log(mut self, path: &Path) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("cannot open fake calls log {}", path.display()))?;
        self.calls_log = Some(Mutex::new(file));
        Ok(self)
    }

    /// Answer the first `n` calls, then fail like a usage limit.
    pub fn failing_after(mut self, n: usize) -> Self {
        self.fail_after = Some(n);
        self
    }
}

impl Respondent for FakeRespondent {
    fn answer(
        &self,
        concept: &Concept,
        index: usize,
        _customer: &SyntheticCustomer,
    ) -> Result<String> {
        if let Some(log) = &self.calls_log {
            let mut file = log.lock().expect("calls log lock");
            writeln!(file, "{}\t{index}", concept.name)?;
        }
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_after.is_some_and(|n| call >= n) {
            bail!("fake usage limit reached");
        }
        let list = &self.answers[&concept.name];
        Ok(list[index % list.len()].clone())
    }

    fn seconds_per_answer(&self) -> f64 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn prompt_names_every_attribute_the_concept_and_the_question() {
        let concept = Concept {
            name: "Mint".into(),
            text: "A mint toothpaste.".into(),
            image: Some(PathBuf::from("/tmp/mint.png")),
        };
        let customer = SyntheticCustomer {
            attributes: [("age", "35-54"), ("income", "middle")]
                .into_iter()
                .map(|(a, v)| (a.to_string(), v.to_string()))
                .collect(),
        };
        let p = prompt(&concept, &customer);
        assert!(p.contains("Impersonate a consumer"));
        assert!(p.contains("- age: 35-54\n- income: middle\n"));
        assert!(p.contains("A mint toothpaste."));
        assert!(p.contains("/tmp/mint.png"));
        assert!(p.contains("How likely are you to purchase the product?"));
    }
}
