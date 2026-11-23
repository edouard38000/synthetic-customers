//! The local Scorer must keep agreeing with the gold set: about 50 written
//! customer answers with the Purchase intent a person would give each.
//!
//! The test needs the local Scorer's model, so it runs only when:
//! - the model is already cached (`synthetic-customers setup` was run), or
//! - `CI` is set (GitHub Actions sets it), and then it downloads the model.
//!
//! Elsewhere it is skipped with a message, so `cargo test` needs no network.
//! `SYNTHETIC_CUSTOMERS_SKIP_GOLD=1` skips it always, for CI without network.
//! Thresholds and the measured values are in `docs/scorer.md`.

use std::path::Path;
use synthetic_customers::anchors::anchor_sets;
use synthetic_customers::gold;
use synthetic_customers::local_scorer::{self, DEFAULT_MODEL, LocalEmbedder};
use synthetic_customers::ssr::{DEFAULT_EPSILON, SsrScorer};

/// Fail below this Spearman rank correlation with the human ratings.
/// First measurement (bge-small-en-v1.5, 2026-11-23): 0.923.
const MIN_SPEARMAN: f64 = 0.88;
/// Fail above this mean absolute error on the 1–5 scale.
/// First measurement (bge-small-en-v1.5, 2026-11-23): 0.801.
const MAX_MAE: f64 = 0.90;

/// An environment switch is on unless unset, empty, `0` or `false`.
fn is_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !matches!(v.trim(), "" | "0" | "false" | "FALSE"))
}

#[test]
fn local_scorer_agrees_with_the_gold_set() {
    if is_on("SYNTHETIC_CUSTOMERS_SKIP_GOLD") {
        eprintln!("skipped: SYNTHETIC_CUSTOMERS_SKIP_GOLD is set");
        return;
    }
    let cache = local_scorer::cache_dir().unwrap();
    if DEFAULT_MODEL.cached_files(&cache).is_none() {
        if !is_on("CI") {
            eprintln!(
                "skipped: the local Scorer's model is not cached; run \
                 `synthetic-customers setup` (or set CI=1) to run this test"
            );
            return;
        }
        DEFAULT_MODEL.download(&cache).unwrap();
    }

    let gold = gold::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gold_set.json"))
        .unwrap();
    let embedder = LocalEmbedder::load(DEFAULT_MODEL, &cache).unwrap();
    let ssr = SsrScorer::new(&embedder, &anchor_sets(), DEFAULT_EPSILON).unwrap();
    let (agreement, predicted) = gold::measure(&ssr, &gold).unwrap();

    eprintln!(
        "{}: Spearman {:.3}, MAE {:.3} on {} gold answers",
        DEFAULT_MODEL.name,
        agreement.spearman,
        agreement.mae,
        gold.len()
    );
    for (g, p) in gold.iter().zip(&predicted) {
        eprintln!("  human {} scored {:.2}  {}", g.score, p, g.product);
    }
    assert!(
        agreement.spearman >= MIN_SPEARMAN,
        "Spearman {:.3} is below {MIN_SPEARMAN}",
        agreement.spearman
    );
    assert!(
        agreement.mae <= MAX_MAE,
        "MAE {:.3} is above {MAX_MAE}",
        agreement.mae
    );
}

#[test]
fn gold_set_covers_every_rating_and_many_products() {
    let gold = gold::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gold_set.json"))
        .unwrap();
    assert!((45..=60).contains(&gold.len()), "{} answers", gold.len());
    for rating in 1..=5u8 {
        let n = gold.iter().filter(|g| g.score == rating).count();
        assert!(n >= 8, "only {n} answers rated {rating}");
    }
    let mut products: Vec<&str> = gold.iter().map(|g| g.product.as_str()).collect();
    products.sort_unstable();
    products.dedup();
    assert_eq!(
        products.len(),
        gold.len(),
        "every answer is about its own product"
    );
}

/// Opt-in switch for the hosted Scorers' gold-set check: it calls the real
/// APIs with the user's keys and costs a fraction of a cent.
const LIVE_ENV: &str = "SYNTHETIC_CUSTOMERS_LIVE_SCORERS";

/// The hosted Scorers on the gold set. Skipped unless `LIVE_ENV` is on and
/// the provider's key is set, so `cargo test` and CI never need a key.
/// Until a first measurement sets their own thresholds (`docs/scorer.md`),
/// they are held to the local Scorer's.
#[test]
fn hosted_scorers_agree_with_the_gold_set() {
    use synthetic_customers::embedder::Embedder;
    use synthetic_customers::hosted_scorer::{HostedEmbedder, Provider};

    if !is_on(LIVE_ENV) {
        eprintln!("skipped: set {LIVE_ENV}=1 and OPENAI_API_KEY / VOYAGE_API_KEY to run it");
        return;
    }
    let gold = gold::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/gold_set.json"))
        .unwrap();
    for provider in [Provider::OpenAi, Provider::Voyage] {
        if std::env::var(provider.key_env()).map_or(true, |k| k.trim().is_empty()) {
            eprintln!(
                "skipped {}: {} is not set",
                provider.model(),
                provider.key_env()
            );
            continue;
        }
        let embedder = HostedEmbedder::from_env(provider).unwrap();
        let ssr = SsrScorer::new(&embedder, &anchor_sets(), DEFAULT_EPSILON).unwrap();
        let (agreement, _) = gold::measure(&ssr, &gold).unwrap();
        eprintln!(
            "{}: Spearman {:.3}, MAE {:.3} on {} gold answers",
            embedder.name(),
            agreement.spearman,
            agreement.mae,
            gold.len()
        );
        assert!(
            agreement.spearman >= MIN_SPEARMAN,
            "{}: Spearman {:.3} is below {MIN_SPEARMAN}",
            embedder.name(),
            agreement.spearman
        );
        assert!(
            agreement.mae <= MAX_MAE,
            "{}: MAE {:.3} is above {MAX_MAE}",
            embedder.name(),
            agreement.mae
        );
    }
}
