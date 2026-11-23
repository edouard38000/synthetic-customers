//! Anchor sets: five reference statements each, one per Purchase intent rating.

/// Points on the Purchase intent scale (1–5).
pub const RATINGS: usize = 5;

/// Five reference statements, index 0 = rating 1 ("would not buy").
#[derive(Debug, Clone)]
pub struct AnchorSet {
    pub id: String,
    pub statements: [String; RATINGS],
}

impl AnchorSet {
    pub fn new(id: &str, statements: [&str; RATINGS]) -> Self {
        Self {
            id: id.to_string(),
            statements: statements.map(str::to_string),
        }
    }
}

/// Placeholder Anchor sets for the walking skeleton. The real sets (PyMC
/// Labs' published ones, or six sets written from the paper's App. C.1)
/// replace these with the local Scorer.
pub fn placeholder_anchor_sets() -> Vec<AnchorSet> {
    vec![
        AnchorSet::new(
            "placeholder-1",
            [
                "I would definitely not buy this product.",
                "I probably would not buy this product.",
                "I am not sure whether I would buy this product.",
                "I would probably buy this product.",
                "I would definitely buy this product.",
            ],
        ),
        AnchorSet::new(
            "placeholder-2",
            [
                "This is not for me at all, I'd never purchase it.",
                "It doesn't really appeal to me, I'd likely pass.",
                "I could go either way on buying it.",
                "It appeals to me and I'd likely purchase it.",
                "I love it and would purchase it right away.",
            ],
        ),
    ]
}
