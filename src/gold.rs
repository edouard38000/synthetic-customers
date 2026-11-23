//! The gold set: written customer answers with the Purchase intent a person
//! would give each, and how well a Scorer agrees with them.

use crate::ssr::{SsrScorer, expected_rating};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

/// One written answer and its human-given rating (1–5).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldAnswer {
    pub product: String,
    pub answer: String,
    pub score: u8,
}

/// Read the gold set (a JSON array of [`GoldAnswer`]).
pub fn load(path: &Path) -> Result<Vec<GoldAnswer>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read the gold set {}", path.display()))?;
    let gold: Vec<GoldAnswer> = serde_json::from_str(&text)
        .with_context(|| format!("invalid gold set {}", path.display()))?;
    if gold.is_empty() {
        bail!("the gold set {} is empty", path.display());
    }
    if let Some(bad) = gold.iter().find(|g| !(1..=5).contains(&g.score)) {
        bail!(
            "gold answer for {} has score {}, not 1–5",
            bad.product,
            bad.score
        );
    }
    Ok(gold)
}

/// How well a Scorer's expected Purchase intent matches the human ratings.
#[derive(Debug, Clone, Copy)]
pub struct Agreement {
    /// Spearman rank correlation, ties averaged.
    pub spearman: f64,
    /// Mean absolute error on the 1–5 scale.
    pub mae: f64,
}

/// Score every gold answer and compare with the human ratings. Also returns
/// each answer's expected Purchase intent, in order.
pub fn measure(scorer: &SsrScorer, gold: &[GoldAnswer]) -> Result<(Agreement, Vec<f64>)> {
    let answers: Vec<&str> = gold.iter().map(|g| g.answer.as_str()).collect();
    let predicted: Vec<f64> = scorer
        .score_many(&answers)?
        .iter()
        .map(expected_rating)
        .collect();
    let human: Vec<f64> = gold.iter().map(|g| g.score as f64).collect();
    let mae = predicted
        .iter()
        .zip(&human)
        .map(|(p, h)| (p - h).abs())
        .sum::<f64>()
        / gold.len().max(1) as f64;
    let agreement = Agreement {
        spearman: spearman(&predicted, &human),
        mae,
    };
    Ok((agreement, predicted))
}

/// Spearman rank correlation: Pearson correlation of the ranks, tied values
/// sharing their average rank.
pub fn spearman(a: &[f64], b: &[f64]) -> f64 {
    pearson(&ranks(a), &ranks(b))
}

fn ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&i, &j| values[i].total_cmp(&values[j]));
    let mut ranks = vec![0.0; values.len()];
    let mut start = 0;
    while start < order.len() {
        let mut end = start;
        while end + 1 < order.len() && values[order[end + 1]] == values[order[start]] {
            end += 1;
        }
        let rank = (start + end) as f64 / 2.0 + 1.0;
        for &i in &order[start..=end] {
            ranks[i] = rank;
        }
        start = end + 1;
    }
    ranks
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let ma = a.iter().sum::<f64>() / n;
    let mb = b.iter().sum::<f64>() / n;
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    if va == 0.0 || vb == 0.0 {
        return 0.0;
    }
    cov / (va * vb).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spearman_of_a_monotone_relation_is_one() {
        assert!((spearman(&[1.0, 2.0, 3.0], &[10.0, 20.0, 35.0]) - 1.0).abs() < 1e-12);
        assert!((spearman(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn tied_values_share_their_average_rank() {
        assert_eq!(ranks(&[5.0, 1.0, 5.0, 3.0]), vec![3.5, 1.0, 3.5, 2.0]);
    }
}
