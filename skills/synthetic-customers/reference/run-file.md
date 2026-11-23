# The run file

The run file is a small TOML file holding what the user confirmed. The tool
saves a copy of it as `run.toml` in the results folder.

## Contents

- Fields
- Concepts
- Customer mix
- Errors the tool gives for a bad run file

## Fields

All top-level fields are optional; unknown fields are an error.

| Field | Values | Default |
|---|---|---|
| `customers_per_concept` | 1–10000; recommended 150–400 (a warning outside) | 200 |
| `respondent` | `"claude"` (`claude -p`) or `"codex"` (`codex exec`) | `"claude"` |
| `model` | `"haiku"`, `"sonnet"`, `"opus"`; Claude only, ignored (with a warning) for Codex | `"haiku"` |
| `concurrency` | customers asked at the same time, 1–64 | 8 |
| `scorer` | `"local"`, `"openai"` (needs `OPENAI_API_KEY`), `"voyage"` (needs `VOYAGE_API_KEY`) | `"local"` |
| `seed` | a whole number, to draw the same Synthetic customers again | random |

## Concepts

One `[[concepts]]` table per Concept, at least one:

```toml
[[concepts]]
name = "Oat bar"                 # required, unique
text = "A soft oat bar ..."      # required: what the customer reads
image = "oat-bar.png"            # optional, relative to the run file
```

For text containing double quotes, either escape them (`\"`) or use a
literal multi-line string:

```toml
text = '''A "soft-baked" oat bar.
Sold in packs of 6 for $4.99.'''
```

## Customer mix

One `[customer_mix.<attribute>]` table per attribute, each value with its
share. Shares are fractions above 0 that add up to 1 per attribute. Round
so they do: thirds as 0.34, 0.33, 0.33 (0.33 × 3 is rejected).

```toml
[customer_mix.age]
"18-34" = 0.40
"35-54" = 0.35
"55+" = 0.25

[customer_mix.income]
"low" = 0.30
"middle" = 0.45
"high" = 0.25
```

Each Synthetic customer gets one value per attribute. Counts per value match
the shares exactly (rounded); the values are combined at random.

The report breaks Purchase intent down only by attributes whose name contains
the word `age` (or `ages`, `agegroup`) or `income` (or `incomes`). Every other
attribute (gender, region, anything else) shapes the customers but is listed
in the report as left out on purpose.

## Errors the tool gives for a bad run file

Every one stops the run before any customer is asked. Fix the file and run
the same command again.

| Message (start) | Fix |
|---|---|
| `Customer mix \`age\`: shares add up to 100.000, they must add up to 1 (write 0.25, not 25)` | Write fractions, and make each attribute add up to 1. |
| `TOML parse error at line …` | Usually an unescaped `"` inside a `text`, an unknown field, or a value the field doesn't accept (`unknown variant …, expected one of haiku, sonnet, opus`). The message points at the line. |
| `no Concepts: add at least one [[concepts]] table …` | Add a Concept. |
| `two Concepts are named \`…\`: names must be unique` | Rename one. |
| `empty Customer mix: …` | Add at least one `[customer_mix.<attribute>]` table. |
| `the image of Concept \`…\` is not a file: …` | Fix the path; it is relative to the run file. |
| `` `customers_per_concept` must be at least 1 `` / `at most 10000` | Pick a size in range. |
| `` `concurrency` must be at least 1 `` / `at most 64` | Pick a value in range. |

Warnings (the run goes ahead; tell the user):

- `customers_per_concept = N is outside the recommended 150–400 (the sample sizes of the paper's real surveys)`
- `` `model` is ignored with respondent = "codex": codex exec uses your default Codex model ``
