//! The Respondent interface: asks one Synthetic customer about one Concept.

use crate::customer_mix::SyntheticCustomer;
use crate::run_file::Concept;
use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use std::path::Path;

/// Plays one Synthetic customer and returns their answer in their own words.
/// The real Respondent runs one `claude -p` or `codex exec` call per customer.
pub trait Respondent: Send + Sync {
    /// `index` is the customer's position within the Concept (0-based).
    fn answer(
        &self,
        concept: &Concept,
        index: usize,
        customer: &SyntheticCustomer,
    ) -> Result<String>;
}

/// Canned answers for tests, from a JSON file mapping each Concept's name to
/// a list of answers: `{"Mint toothpaste": ["4", "I'd buy it, 5"]}`.
/// Customer `i` of a Concept gets answer `i` modulo the list length.
pub struct FakeRespondent {
    answers: IndexMap<String, Vec<String>>,
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
        Ok(Self { answers })
    }
}

impl Respondent for FakeRespondent {
    fn answer(
        &self,
        concept: &Concept,
        index: usize,
        _customer: &SyntheticCustomer,
    ) -> Result<String> {
        let list = &self.answers[&concept.name];
        Ok(list[index % list.len()].clone())
    }
}
