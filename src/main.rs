use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use synthetic_customers::anchors::placeholder_anchor_sets;
use synthetic_customers::embedder::{Embedder, FakeEmbedder};
use synthetic_customers::respondent::{FakeRespondent, Respondent};
use synthetic_customers::run::{self, RunSetup};
use synthetic_customers::run_file::{RunFile, ScorerChoice};
use synthetic_customers::ssr::DEFAULT_EPSILON;

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
        /// Testing only: answer with canned answers from this JSON file
        /// instead of real Synthetic customers.
        #[arg(long, hide = true)]
        fake_answers: Option<PathBuf>,
    },
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
            fake_answers,
        } => run_command(run_file, out, fake_answers),
    }
}

fn run_command(path: PathBuf, out: Option<PathBuf>, fake_answers: Option<PathBuf>) -> Result<()> {
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read run file {}", path.display()))?;
    let run_file =
        RunFile::parse(&text).with_context(|| format!("invalid run file {}", path.display()))?;
    for w in &run_file.warnings {
        eprintln!("warning: {w}");
    }

    let respondent: Box<dyn Respondent> = match fake_answers {
        Some(answers) => Box::new(FakeRespondent::load(&answers, &run_file.concepts)?),
        None => bail!(
            "real Synthetic customers (claude -p / codex exec) are not available in this version yet"
        ),
    };

    let anchor_sets = placeholder_anchor_sets();
    let embedder: Box<dyn Embedder> = match run_file.scorer {
        ScorerChoice::Fake => Box::new(FakeEmbedder::new(&anchor_sets)),
        other => bail!(
            "scorer `{}` is not available in this version yet",
            other.as_str()
        ),
    };

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
        },
        &out_dir,
    )?;
    println!("Report: {}", output.report.display());
    println!("Data:   {}", output.data.display());
    Ok(())
}
