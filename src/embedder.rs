//! The Embedder interface behind every Scorer, and the fake Scorer for tests.

use crate::anchors::{AnchorSet, RATINGS};
use anyhow::Result;
use std::collections::HashMap;

/// Turns texts into vectors. Real Scorers (local, OpenAI, Voyage) implement
/// this; SSR only needs the vectors.
pub trait Embedder: Send + Sync {
    /// Name shown in the report, e.g. `fake`.
    fn name(&self) -> &str;
    /// One vector per text, in order.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>>;
}

/// A fake Scorer whose results can be worked out by hand.
///
/// - Anchor statement `r` of any Anchor set embeds as the unit vector e_r.
/// - An answer embeds as the sum of e_d for each distinct digit d in 1–5 it
///   contains: "4" → e_4, so all the probability goes to rating 4; "3 or 4"
///   → e_3 + e_4, so ratings 3 and 4 get half each.
/// - An answer with no such digit embeds as (1, 1, 1, 1, 1): equally close
///   to every anchor, so the distribution is uniform (expected rating 3).
pub struct FakeEmbedder {
    anchors: HashMap<String, usize>,
}

impl FakeEmbedder {
    pub fn new(anchor_sets: &[AnchorSet]) -> Self {
        let anchors = anchor_sets
            .iter()
            .flat_map(|set| {
                set.statements
                    .iter()
                    .enumerate()
                    .map(|(r, s)| (s.clone(), r))
            })
            .collect();
        Self { anchors }
    }

    fn embed_one(&self, text: &str) -> Vec<f64> {
        let mut v = vec![0.0; RATINGS];
        if let Some(&r) = self.anchors.get(text) {
            v[r] = 1.0;
            return v;
        }
        for c in text.chars() {
            if let Some(d) = c.to_digit(10).filter(|d| (1..=RATINGS as u32).contains(d)) {
                v[d as usize - 1] = 1.0;
            }
        }
        if v.iter().all(|x| *x == 0.0) {
            v = vec![1.0; RATINGS];
        }
        v
    }
}

impl Embedder for FakeEmbedder {
    fn name(&self) -> &str {
        "fake"
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }
}
