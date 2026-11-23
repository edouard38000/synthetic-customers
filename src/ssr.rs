//! Semantic similarity rating (SSR), arXiv 2510.08338 eq. 8.
//!
//! For each Anchor set, the probability of Purchase intent rating `r` is
//!
//! ```text
//! p(r) = (γ_r − γ_min + ε·[r = argmin]) / (Σ_r γ_r − 5·γ_min + ε)
//! ```
//!
//! with γ the similarity between the answer and anchor statement `r`. The
//! distributions of all Anchor sets are averaged, then normalised. The
//! temperature is fixed at T = 1, so no rescaling happens.
//!
//! Similarity follows PyMC Labs' reference code (github.com/pymc-labs/
//! semantic-similarity-rating, `compute.py`): γ = (1 + cos) / 2. Halving the
//! cosine cancels out when ε = 0; it only changes the result when ε > 0.

use crate::anchors::{AnchorSet, RATINGS};
use crate::embedder::Embedder;
use anyhow::{Context, Result, bail};

/// A probability for each Purchase intent rating, index 0 = rating 1.
pub type Distribution = [f64; RATINGS];

/// Default ε, as in PyMC Labs' reference code.
pub const DEFAULT_EPSILON: f64 = 0.0;

/// Cosine similarity of two vectors. Zero when either vector is all zeros.
pub fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

/// γ = (1 + cos) / 2, a similarity in [0, 1].
pub fn gamma(a: &[f64], b: &[f64]) -> f64 {
    (1.0 + cosine(a, b)) / 2.0
}

/// Eq. 8 for one Anchor set: five similarities γ_r to one distribution.
///
/// ε is added at the first index of the minimum similarity. When every
/// similarity is equal and ε = 0 the formula is 0/0; the answer then says
/// nothing about the rating, so the distribution is uniform.
pub fn similarities_to_distribution(similarities: &Distribution, epsilon: f64) -> Distribution {
    let (min_index, min) =
        similarities
            .iter()
            .copied()
            .enumerate()
            .fold(
                (0, f64::INFINITY),
                |best, (i, s)| {
                    if s < best.1 { (i, s) } else { best }
                },
            );
    let mut numerator = similarities.map(|s| s - min);
    numerator[min_index] += epsilon;
    let denominator: f64 = numerator.iter().sum();
    if denominator <= 0.0 || !denominator.is_finite() {
        return [1.0 / RATINGS as f64; RATINGS];
    }
    numerator.map(|n| n / denominator)
}

/// Average the distributions of several Anchor sets, then normalise.
pub fn average_distributions(distributions: &[Distribution]) -> Distribution {
    let mut sum = [0.0; RATINGS];
    for d in distributions {
        for (s, p) in sum.iter_mut().zip(d) {
            *s += p;
        }
    }
    let total: f64 = sum.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return [1.0 / RATINGS as f64; RATINGS];
    }
    sum.map(|s| s / total)
}

/// Expected Purchase intent, Σ r·p(r), on the 1–5 scale.
pub fn expected_rating(distribution: &Distribution) -> f64 {
    distribution
        .iter()
        .enumerate()
        .map(|(i, p)| (i + 1) as f64 * p)
        .sum()
}

/// Scores answers with SSR: an Embedder plus the embedded Anchor sets.
pub struct SsrScorer<'a> {
    embedder: &'a dyn Embedder,
    anchor_embeddings: Vec<[Vec<f64>; RATINGS]>,
    epsilon: f64,
}

impl<'a> SsrScorer<'a> {
    /// Embed every Anchor set once, up front.
    pub fn new(
        embedder: &'a dyn Embedder,
        anchor_sets: &[AnchorSet],
        epsilon: f64,
    ) -> Result<Self> {
        if anchor_sets.is_empty() {
            bail!("SSR needs at least one Anchor set");
        }
        let mut anchor_embeddings = Vec::with_capacity(anchor_sets.len());
        for set in anchor_sets {
            let texts: Vec<&str> = set.statements.iter().map(String::as_str).collect();
            let vectors = embedder
                .embed(&texts)
                .with_context(|| format!("embedding Anchor set `{}`", set.id))?;
            let vectors: [Vec<f64>; RATINGS] = vectors.try_into().map_err(|v: Vec<_>| {
                anyhow::anyhow!(
                    "Scorer `{}` returned {} vectors for {} anchor statements",
                    embedder.name(),
                    v.len(),
                    RATINGS
                )
            })?;
            anchor_embeddings.push(vectors);
        }
        Ok(Self {
            embedder,
            anchor_embeddings,
            epsilon,
        })
    }

    /// The Purchase intent distribution of one answer.
    pub fn score(&self, answer: &str) -> Result<Distribution> {
        let vector = self
            .embedder
            .embed(&[answer])?
            .into_iter()
            .next()
            .context("Scorer returned no vector for the answer")?;
        let per_set: Vec<Distribution> = self
            .anchor_embeddings
            .iter()
            .map(|anchors| {
                let similarities = std::array::from_fn(|r| gamma(&vector, &anchors[r]));
                similarities_to_distribution(&similarities, self.epsilon)
            })
            .collect();
        Ok(average_distributions(&per_set))
    }
}
