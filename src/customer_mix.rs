//! The Customer mix and sampling Synthetic customers from it.
//!
//! Sampling is by quota: for each attribute, the counts of each value match
//! the stated shares exactly, rounded by largest remainder. Each attribute's
//! values are then shuffled independently, so combinations are random while
//! every attribute's counts stay exact.

use crate::run_file::SHARE_TOLERANCE;
use anyhow::{Result, bail};
use indexmap::IndexMap;
use serde::Serialize;

/// Confirmed shares per attribute: attribute → (value → share), shares sum to 1.
#[derive(Debug, Clone)]
pub struct CustomerMix {
    attributes: IndexMap<String, IndexMap<String, f64>>,
}

/// One AI-played persona: a value for every attribute of the Customer mix.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SyntheticCustomer {
    pub attributes: IndexMap<String, String>,
}

impl CustomerMix {
    /// Validate and normalise the shares.
    pub fn new(attributes: IndexMap<String, IndexMap<String, f64>>) -> Result<Self> {
        if attributes.is_empty() {
            bail!(
                "empty Customer mix: add at least one [customer_mix.<attribute>] table, \
                 e.g. [customer_mix.age] with \"18-34\" = 0.5 and \"35+\" = 0.5"
            );
        }
        let mut normalised = IndexMap::new();
        for (attribute, values) in attributes {
            if values.is_empty() {
                bail!("Customer mix attribute `{attribute}` has no values");
            }
            for (value, share) in &values {
                if !share.is_finite() || *share <= 0.0 {
                    bail!(
                        "Customer mix `{attribute}`: share of `{value}` must be above 0, got {share}"
                    );
                }
            }
            let total: f64 = values.values().sum();
            if (total - 1.0).abs() > SHARE_TOLERANCE {
                bail!(
                    "Customer mix `{attribute}`: shares add up to {total:.3}, they must add up to 1 \
                     (write 0.25, not 25)"
                );
            }
            let values = values.into_iter().map(|(v, s)| (v, s / total)).collect();
            normalised.insert(attribute, values);
        }
        Ok(Self {
            attributes: normalised,
        })
    }

    pub fn attributes(&self) -> &IndexMap<String, IndexMap<String, f64>> {
        &self.attributes
    }

    /// Sample `n` Synthetic customers in the stated shares.
    pub fn sample(&self, n: usize, rng: &mut Rng) -> Vec<SyntheticCustomer> {
        let mut columns: Vec<(&str, Vec<&str>)> = Vec::new();
        for (attribute, values) in &self.attributes {
            let shares: Vec<f64> = values.values().copied().collect();
            let counts = quota_counts(&shares, n);
            let mut column: Vec<&str> = values
                .keys()
                .zip(counts)
                .flat_map(|(v, c)| std::iter::repeat_n(v.as_str(), c))
                .collect();
            rng.shuffle(&mut column);
            columns.push((attribute.as_str(), column));
        }
        (0..n)
            .map(|i| SyntheticCustomer {
                attributes: columns
                    .iter()
                    .map(|(a, col)| (a.to_string(), col[i].to_string()))
                    .collect(),
            })
            .collect()
    }
}

/// Split `n` into counts proportional to `shares` (summing to 1), by largest
/// remainder; ties go to the earlier value.
pub fn quota_counts(shares: &[f64], n: usize) -> Vec<usize> {
    let exact: Vec<f64> = shares.iter().map(|s| s * n as f64).collect();
    let mut counts: Vec<usize> = exact.iter().map(|x| x.floor() as usize).collect();
    let assigned: usize = counts.iter().sum();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = exact[a] - exact[a].floor();
        let rb = exact[b] - exact[b].floor();
        rb.total_cmp(&ra).then(a.cmp(&b))
    });
    for &i in order.iter().take(n.saturating_sub(assigned)) {
        counts[i] += 1;
    }
    counts
}

/// A small seeded random number generator (SplitMix64), so a seed in the run
/// file gives the same Synthetic customers every time.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_counts_round_by_largest_remainder() {
        assert_eq!(quota_counts(&[0.5, 0.5], 10), vec![5, 5]);
        // 1/3 each of 10 → 3.33 each; the one left over goes to the first.
        assert_eq!(quota_counts(&[1.0 / 3.0; 3], 10), vec![4, 3, 3]);
        // 0.25 / 0.35 / 0.40 of 7 → 1.75 / 2.45 / 2.8 → 2 / 2 / 3.
        assert_eq!(quota_counts(&[0.25, 0.35, 0.40], 7), vec![2, 2, 3]);
    }
}
