//! The run file: what the user confirmed, in TOML.
//!
//! ```toml
//! customers_per_concept = 200   # default 200, warning outside 150–400
//! model = "haiku"               # haiku (default), sonnet, opus
//! concurrency = 8               # default 8
//! scorer = "local"              # local (default), openai, voyage, fake
//! seed = 42                     # optional, for a repeatable Customer mix sample
//!
//! [[concepts]]
//! name = "Mint toothpaste"
//! text = "A toothpaste that ..."
//! image = "mint.png"            # optional
//!
//! [customer_mix.age]            # one table per attribute: value = share
//! "18-34" = 0.4
//! "35-54" = 0.35
//! "55+" = 0.25
//! ```

use crate::customer_mix::CustomerMix;
use anyhow::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_CUSTOMERS_PER_CONCEPT: usize = 200;
pub const RECOMMENDED_CUSTOMERS: std::ops::RangeInclusive<usize> = 150..=400;
pub const DEFAULT_CONCURRENCY: usize = 8;
/// Hard limits, so a typo cannot exhaust memory or spawn thousands of calls.
pub const MAX_CUSTOMERS_PER_CONCEPT: usize = 10_000;
pub const MAX_CONCURRENCY: usize = 64;
/// How far each attribute's shares may be from summing to 1.
pub const SHARE_TOLERANCE: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    Haiku,
    Sonnet,
    Opus,
}

impl Model {
    pub fn as_str(self) -> &'static str {
        match self {
            Model::Haiku => "haiku",
            Model::Sonnet => "sonnet",
            Model::Opus => "opus",
        }
    }
}

/// Which embedding model scores the answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScorerChoice {
    #[default]
    Local,
    Openai,
    Voyage,
    /// Hand-computable fake for tests; see `FakeEmbedder`.
    Fake,
}

impl ScorerChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            ScorerChoice::Local => "local",
            ScorerChoice::Openai => "openai",
            ScorerChoice::Voyage => "voyage",
            ScorerChoice::Fake => "fake",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub name: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRunFile {
    #[serde(default)]
    concepts: Vec<Concept>,
    #[serde(default)]
    customer_mix: IndexMap<String, IndexMap<String, f64>>,
    customers_per_concept: Option<i64>,
    #[serde(default)]
    model: Model,
    concurrency: Option<i64>,
    #[serde(default)]
    scorer: ScorerChoice,
    seed: Option<u64>,
}

/// A validated run file.
#[derive(Debug, Clone)]
pub struct RunFile {
    pub concepts: Vec<Concept>,
    pub customer_mix: CustomerMix,
    pub customers_per_concept: usize,
    pub model: Model,
    pub concurrency: usize,
    pub scorer: ScorerChoice,
    pub seed: Option<u64>,
    /// Non-fatal problems to show the user, e.g. a sample outside 150–400.
    pub warnings: Vec<String>,
}

impl RunFile {
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawRunFile = toml::from_str(text)?;
        let mut warnings = Vec::new();

        if raw.concepts.is_empty() {
            bail!("no Concepts: add at least one [[concepts]] table with a `name` and a `text`");
        }
        let mut seen = std::collections::HashSet::new();
        for (i, c) in raw.concepts.iter().enumerate() {
            if c.name.trim().is_empty() {
                bail!("Concept {} has an empty `name`", i + 1);
            }
            if c.text.trim().is_empty() {
                bail!("Concept `{}` has an empty `text`", c.name);
            }
            if !seen.insert(c.name.trim()) {
                bail!("two Concepts are named `{}`: names must be unique", c.name);
            }
        }

        let customers_per_concept = match raw.customers_per_concept {
            None => DEFAULT_CUSTOMERS_PER_CONCEPT,
            Some(n) if n < 1 => bail!("`customers_per_concept` must be at least 1, got {n}"),
            Some(n) if n as u64 > MAX_CUSTOMERS_PER_CONCEPT as u64 => bail!(
                "`customers_per_concept` must be at most {MAX_CUSTOMERS_PER_CONCEPT}, got {n}"
            ),
            Some(n) => n as usize,
        };
        if !RECOMMENDED_CUSTOMERS.contains(&customers_per_concept) {
            warnings.push(format!(
                "customers_per_concept = {customers_per_concept} is outside the recommended {}–{} \
                 (the sample sizes of the paper's real surveys)",
                RECOMMENDED_CUSTOMERS.start(),
                RECOMMENDED_CUSTOMERS.end()
            ));
        }

        let concurrency = match raw.concurrency {
            None => DEFAULT_CONCURRENCY,
            Some(n) if n < 1 => bail!("`concurrency` must be at least 1, got {n}"),
            Some(n) if n as u64 > MAX_CONCURRENCY as u64 => {
                bail!("`concurrency` must be at most {MAX_CONCURRENCY}, got {n}")
            }
            Some(n) => n as usize,
        };

        let customer_mix = CustomerMix::new(raw.customer_mix)?;

        Ok(Self {
            concepts: raw.concepts,
            customer_mix,
            customers_per_concept,
            model: raw.model,
            concurrency,
            scorer: raw.scorer,
            seed: raw.seed,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let run = RunFile::parse(
            "[[concepts]]\nname = \"A\"\ntext = \"An idea\"\n[customer_mix.age]\n\"18+\" = 1.0\n",
        )
        .unwrap();
        assert_eq!(run.customers_per_concept, 200);
        assert_eq!(run.concurrency, 8);
        assert_eq!(run.model, Model::Haiku);
        assert_eq!(run.scorer, ScorerChoice::Local);
        assert!(run.warnings.is_empty());
    }
}
