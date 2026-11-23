//! The report: ranking with an uncertainty band, age and income breakdowns,
//! quotes, the fixed limits section and run details. `analyse` computes
//! everything once; `markdown` (here) and `report_html::html` render it.
//!
//! Only age and income are broken down. The paper did not reproduce real
//! differences by gender, region or ethnicity, so those are never presented
//! as findings: the report names them as left out.

use crate::anchors::RATINGS;
use crate::customer_mix::Rng;
use crate::run::AnswerRecord;
use crate::run_file::{RespondentChoice, RunFile};
use crate::ssr::Distribution;
use std::fmt::Write;

/// Bootstrap resamples over customers.
pub const BOOTSTRAP_RESAMPLES: usize = 1000;
/// The band is the middle 95% of the resampled averages.
pub const BAND_LOW: f64 = 0.025;
pub const BAND_HIGH: f64 = 0.975;
/// Quotes per list (reasons to buy, objections) and Concept.
pub const QUOTES_PER_LIST: usize = 3;
/// Quotes are cut to about this many characters.
pub const QUOTE_MAX_CHARS: usize = 220;
/// Groups smaller than this are flagged in breakdowns.
pub const SMALL_GROUP: usize = 30;
/// Fewer customers than this per Concept: the band is not trustworthy.
pub const MIN_RELIABLE: usize = 10;

/// The limits section: always in the report, whatever the run found.
pub const LIMITS: [&str; 5] = [
    "**Stated intent is not sales.** Purchase intent is what the Synthetic customers say they \
     would do. Real shoppers buy less often than they say they will.",
    "**Best for everyday consumer products the AI knows well.** Toothpaste, snacks and household \
     goods suit the method. Niche, technical or very new products are where the AI knows least.",
    "**A comparison means more than one score alone.** Use the ranking of several Concepts, not \
     one Concept's average on its own, and treat \"too close to call\" as exactly that.",
    "**The paper's numbers came from other models.** arXiv 2510.08338 used GPT-4o and \
     Gemini 2.0 Flash to play the customers; this run used the model named in the run details.",
    "**A local Scorer differs from the paper's.** The paper scored answers with OpenAI \
     text-embedding-3-small; a local Scorer is a different embedding model and gives different \
     numbers.",
];
pub const LIMITS_FOOTER: &str = "This is not a replacement for research with real people: the \
     paper itself says so. Use it to compare ideas early and decide what to test with real \
     customers.";
pub const CREDITS: [&str; 2] = [
    "Method: *LLMs Reproduce Human Purchase Intent via Semantic Similarity Elicitation of Likert \
     Ratings*, PyMC Labs and Colgate-Palmolive, arXiv 2510.08338.",
    "Semantic similarity rating (SSR) after PyMC Labs' code, github.com/pymc-labs/\
     semantic-similarity-rating (Apache-2.0).",
];

/// One Concept's place in the ranking.
#[derive(Debug, Clone)]
pub struct ConceptResult {
    pub concept: String,
    pub customers: usize,
    pub average: f64,
    pub spread: Distribution,
    /// 95% bootstrap band of the average; none without answers.
    pub band: Option<(f64, f64)>,
    /// 1-based rank by average; none without answers.
    pub rank: Option<usize>,
    /// Other Concepts whose band overlaps this one's.
    pub too_close_to: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakdownKind {
    Age,
    Income,
}

impl BreakdownKind {
    pub fn label(self) -> &'static str {
        match self {
            BreakdownKind::Age => "Age group",
            BreakdownKind::Income => "Income",
        }
    }
}

/// Average Purchase intent per group (rows) and Concept (columns).
#[derive(Debug, Clone)]
pub struct Breakdown {
    pub kind: BreakdownKind,
    /// The attribute's name in the Customer mix.
    pub attribute: String,
    pub groups: Vec<String>,
    /// `cells[group][concept]`: (average, customers), none when nobody.
    pub cells: Vec<Vec<Option<(f64, usize)>>>,
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub text: String,
    /// Age and income of the customer, for example `35-54, middle`.
    pub who: String,
    pub rating: f64,
}

#[derive(Debug, Clone)]
pub struct ConceptQuotes {
    pub concept: String,
    pub reasons: Vec<Quote>,
    pub objections: Vec<Quote>,
}

pub struct Analysis {
    pub concepts: Vec<ConceptResult>,
    /// Concept names in rank order (Concepts without answers last).
    pub ranking: Vec<usize>,
    pub breakdowns: Vec<Breakdown>,
    /// Customer mix attributes that are not broken down.
    pub left_out: Vec<String>,
    pub quotes: Vec<ConceptQuotes>,
    pub answered: usize,
    pub planned: usize,
    pub model: String,
    pub scorer: String,
    pub seed: u64,
    pub date: Option<String>,
    pub customers_per_concept: usize,
}

pub fn analyse(
    run_file: &RunFile,
    scorer_name: &str,
    seed: u64,
    date: Option<&str>,
    records: &[AnswerRecord],
) -> Analysis {
    let mut concepts: Vec<ConceptResult> = Vec::new();
    for (ci, c) in run_file.concepts.iter().enumerate() {
        let rows: Vec<&AnswerRecord> = records.iter().filter(|r| r.concept == c.name).collect();
        let n = rows.len();
        let mut spread = [0.0; RATINGS];
        let mut average = 0.0;
        for r in &rows {
            for (s, p) in spread.iter_mut().zip(&r.distribution) {
                *s += p;
            }
            average += r.expected_rating;
        }
        if n > 0 {
            spread = spread.map(|s| s / n as f64);
            average /= n as f64;
        }
        let ratings: Vec<f64> = rows.iter().map(|r| r.expected_rating).collect();
        concepts.push(ConceptResult {
            concept: c.name.clone(),
            customers: n,
            average,
            spread,
            band: bootstrap_band(&ratings, seed, ci),
            rank: None,
            too_close_to: Vec::new(),
        });
    }

    let mut ranking: Vec<usize> = (0..concepts.len()).collect();
    ranking.sort_by(|&a, &b| {
        let (ca, cb) = (&concepts[a], &concepts[b]);
        (ca.customers == 0)
            .cmp(&(cb.customers == 0))
            .then(cb.average.total_cmp(&ca.average))
            .then(a.cmp(&b))
    });
    let mut rank = 0;
    for &i in &ranking {
        if concepts[i].customers > 0 {
            rank += 1;
            concepts[i].rank = Some(rank);
        }
    }
    for i in 0..concepts.len() {
        let Some((lo_i, hi_i)) = concepts[i].band else {
            continue;
        };
        let close: Vec<String> = (0..concepts.len())
            .filter(|&j| j != i)
            .filter(|&j| {
                concepts[j]
                    .band
                    .is_some_and(|(lo_j, hi_j)| lo_i <= hi_j && lo_j <= hi_i)
            })
            .map(|j| concepts[j].concept.clone())
            .collect();
        concepts[i].too_close_to = close;
    }

    let (breakdowns, left_out) = breakdowns(run_file, records);
    let used: Vec<String> = breakdowns.iter().map(|b| b.attribute.clone()).collect();
    let quotes = run_file
        .concepts
        .iter()
        .map(|c| quotes_for(&c.name, records, &used))
        .collect();

    let model = match run_file.respondent {
        RespondentChoice::Claude => run_file.model.as_str().to_string(),
        RespondentChoice::Codex => "codex (default Codex model)".to_string(),
    };
    Analysis {
        answered: concepts.iter().map(|c| c.customers).sum(),
        planned: run_file.customers_per_concept * run_file.concepts.len(),
        concepts,
        ranking,
        breakdowns,
        left_out,
        quotes,
        model,
        scorer: scorer_name.to_string(),
        seed,
        date: date.map(str::to_string),
        customers_per_concept: run_file.customers_per_concept,
    }
}

/// 95% bootstrap band of the average over customers. Seeded from the run seed
/// and the Concept's position, so the same answers always give the same band.
fn bootstrap_band(ratings: &[f64], seed: u64, concept_index: usize) -> Option<(f64, f64)> {
    let n = ratings.len();
    if n == 0 {
        return None;
    }
    let mut rng =
        Rng::new(seed ^ (0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(concept_index as u64 + 1)));
    let mut means: Vec<f64> = (0..BOOTSTRAP_RESAMPLES)
        .map(|_| {
            let sum: f64 = (0..n)
                .map(|_| ratings[(rng.next_u64() % n as u64) as usize])
                .sum();
            sum / n as f64
        })
        .collect();
    means.sort_by(f64::total_cmp);
    let at = |q: f64| means[((q * (means.len() - 1) as f64).round() as usize).min(means.len() - 1)];
    Some((at(BAND_LOW), at(BAND_HIGH)))
}

/// Which breakdown, if any, a Customer mix attribute name stands for: whole
/// words only, so `language` and `image` are not ages.
pub fn breakdown_kind(attribute: &str) -> Option<BreakdownKind> {
    let lower = attribute.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words
        .iter()
        .any(|w| matches!(*w, "age" | "ages" | "agegroup"))
    {
        Some(BreakdownKind::Age)
    } else if words.iter().any(|w| matches!(*w, "income" | "incomes")) {
        Some(BreakdownKind::Income)
    } else {
        None
    }
}

fn breakdowns(run_file: &RunFile, records: &[AnswerRecord]) -> (Vec<Breakdown>, Vec<String>) {
    let mut result = Vec::new();
    let mut left_out = Vec::new();
    let mut seen = Vec::new();
    for (attribute, values) in run_file.customer_mix.attributes() {
        let Some(kind) = breakdown_kind(attribute) else {
            left_out.push(attribute.clone());
            continue;
        };
        if seen.contains(&kind) {
            // A second age-like or income-like attribute: one breakdown each.
            left_out.push(attribute.clone());
            continue;
        }
        seen.push(kind);
        let groups: Vec<String> = values.keys().cloned().collect();
        let cells = groups
            .iter()
            .map(|g| {
                run_file
                    .concepts
                    .iter()
                    .map(|c| {
                        let ratings: Vec<f64> = records
                            .iter()
                            .filter(|r| {
                                r.concept == c.name
                                    && r.attributes.get(attribute).is_some_and(|v| v == g)
                            })
                            .map(|r| r.expected_rating)
                            .collect();
                        (!ratings.is_empty()).then(|| {
                            (
                                ratings.iter().sum::<f64>() / ratings.len() as f64,
                                ratings.len(),
                            )
                        })
                    })
                    .collect()
            })
            .collect();
        result.push(Breakdown {
            kind,
            attribute: attribute.clone(),
            groups,
            cells,
        });
    }
    (result, left_out)
}

/// Strongest "would buy" answers as reasons, strongest "would not buy"
/// answers as objections, each distinct and quoted from a saved answer.
fn quotes_for(concept: &str, records: &[AnswerRecord], used: &[String]) -> ConceptQuotes {
    let mut rows: Vec<&AnswerRecord> = records.iter().filter(|r| r.concept == concept).collect();
    rows.sort_by(|a, b| {
        b.expected_rating
            .total_cmp(&a.expected_rating)
            .then(a.customer.cmp(&b.customer))
    });
    let pick = |ordered: Vec<&AnswerRecord>| -> Vec<Quote> {
        let mut seen: Vec<String> = Vec::new();
        let mut quotes = Vec::new();
        for r in ordered {
            let text = short_quote(&r.answer);
            if text.is_empty() || seen.contains(&text) {
                continue;
            }
            seen.push(text.clone());
            let who: Vec<&str> = r
                .attributes
                .iter()
                .filter(|(a, _)| used.iter().any(|u| u == *a))
                .map(|(_, v)| v.as_str())
                .collect();
            quotes.push(Quote {
                text,
                who: who.join(", "),
                rating: r.expected_rating,
            });
            if quotes.len() == QUOTES_PER_LIST {
                break;
            }
        }
        quotes
    };
    let reasons = pick(
        rows.iter()
            .copied()
            .filter(|r| r.expected_rating > 3.0)
            .collect(),
    );
    let objections = pick(
        rows.iter()
            .rev()
            .copied()
            .filter(|r| r.expected_rating < 3.0)
            .collect(),
    );
    ConceptQuotes {
        concept: concept.to_string(),
        reasons,
        objections,
    }
}

/// The answer on one line, cut to whole sentences (or a word boundary and an
/// ellipsis) when long. Never reworded.
pub fn short_quote(answer: &str) -> String {
    let text = answer.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= QUOTE_MAX_CHARS {
        return text;
    }
    let head: String = text.chars().take(QUOTE_MAX_CHARS).collect();
    let bytes = head.as_bytes();
    let sentence_end = head.char_indices().rev().find(|&(i, c)| {
        matches!(c, '.' | '!' | '?')
            && i >= 40
            && bytes.get(i + 1).is_none_or(|b| b.is_ascii_whitespace())
    });
    if let Some((end, _)) = sentence_end {
        return format!("{} …", &head[..=end]);
    }
    match head.rfind(' ') {
        Some(space) => format!("{}…", &head[..space]),
        None => format!("{head}…"),
    }
}

pub fn markdown(a: &Analysis) -> String {
    let mut md = String::new();
    let _ = writeln!(md, "# Synthetic customers report\n");
    let _ = writeln!(
        md,
        "Purchase intent on a 1–5 scale (1 = would definitely not buy, 5 = would definitely buy), \
         scored with semantic similarity rating (SSR, arXiv 2510.08338).\n"
    );

    if a.answered < a.planned {
        let _ = writeln!(
            md,
            "> **Short sample.** This run stopped early (for example at a usage limit): \
             {} of the {} planned Synthetic customers answered. \
             The results below are less certain than a full run's. \
             Run it again with `--resume` to ask only the missing customers.\n",
            a.answered, a.planned
        );
    }

    let _ = writeln!(md, "## Ranking\n");
    if a.concepts
        .iter()
        .any(|c| c.customers > 0 && c.customers < MIN_RELIABLE)
    {
        let _ = writeln!(
            md,
            "> **Very few customers.** At least one Concept has fewer than {MIN_RELIABLE} \
             Synthetic customers: its band is unreliable, so do not read \"clear of the others\" \
             as certain.\n"
        );
    }
    let ranked: Vec<&ConceptResult> = a
        .ranking
        .iter()
        .map(|&i| &a.concepts[i])
        .filter(|c| c.rank.is_some())
        .collect();
    if ranked.is_empty() {
        let _ = writeln!(
            md,
            "No Synthetic customer has answered yet, so nothing is ranked.\n"
        );
    } else {
        let _ = writeln!(
            md,
            "| Rank | Concept | Average Purchase intent | 95% band | Customers | Against the others |"
        );
        let _ = writeln!(md, "|---:|---|---:|---|---:|---|");
        for c in &ranked {
            let (lo, hi) = c.band.unwrap_or((c.average, c.average));
            let _ = writeln!(
                md,
                "| {} | {} | {:.2} | {:.2} to {:.2} | {} | {} |",
                c.rank.unwrap_or(0),
                escape_cell(&c.concept),
                c.average,
                lo,
                hi,
                c.customers,
                escape_cell(&verdict(c, ranked.len()))
            );
        }
        let _ = writeln!(md, "\n{}\n", ranking_summary(&ranked));
        let _ = writeln!(
            md,
            "The band is a bootstrap over the Synthetic customers (1000 resamples, middle 95%). \
             Two Concepts whose bands overlap are **too close to call**: do not read their order \
             as a winner.\n"
        );
    }

    let _ = writeln!(md, "## Purchase intent per Concept\n");
    let _ = writeln!(
        md,
        "| Concept | Customers | Average Purchase intent | 1 | 2 | 3 | 4 | 5 |"
    );
    let _ = writeln!(md, "|---|---:|---:|---:|---:|---:|---:|---:|");
    for s in &a.concepts {
        if s.customers == 0 {
            // Stopped before anyone answered: no scores to show.
            let _ = writeln!(
                md,
                "| {} | 0 | — | — | — | — | — | — |",
                escape_cell(&s.concept)
            );
            continue;
        }
        let _ = write!(
            md,
            "| {} | {} | {:.2} |",
            escape_cell(&s.concept),
            s.customers,
            s.average
        );
        for p in s.spread {
            let _ = write!(md, " {:.1}% |", p * 100.0);
        }
        let _ = writeln!(md);
    }
    let _ = writeln!(
        md,
        "\nThe 1–5 columns are the spread: the share of Purchase intent at each rating, \
         averaged over the Synthetic customers.\n"
    );

    let _ = writeln!(md, "## Who wants it: age and income\n");
    if a.breakdowns.is_empty() {
        let _ = writeln!(
            md,
            "The Customer mix has no age or income attribute, so there is no breakdown.\n"
        );
    }
    for b in &a.breakdowns {
        let _ = writeln!(md, "### {} (`{}`)\n", b.kind.label(), b.attribute);
        let _ = write!(md, "| {} |", b.kind.label());
        for c in &a.concepts {
            let _ = write!(md, " {} |", escape_cell(&c.concept));
        }
        let _ = writeln!(md);
        let _ = writeln!(md, "|---|{}", "---:|".repeat(a.concepts.len()));
        for (g, row) in b.groups.iter().zip(&b.cells) {
            let _ = write!(md, "| {} |", escape_cell(g));
            for cell in row {
                match cell {
                    Some((avg, n)) if *n < SMALL_GROUP => {
                        let _ = write!(md, " {avg:.2} (n={n}, few customers) |");
                    }
                    Some((avg, n)) => {
                        let _ = write!(md, " {avg:.2} (n={n}) |");
                    }
                    None => {
                        let _ = write!(md, " — |");
                    }
                }
            }
            let _ = writeln!(md);
        }
        let _ = writeln!(
            md,
            "\nAverage Purchase intent, with the number of Synthetic customers in each group. \
             Groups are small and carry no uncertainty band: read them as hints, not findings.\n"
        );
    }
    if !a.left_out.is_empty() {
        let names: Vec<String> = a.left_out.iter().map(|n| format!("`{n}`")).collect();
        let _ = writeln!(
            md,
            "Left out on purpose: {}. Only age and income are broken down. The paper's Synthetic \
             customers did not reproduce real differences by gender, region or ethnicity, so \
             no result is shown for those: treat them as unreliable.\n",
            names.join(", ")
        );
    }

    let _ = writeln!(md, "## Reasons and objections, in their own words\n");
    let _ = writeln!(
        md,
        "Quoted from the saved answers: the strongest \"would buy\" and \"would not buy\" answers \
         of each Concept, unedited except for cutting long answers short.\n"
    );
    for q in &a.quotes {
        let _ = writeln!(md, "### {}\n", escape_cell(&q.concept));
        write_quotes(&mut md, "Top reasons to buy", &q.reasons);
        write_quotes(&mut md, "Top objections", &q.objections);
    }

    let _ = writeln!(md, "## Limits\n");
    for l in LIMITS {
        let _ = writeln!(md, "- {l}");
    }
    let _ = writeln!(md, "\n{LIMITS_FOOTER}\n");
    if a.scorer == "fake" {
        let _ = writeln!(
            md,
            "> This run used the `fake` Scorer, which exists for tests only: its numbers mean nothing.\n"
        );
    }

    let _ = writeln!(md, "## Run details\n");
    let _ = writeln!(md, "- Model playing the customers: {}", a.model);
    let _ = writeln!(md, "- Scorer: {}", a.scorer);
    let _ = writeln!(
        md,
        "- Customers who answered: {} of {} planned",
        a.answered, a.planned
    );
    let _ = writeln!(md, "- Customers per Concept: {}", a.customers_per_concept);
    let _ = writeln!(md, "- Seed: {}", a.seed);
    let _ = writeln!(md, "- Date: {}", a.date.as_deref().unwrap_or("unknown"));

    let _ = writeln!(md, "\n## Credits\n");
    for c in CREDITS {
        let _ = writeln!(md, "- {c}");
    }
    md
}

fn write_quotes(md: &mut String, title: &str, quotes: &[Quote]) {
    let _ = writeln!(md, "**{title}**\n");
    if quotes.is_empty() {
        let _ = writeln!(md, "No answer was clearly on this side.\n");
        return;
    }
    for q in quotes {
        let who = if q.who.is_empty() {
            String::new()
        } else {
            format!(" ({})", q.who)
        };
        let text = q
            .text
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('`', "'");
        let _ = writeln!(md, "> \"{text}\"{who}\n");
    }
}

/// The ranking table's last column.
pub fn verdict(c: &ConceptResult, ranked: usize) -> String {
    if ranked < 2 {
        "nothing to compare with".to_string()
    } else if c.too_close_to.is_empty() {
        "clear of the others".to_string()
    } else {
        format!("too close to call with {}", c.too_close_to.join(", "))
    }
}

/// One sentence under the ranking: who leads, or that the lead is too close.
pub fn ranking_summary(ranked: &[&ConceptResult]) -> String {
    let Some(first) = ranked.first() else {
        return String::new();
    };
    if ranked.len() < 2 {
        return format!(
            "Only one Concept ran: {} averages {:.2}. One score alone says little; compare Concepts.",
            first.concept, first.average
        );
    }
    if first.too_close_to.is_empty() {
        format!(
            "{} leads, clear of the others: its band does not overlap theirs.",
            first.concept
        )
    } else {
        let mut tied = vec![first.concept.clone()];
        tied.extend(first.too_close_to.iter().cloned());
        format!("Too close to call for first place: {}.", tied.join(", "))
    }
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}
