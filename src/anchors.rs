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

/// The six Anchor sets every Scorer scores against.
///
/// PyMC Labs' code (github.com/pymc-labs/semantic-similarity-rating) does not
/// publish the paper's purchase-intent statements, and the paper describes
/// them without listing them. These six sets of five are written from its
/// App. C.1: short, generic and product-independent; the lowest statement
/// says a purchase is unlikely, the middle one is indifferent or unsure, the
/// highest says the customer would buy, and the two between sit halfway.
/// Each set words purchase intent in a different way. See `docs/scorer.md`.
pub fn anchor_sets() -> Vec<AnchorSet> {
    vec![
        AnchorSet::new(
            "likelihood",
            [
                "I would definitely not buy it.",
                "I probably would not buy it.",
                "I am not sure whether I would buy it or not.",
                "I would probably buy it.",
                "I would definitely buy it.",
            ],
        ),
        AnchorSet::new(
            "interest",
            [
                "This doesn't interest me at all, I wouldn't purchase it.",
                "I'm not very interested and would likely pass on it.",
                "I'm indifferent, I could take it or leave it.",
                "It interests me and I'd likely purchase it.",
                "I really want this and would purchase it.",
            ],
        ),
        AnchorSet::new(
            "fit",
            [
                "It's not for me, I'd never get it.",
                "It's probably not for me.",
                "It might or might not be for me.",
                "It seems like a good fit for me, I'd likely get it.",
                "It's exactly what I want, I'd get it for sure.",
            ],
        ),
        AnchorSet::new(
            "chance",
            [
                "There is no chance I would buy this product.",
                "There is a small chance I would buy this product.",
                "There is about an even chance I would buy this product.",
                "There is a good chance I would buy this product.",
                "I am certain I would buy this product.",
            ],
        ),
        AnchorSet::new(
            "trial",
            [
                "I wouldn't even try it.",
                "I doubt I'd try it.",
                "Maybe I'd try it, maybe not.",
                "I'd likely give it a try.",
                "I'd buy it right away.",
            ],
        ),
        AnchorSet::new(
            "spoken",
            [
                "No, I'm not going to buy that.",
                "I don't think I'd buy that.",
                "I don't know if I'd buy that.",
                "I think I'd buy that.",
                "Yes, I'm going to buy that.",
            ],
        ),
    ]
}
