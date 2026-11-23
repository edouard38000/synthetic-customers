//! SSR maths (arXiv 2510.08338 eq. 8, T = 1) on small hand-worked examples.

use anyhow::Result;
use std::collections::HashMap;
use synthetic_customers::anchors::{AnchorSet, anchor_sets};
use synthetic_customers::embedder::{Embedder, FakeEmbedder};
use synthetic_customers::ssr::{
    Distribution, SsrScorer, average_distributions, expected_rating, similarities_to_distribution,
};

fn assert_close(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a - e).abs() < 1e-9,
            "expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn one_anchor_set_without_epsilon() {
    // γ = .2 .4 .6 .8 1.0 → minus the minimum .2 → 0 .2 .4 .6 .8, sum 2.0
    let d = similarities_to_distribution(&[0.2, 0.4, 0.6, 0.8, 1.0], 0.0);
    assert_close(&d, &[0.0, 0.1, 0.2, 0.3, 0.4]);
    // 1·0 + 2·.1 + 3·.2 + 4·.3 + 5·.4 = 4.0
    assert!((expected_rating(&d) - 4.0).abs() < 1e-9);
}

#[test]
fn epsilon_goes_only_to_the_first_minimum() {
    // γ = .5 .6 .5 .7 .7, ε = .1. The minimum .5 is at ratings 1 and 3; ε goes
    // to the first one. Numerator .1 .1 0 .2 .2, denominator .6.
    let d = similarities_to_distribution(&[0.5, 0.6, 0.5, 0.7, 0.7], 0.1);
    assert_close(&d, &[1.0 / 6.0, 1.0 / 6.0, 0.0, 1.0 / 3.0, 1.0 / 3.0]);
}

#[test]
fn all_equal_similarities_give_a_uniform_distribution() {
    // ε = 0: the formula is 0/0, the answer says nothing → uniform.
    let d = similarities_to_distribution(&[0.7; 5], 0.0);
    assert_close(&d, &[0.2; 5]);
    // ε > 0: everything goes to the first minimum, as in the formula.
    let d = similarities_to_distribution(&[0.7; 5], 0.1);
    assert_close(&d, &[1.0, 0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn anchor_sets_are_averaged_as_distributions_not_similarities() {
    let a = similarities_to_distribution(&[0.2, 0.4, 0.6, 0.8, 1.0], 0.0); // 0 .1 .2 .3 .4
    let b = similarities_to_distribution(&[1.0, 0.5, 0.5, 0.5, 0.5], 0.0); // 1 0 0 0 0
    assert_close(&b, &[1.0, 0.0, 0.0, 0.0, 0.0]);
    let averaged = average_distributions(&[a, b]);
    assert_close(&averaged, &[0.5, 0.05, 0.1, 0.15, 0.2]);

    // Averaging the similarities first would give .6 .45 .55 .65 .75 →
    // .2 0 .133 .267 .4, which is not eq. 8.
    let wrong = similarities_to_distribution(&[0.6, 0.45, 0.55, 0.65, 0.75], 0.0);
    assert!((wrong[0] - averaged[0]).abs() > 0.1);
}

/// An Embedder with a fixed table, for the scorer tests below.
struct TableEmbedder(HashMap<String, Vec<f64>>);

impl Embedder for TableEmbedder {
    fn name(&self) -> &str {
        "table"
    }
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>> {
        Ok(texts.iter().map(|t| self.0[*t].clone()).collect())
    }
}

fn unit(r: usize) -> Vec<f64> {
    let mut v = vec![0.0; 5];
    v[r] = 1.0;
    v
}

fn one_hot_anchor_set(id: &str) -> (AnchorSet, Vec<(String, Vec<f64>)>) {
    let statements = ["a1", "a2", "a3", "a4", "a5"].map(|s| format!("{id}-{s}"));
    let table = statements
        .iter()
        .enumerate()
        .map(|(r, s)| (s.clone(), unit(r)))
        .collect();
    let set = AnchorSet {
        id: id.to_string(),
        statements,
    };
    (set, table)
}

#[test]
fn scorer_uses_one_plus_cosine_over_two() {
    // Anchors e1..e5, answer (0, 0, 0, .6, .8): cosines 0 0 0 .6 .8,
    // γ = (1 + cos) / 2 = .5 .5 .5 .8 .9. With ε = .1 on the first minimum:
    // numerator .1 0 0 .3 .4, denominator .8 → .125 0 0 .375 .5.
    // (Raw cosines would give .1 0 0 .6 .8 / 1.5 instead.)
    let (set, mut table) = one_hot_anchor_set("s");
    table.push(("answer".to_string(), vec![0.0, 0.0, 0.0, 0.6, 0.8]));
    let embedder = TableEmbedder(table.into_iter().collect());
    let scorer = SsrScorer::new(&embedder, &[set], 0.1).unwrap();
    let d = scorer.score("answer").unwrap();
    assert_close(&d, &[0.125, 0.0, 0.0, 0.375, 0.5]);
}

#[test]
fn scorer_averages_over_anchor_sets() {
    // Set "s" has its anchors in order, set "t" in reverse, so an answer equal
    // to e5 is rating 5 for "s" and rating 1 for "t": average .5 0 0 0 .5.
    let (s, mut table) = one_hot_anchor_set("s");
    let (mut t, t_table) = one_hot_anchor_set("t");
    t.statements.reverse();
    table.extend(t_table);
    table.push(("answer".to_string(), unit(4)));
    let embedder = TableEmbedder(table.into_iter().collect());
    let scorer = SsrScorer::new(&embedder, &[s, t], 0.0).unwrap();
    let d: Distribution = scorer.score("answer").unwrap();
    assert_close(&d, &[0.5, 0.0, 0.0, 0.0, 0.5]);
}

#[test]
fn fake_scorer_is_hand_computable() {
    let sets = anchor_sets();
    let embedder = FakeEmbedder::new(&sets);
    let scorer = SsrScorer::new(&embedder, &sets, 0.0).unwrap();
    assert_close(&scorer.score("5").unwrap(), &[0.0, 0.0, 0.0, 0.0, 1.0]);
    assert_close(
        &scorer.score("maybe 3 or 4").unwrap(),
        &[0.0, 0.0, 0.5, 0.5, 0.0],
    );
    assert_close(&scorer.score("no idea").unwrap(), &[0.2; 5]);
}
