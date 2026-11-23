//! The optional HTML report: one self-contained file, inline SVG charts, no
//! scripts, no fonts, no links and no network calls of any kind.

use crate::report::{
    Analysis, CREDITS, ConceptResult, LIMITS, LIMITS_FOOTER, Quote, SMALL_GROUP, ranking_summary,
    verdict,
};
use std::fmt::Write;

const PALETTE: [&str; 6] = [
    "#3b6ea8", "#d9822b", "#4a9d6f", "#a85a9b", "#c2453f", "#7a7a7a",
];
/// Chart width in SVG units; the SVG scales to its container.
const W: f64 = 640.0;
const LABEL_W: f64 = 190.0;

pub fn html(a: &Analysis) -> String {
    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Synthetic customers report</title><style>{CSS}</style></head><body><main>\n"
    );
    let _ = writeln!(h, "<h1>Synthetic customers report</h1>");
    let _ = writeln!(
        h,
        "<p>Purchase intent on a 1–5 scale (1 = would definitely not buy, 5 = would definitely \
         buy), scored with semantic similarity rating (SSR, arXiv 2510.08338).</p>"
    );
    if a.answered < a.planned {
        let _ = writeln!(
            h,
            "<p class=\"note\"><strong>Short sample.</strong> This run stopped early: {} of the {} \
             planned Synthetic customers answered. The results are less certain than a full \
             run's. Run it again with <code>--resume</code> to ask only the missing customers.</p>",
            a.answered, a.planned
        );
    }

    let ranked: Vec<&ConceptResult> = a
        .ranking
        .iter()
        .map(|&i| &a.concepts[i])
        .filter(|c| c.rank.is_some())
        .collect();
    let _ = writeln!(h, "<h2>Ranking</h2>");
    if a.concepts
        .iter()
        .any(|c| c.customers > 0 && c.customers < crate::report::MIN_RELIABLE)
    {
        let _ = writeln!(
            h,
            "<p class=\"note\"><strong>Very few customers.</strong> At least one Concept has \
             fewer than {} Synthetic customers: its band is unreliable.</p>",
            crate::report::MIN_RELIABLE
        );
    }
    if ranked.is_empty() {
        let _ = writeln!(h, "<p>No Synthetic customer has answered yet.</p>");
    } else {
        let _ = writeln!(h, "{}", ranking_chart(&ranked));
        let _ = writeln!(
            h,
            "<table><thead><tr><th>Rank</th><th>Concept</th><th>Average Purchase intent</th><th>95% band</th><th>Customers</th><th>Against the others</th></tr></thead><tbody>"
        );
        for c in &ranked {
            let (lo, hi) = c.band.unwrap_or((c.average, c.average));
            let _ = writeln!(
                h,
                "<tr><td>{}</td><td>{}</td><td>{:.2}</td><td>{:.2} to {:.2}</td><td>{}</td><td>{}</td></tr>",
                c.rank.unwrap_or(0),
                esc(&c.concept),
                c.average,
                lo,
                hi,
                c.customers,
                esc(&verdict(c, ranked.len()))
            );
        }
        let _ = writeln!(h, "</tbody></table>");
        let _ = writeln!(
            h,
            "<p><strong>{}</strong></p>",
            esc(&ranking_summary(&ranked))
        );
        let _ = writeln!(
            h,
            "<p>The band is a bootstrap over the Synthetic customers (1000 resamples, middle \
             95%). Two Concepts whose bands overlap are <strong>too close to call</strong>.</p>"
        );
    }

    let _ = writeln!(h, "<h2>Purchase intent per Concept</h2>");
    let _ = writeln!(h, "{}", spread_chart(a));

    let _ = writeln!(h, "<h2>Who wants it: age and income</h2>");
    if a.breakdowns.is_empty() {
        let _ = writeln!(
            h,
            "<p>The Customer mix has no age or income attribute, so there is no breakdown.</p>"
        );
    }
    for b in &a.breakdowns {
        let _ = writeln!(
            h,
            "<h3>{} (<code>{}</code>)</h3>",
            b.kind.label(),
            esc(&b.attribute)
        );
        let _ = writeln!(h, "{}", breakdown_chart(a, b));
        let _ = writeln!(
            h,
            "<p class=\"muted\">Average Purchase intent per group. Groups under {SMALL_GROUP} \
             customers are marked with an asterisk. No uncertainty band: read them as hints, \
             not findings.</p>"
        );
    }
    if !a.left_out.is_empty() {
        let names: Vec<String> = a
            .left_out
            .iter()
            .map(|n| format!("<code>{}</code>", esc(n)))
            .collect();
        let _ = writeln!(
            h,
            "<p class=\"note\">Left out on purpose: {}. Only age and income are broken down. The \
             paper's Synthetic customers did not reproduce real differences by gender, region \
             or ethnicity, so no result is shown for those: treat them as unreliable.</p>",
            names.join(", ")
        );
    }

    let _ = writeln!(h, "<h2>Reasons and objections, in their own words</h2>");
    let _ = writeln!(
        h,
        "<p>Quoted from the saved answers, unedited except for cutting long answers short.</p>"
    );
    for q in &a.quotes {
        let _ = writeln!(h, "<h3>{}</h3>", esc(&q.concept));
        quote_list(&mut h, "Top reasons to buy", &q.reasons);
        quote_list(&mut h, "Top objections", &q.objections);
    }

    let _ = writeln!(h, "<h2>Limits</h2><ul>");
    for l in LIMITS {
        let _ = writeln!(h, "<li>{}</li>", inline_md(&esc(l)));
    }
    let _ = writeln!(h, "</ul><p>{}</p>", esc(LIMITS_FOOTER));
    if a.scorer == "fake" {
        let _ = writeln!(
            h,
            "<p class=\"note\">This run used the <code>fake</code> Scorer, which exists for \
             tests only: its numbers mean nothing.</p>"
        );
    }

    let _ = writeln!(h, "<h2>Run details</h2><ul>");
    let _ = writeln!(h, "<li>Model playing the customers: {}</li>", esc(&a.model));
    let _ = writeln!(h, "<li>Scorer: {}</li>", esc(&a.scorer));
    let _ = writeln!(
        h,
        "<li>Customers who answered: {} of {} planned</li>",
        a.answered, a.planned
    );
    let _ = writeln!(
        h,
        "<li>Customers per Concept: {}</li>",
        a.customers_per_concept
    );
    let _ = writeln!(h, "<li>Seed: {}</li>", a.seed);
    let _ = writeln!(
        h,
        "<li>Date: {}</li></ul>",
        esc(a.date.as_deref().unwrap_or("unknown"))
    );
    let _ = writeln!(h, "<h2>Credits</h2><ul>");
    for c in CREDITS {
        let _ = writeln!(h, "<li>{}</li>", inline_md(&esc(c)));
    }
    let _ = writeln!(h, "</ul></main></body></html>");
    h
}

const CSS: &str = "\
:root{color-scheme:light dark;--fg:#1d1d1f;--bg:#fff;--muted:#6b6b70;--rule:#d9d9de;--note:#fff6e0}\
@media (prefers-color-scheme:dark){:root{--fg:#ececf0;--bg:#16161a;--muted:#a0a0a8;--rule:#38383f;--note:#2e2814}}\
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif}\
main{max-width:820px;margin:0 auto;padding:24px 16px 64px}\
h1,h2,h3{line-height:1.2}h2{margin-top:2em;border-bottom:1px solid var(--rule);padding-bottom:.2em}\
table{border-collapse:collapse;width:100%;margin:1em 0;font-size:.92em}\
th,td{border-bottom:1px solid var(--rule);padding:6px 8px;text-align:left}\
blockquote{margin:.5em 0;padding:.2em 1em;border-left:3px solid var(--rule)}\
.note{background:var(--note);padding:10px 14px;border-radius:6px}.muted{color:var(--muted)}\
svg{width:100%;height:auto;display:block}svg text{fill:var(--fg);font:12px system-ui,sans-serif}\
.axis{stroke:var(--rule)}.band{stroke:var(--fg);stroke-width:2}\
.legend{display:flex;flex-wrap:wrap;gap:4px 16px;font-size:.9em;margin:.4em 0}\
.legend i{display:inline-block;width:10px;height:10px;margin-right:6px;border-radius:2px}";

/// x position of a 1–5 rating inside the chart area.
fn x_of(rating: f64) -> f64 {
    LABEL_W + (rating - 1.0) / 4.0 * (W - LABEL_W - 20.0)
}

fn axis(h: &mut String, top: f64, bottom: f64) {
    for r in 1..=5 {
        let x = x_of(f64::from(r));
        let _ = write!(
            h,
            "<line class=\"axis\" x1=\"{x:.1}\" y1=\"{top:.1}\" x2=\"{x:.1}\" y2=\"{bottom:.1}\"/>\
             <text x=\"{x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{r}</text>",
            bottom + 16.0
        );
    }
}

fn ranking_chart(ranked: &[&ConceptResult]) -> String {
    let row = 34.0;
    let height = ranked.len() as f64 * row + 30.0;
    let mut s = format!(
        "<svg viewBox=\"0 0 {W} {height}\" role=\"img\" aria-label=\"Average Purchase intent per Concept with 95% band\">"
    );
    axis(&mut s, 0.0, ranked.len() as f64 * row);
    for (i, c) in ranked.iter().enumerate() {
        let y = i as f64 * row;
        let color = PALETTE[i % PALETTE.len()];
        let _ = write!(
            s,
            "<text x=\"0\" y=\"{:.1}\">{}</text>\
             <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"18\" fill=\"{color}\"/>",
            y + 21.0,
            esc(&clip(&c.concept, 28)),
            x_of(1.0),
            y + 8.0,
            x_of(c.average) - x_of(1.0)
        );
        if let Some((lo, hi)) = c.band {
            let _ = write!(
                s,
                "<line class=\"band\" x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\"/>\
                 <line class=\"band\" x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\"/>\
                 <line class=\"band\" x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\"/>",
                x_of(lo),
                y + 17.0,
                x_of(hi),
                y + 17.0,
                x_of(lo),
                y + 11.0,
                x_of(lo),
                y + 23.0,
                x_of(hi),
                y + 11.0,
                x_of(hi),
                y + 23.0
            );
        }
    }
    s.push_str("</svg>");
    s
}

fn spread_chart(a: &Analysis) -> String {
    let row = 34.0;
    let n = a.concepts.len() as f64;
    let shades = ["#c2453f", "#e08a4e", "#d4c35a", "#7cb46a", "#3b8f5c"];
    let mut s = format!(
        "<div class=\"legend\">{}</div><svg viewBox=\"0 0 {W} {}\" role=\"img\" \
         aria-label=\"Spread of Purchase intent per Concept\">",
        (1..=5)
            .map(|r| format!(
                "<span><i style=\"background:{}\"></i>{r}</span>",
                shades[r - 1]
            ))
            .collect::<String>(),
        n * row + 6.0
    );
    let width = W - LABEL_W - 20.0;
    for (i, c) in a.concepts.iter().enumerate() {
        let y = i as f64 * row;
        let _ = write!(
            s,
            "<text x=\"0\" y=\"{:.1}\">{}</text>",
            y + 21.0,
            esc(&clip(&c.concept, 28))
        );
        if c.customers == 0 {
            let _ = write!(
                s,
                "<text x=\"{LABEL_W}\" y=\"{:.1}\">no answers</text>",
                y + 21.0
            );
            continue;
        }
        let mut x = LABEL_W;
        for (r, p) in c.spread.iter().enumerate() {
            let w = p * width;
            let _ = write!(
                s,
                "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{w:.1}\" height=\"20\" fill=\"{}\"><title>{}: {:.1}%</title></rect>",
                y + 7.0,
                shades[r],
                r + 1,
                p * 100.0
            );
            x += w;
        }
    }
    s.push_str("</svg>");
    s
}

fn breakdown_chart(a: &Analysis, b: &crate::report::Breakdown) -> String {
    let bar = 12.0;
    let group_h = a.concepts.len() as f64 * bar + 12.0;
    let height = b.groups.len() as f64 * group_h + 30.0;
    let legend: String = a
        .concepts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "<span><i style=\"background:{}\"></i>{}</span>",
                PALETTE[i % PALETTE.len()],
                esc(&c.concept)
            )
        })
        .collect();
    let mut s = format!(
        "<div class=\"legend\">{legend}</div><svg viewBox=\"0 0 {W} {height}\" role=\"img\" \
         aria-label=\"Average Purchase intent by {}\">",
        b.kind.label().to_lowercase()
    );
    axis(&mut s, 0.0, b.groups.len() as f64 * group_h);
    for (gi, (g, row)) in b.groups.iter().zip(&b.cells).enumerate() {
        let y = gi as f64 * group_h;
        let small = row.iter().flatten().any(|(_, n)| *n < SMALL_GROUP);
        let _ = write!(
            s,
            "<text x=\"0\" y=\"{:.1}\">{}{}</text>",
            y + group_h / 2.0 + 2.0,
            esc(&clip(g, 24)),
            if small { " *" } else { "" }
        );
        for (ci, cell) in row.iter().enumerate() {
            if let Some((avg, n)) = cell {
                let _ = write!(
                    s,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\">\
                     <title>{}: {avg:.2} (n={n})</title></rect>",
                    x_of(1.0),
                    y + 4.0 + ci as f64 * bar,
                    x_of(*avg) - x_of(1.0),
                    bar - 2.0,
                    PALETTE[ci % PALETTE.len()],
                    esc(&a.concepts[ci].concept)
                );
            }
        }
    }
    s.push_str("</svg>");
    s
}

fn quote_list(h: &mut String, title: &str, quotes: &[Quote]) {
    let _ = writeln!(h, "<h4>{title}</h4>");
    if quotes.is_empty() {
        let _ = writeln!(
            h,
            "<p class=\"muted\">No answer was clearly on this side.</p>"
        );
    }
    for q in quotes {
        let who = if q.who.is_empty() {
            String::new()
        } else {
            format!(" <span class=\"muted\">({})</span>", esc(&q.who))
        };
        let _ = writeln!(h, "<blockquote>“{}”{who}</blockquote>", esc(&q.text));
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
}

/// Escape text for HTML, whatever it holds: answers come from an AI.
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The fixed texts use `**bold**` and `*italic*`; turn the pairs into tags
/// (the input is already escaped).
fn inline_md(text: &str) -> String {
    let mut out = String::new();
    for (i, part) in text.split("**").enumerate() {
        if i % 2 == 1 {
            let _ = write!(out, "<strong>{part}</strong>");
        } else {
            for (j, piece) in part.split('*').enumerate() {
                if j % 2 == 1 {
                    let _ = write!(out, "<em>{piece}</em>");
                } else {
                    out.push_str(piece);
                }
            }
        }
    }
    out
}
