use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use synthetic_customers::anchors::{AnchorSet, placeholder_anchor_sets};
use synthetic_customers::embedder::{Embedder, FakeEmbedder};
use synthetic_customers::respondent::{CliRespondent, FakeRespondent, Respondent};
use synthetic_customers::run::{self, RunSetup};
use synthetic_customers::run_file::{RespondentChoice, RunFile, ScorerChoice};
use synthetic_customers::ssr::{DEFAULT_EPSILON, SsrScorer};

/// Test a product idea on AI-played Synthetic customers.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a confirmed run file end to end: ask every Synthetic customer,
    /// score each answer and write the report.
    Run {
        /// The run file (TOML).
        run_file: PathBuf,
        /// Folder for report.md and answers.jsonl
        /// [default: <run file name>-results next to the run file].
        #[arg(long)]
        out: Option<PathBuf>,
        /// Continue the stopped run in the output folder, asking only the
        /// Synthetic customers who have not answered yet.
        #[arg(long)]
        resume: bool,
        /// Also write report.html: one self-contained page with charts,
        /// no network needed.
        #[arg(long)]
        html: bool,
        /// Testing only: answer with canned answers from this JSON file
        /// instead of real Synthetic customers.
        #[arg(long, hide = true)]
        fake_answers: Option<PathBuf>,
        /// Testing only: log each fake customer call to this file.
        #[arg(long, hide = true, requires = "fake_answers")]
        fake_calls_log: Option<PathBuf>,
        /// Testing only: fake customers fail after this many calls, like a
        /// usage limit.
        #[arg(long, hide = true, requires = "fake_answers")]
        fake_fail_after: Option<usize>,
    },
    /// Rebuild the report from a saved output folder (answers.jsonl and
    /// run.toml) without asking any Synthetic customer again.
    Report {
        /// The output folder of an earlier run.
        folder: PathBuf,
        /// Score the saved answers again first (after a new Scorer or new
        /// Anchor sets) and save the new scores.
        #[arg(long)]
        rescore: bool,
        /// The Scorer to re-score with [default: the run file's].
        #[arg(long, requires = "rescore", value_parser = parse_scorer)]
        scorer: Option<ScorerChoice>,
        /// Also write report.html.
        #[arg(long)]
        html: bool,
    },
}

fn parse_scorer(name: &str) -> Result<ScorerChoice, String> {
    serde_json::from_value(serde_json::Value::String(name.to_string()))
        .map_err(|_| format!("unknown Scorer `{name}`: use local, openai, voyage or fake"))
}

/// Testing-only options for the fake Respondent.
struct FakeOptions {
    answers: Option<PathBuf>,
    calls_log: Option<PathBuf>,
    fail_after: Option<usize>,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<()> {
    match Cli::parse().command {
        Command::Run {
            run_file,
            out,
            resume,
            html,
            fake_answers,
            fake_calls_log,
            fake_fail_after,
        } => run_command(
            run_file,
            out,
            resume,
            html,
            FakeOptions {
                answers: fake_answers,
                calls_log: fake_calls_log,
                fail_after: fake_fail_after,
            },
        ),
        Command::Report {
            folder,
            rescore,
            scorer,
            html,
        } => report_command(folder, rescore, scorer, html),
    }
}

fn run_command(
    path: PathBuf,
    out: Option<PathBuf>,
    resume: bool,
    html: bool,
    fake: FakeOptions,
) -> Result<()> {
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read run file {}", path.display()))?;
    let mut run_file =
        RunFile::parse(&text).with_context(|| format!("invalid run file {}", path.display()))?;
    // A Concept's image path is relative to the run file.
    let base = path.parent().unwrap_or(std::path::Path::new("."));
    for concept in &mut run_file.concepts {
        if let Some(image) = &concept.image {
            let image = std::path::absolute(base.join(image))
                .with_context(|| format!("bad image path for Concept `{}`", concept.name))?;
            if !image.is_file() {
                bail!(
                    "the image of Concept `{}` is not a file: {}",
                    concept.name,
                    image.display()
                );
            }
            concept.image = Some(image);
        }
    }
    for w in &run_file.warnings {
        eprintln!("warning: {w}");
    }

    let respondent: Box<dyn Respondent> = match fake.answers {
        Some(answers) => {
            let mut r = FakeRespondent::load(&answers, &run_file.concepts)?;
            if let Some(log) = &fake.calls_log {
                r = r.with_calls_log(log)?;
            }
            if let Some(n) = fake.fail_after {
                r = r.failing_after(n);
            }
            Box::new(r)
        }
        None => {
            let r = match run_file.respondent {
                RespondentChoice::Claude => CliRespondent::Claude(run_file.model),
                RespondentChoice::Codex => CliRespondent::Codex,
            };
            r.check_installed()?;
            Box::new(r)
        }
    };

    let (embedder, anchor_sets) = build_scorer(run_file.scorer)?;

    let seed = run_file.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    });

    let out_dir = out.unwrap_or_else(|| {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "run".to_string());
        path.with_file_name(format!("{stem}-results"))
    });

    let output = run::run(
        &run_file,
        &text,
        RunSetup {
            respondent: respondent.as_ref(),
            embedder: embedder.as_ref(),
            anchor_sets,
            epsilon: DEFAULT_EPSILON,
            seed,
            resume,
            html,
        },
        &out_dir,
    )?;
    print_output(&output);
    Ok(())
}

fn print_output(output: &run::RunOutput) {
    println!("Report: {}", output.report.display());
    if let Some(html) = &output.html {
        println!("HTML:   {}", html.display());
    }
    println!("Data:   {}", output.data.display());
}

/// The Scorer's embedder and the Anchor sets it scores against.
fn build_scorer(choice: ScorerChoice) -> Result<(Box<dyn Embedder>, Vec<AnchorSet>)> {
    let anchor_sets = placeholder_anchor_sets();
    match choice {
        ScorerChoice::Fake => Ok((Box::new(FakeEmbedder::new(&anchor_sets)), anchor_sets)),
        other => bail!(
            "scorer `{}` is not available in this version yet",
            other.as_str()
        ),
    }
}

fn report_command(
    folder: PathBuf,
    rescore: bool,
    scorer: Option<ScorerChoice>,
    html: bool,
) -> Result<()> {
    let output = if rescore {
        let text = std::fs::read_to_string(folder.join(run::RUN_FILE_COPY))
            .with_context(|| format!("cannot read run.toml in {}", folder.display()))?;
        let run_file = RunFile::parse(&text).context("invalid run.toml")?;
        let choice = scorer.unwrap_or(run_file.scorer);
        let (embedder, anchor_sets) = build_scorer(choice)?;
        let ssr = SsrScorer::new(embedder.as_ref(), &anchor_sets, DEFAULT_EPSILON)?;
        run::rebuild_report(&folder, Some((&ssr, embedder.name())), html)?
    } else {
        run::rebuild_report(&folder, None, html)?
    };
    print_output(&output);
    Ok(())
}
