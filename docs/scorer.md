# The local Scorer

The local Scorer is the default Scorer: a small open embedding model that runs
on the CPU, needs no account or key, and works offline once downloaded.

```bash
synthetic-customers setup
```

`setup` warns when neither `claude` nor `codex` is installed, downloads the model once
from Hugging Face into the user cache folder, and checks that it runs. Later
runs only read the cache and never touch the network; a run with
`scorer = "local"` and no downloaded model stops before asking any Synthetic
customer and says to run `setup`.

- Cache folder: `synthetic-customers/models` in the user cache folder
  (`~/Library/Caches` on macOS, `~/.cache` on Linux, `%LOCALAPPDATA%` on
  Windows). Set `SYNTHETIC_CUSTOMERS_CACHE_DIR` to use another folder.
- The model is pinned to one Hugging Face commit, so everyone scores with the
  same weights.

## Model choice

The local Scorer uses **bge-small-en-v1.5** (BAAI, MIT licence; the ONNX
export from `Xenova/bge-small-en-v1.5`, commit `ea104da`, 134 MB, CLS
pooling). It agreed best with the gold set of every candidate that is fast on
a laptop: **Spearman 0.923, MAE 0.801** on 53 answers, at 6.7 ms per answer,
so 200 Synthetic customers score in about 1.5 seconds.

Measured on 2026-11-23, nine candidates under about 400 MB (all-mpnet-base-v2
in full precision, 436 MB, is included as a reference just over the limit):

| Model | Size (MB) | Spearman | MAE | Load (s) | ms per answer |
|---|---:|---:|---:|---:|---:|
| **bge-small-en-v1.5** | 134 | **0.923** | 0.801 | 0.2 | 6.7 |
| bge-base-en-v1.5 (int8) | 111 | 0.927 | 0.752 | 0.4 | 32.9 |
| gte-small | 134 | 0.903 | 0.868 | 0.2 | 6.6 |
| all-mpnet-base-v2 (int8) | 111 | 0.723 | 0.950 | 0.5 | 38.5 |
| all-mpnet-base-v2 | 436 | 0.717 | 0.956 | 1.2 | 21.0 |
| all-MiniLM-L12-v2 | 134 | 0.563 | 1.068 | 0.5 | 6.7 |
| mxbai-embed-xsmall-v1 | 97 | 0.553 | 1.097 | 0.3 | 3.4 |
| all-MiniLM-L6-v2 | 91 | 0.529 | 1.098 | 0.1 | 3.5 |
| snowflake-arctic-embed-s | 134 | 0.417 | 1.138 | 0.2 | 6.6 |

bge-base-en-v1.5 (int8) agrees a hair better (0.927) but is five times
slower, a difference well inside what 53 answers can tell apart. all-MiniLM-L6-v2,
the default of PyMC Labs' Python package, ranks the gold set poorly with these
Anchor sets.

The MAE is high next to the Spearman because SSR with ε = 0 pulls every
answer towards the middle of the scale: on the gold set, clear "1" answers
score about 2.1–2.4 and clear "5" answers about 3.6–4.0. The order is right;
the spread is narrower than a person's. Comparisons between Concepts, which
the report leads with, depend on the order.

### How the candidates were compared

`cargo run --release --example compare_scorers` downloads each candidate,
scores the gold set with SSR (the six Anchor sets below, ε = 0) and prints the
table above. Spearman is the rank correlation between each answer's expected
Purchase intent and the human rating (1 is perfect); MAE is the mean absolute
gap on the 1–5 scale. Speed is the time to embed one answer in batches of 16,
on an Apple M-series laptop CPU in a release build. Add `--clean` to delete
every model but the default from the cache afterwards.

## ONNX runtime

The model runs through [tract](https://github.com/sonos/tract) (MIT or
Apache-2.0), a pure-Rust ONNX runtime, with Hugging Face's `tokenizers`
(pure-Rust regex backend) and `hf-hub` for the download. No native library is
linked, so the release targets build unchanged: macOS on Apple Silicon and
Intel, Linux x86_64 and Windows x86_64 with the C runtime linked statically
(`+crt-static`).

ONNX Runtime through `ort`/fastembed was tried first and dropped: `ort`
2.0.0-rc.15 (ONNX Runtime 1.31) ships no prebuilt library for Intel macOS
(`x86_64-apple-darwin`), which the release builds, and its only Windows
libraries are DirectML builds with no static-C-runtime variant, so the
`+crt-static` Windows build was at risk too.

## Anchor sets

PyMC Labs' code ([semantic-similarity-rating](https://github.com/pymc-labs/semantic-similarity-rating),
commit `86dcd25`) contains only generic agree/disagree example statements, not
the purchase-intent statements of the paper, and the paper (arXiv 2510.08338,
App. C.1) describes its six sets without listing them. So this repo's six sets
of five (`src/anchors.rs`) were written for it, following App. C.1: short,
generic and product-independent; the lowest statement says a purchase is
unlikely, the middle one is indifferent or unsure, the highest says the
customer would buy, and the two between sit halfway. Each set words purchase
intent differently (likelihood, interest, fit, chance, trial, spoken). They
were written before the first measurement and not tuned on the gold set.

## Gold set

`tests/data/gold_set.json` holds 53 written answers to "How likely are you
to purchase the product?", each about a different product, with the 1–5
Purchase intent a person would give it: ten rated 1, eleven each rated 2, 3 and 4, and ten rated 5. They include
hedged and mixed answers ("Part of me wants it, part of me thinks it's a
gimmick", "It's pricier than what I usually pay, but I think it's worth it").
The answers and ratings were written for this repo.

## Agreement test

`tests/gold_agreement.rs` scores the gold set with the local Scorer and fails
below the thresholds below. It runs when the model is cached (after `setup`)
or when `CI` is set (the victus runner, where it downloads the model once);
otherwise it is skipped with a message, so `cargo test` needs no network.
`SYNTHETIC_CUSTOMERS_SKIP_GOLD=1` skips it always (`0`, `false` or empty
count as unset, for both variables).

| Measure | First measurement (2026-11-23) | Test fails when |
|---|---:|---|
| Spearman | 0.923 | below 0.88 |
| MAE | 0.801 | above 0.90 |

The margins leave room for small floating-point differences between machines
while catching a changed model, broken pooling or reworded Anchor sets. The
same numbers came out of a release and a debug build.
