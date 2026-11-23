<a id="readme-top"></a>

[![Contributors][contributors-shield]][contributors-url]
[![Forks][forks-shield]][forks-url]
[![Stars][stars-shield]][stars-url]
[![Issues][issues-shield]][issues-url]
[![Apache-2.0 License][license-shield]][license-url]

<br />
<div align="center">
  <h1 align="center">synthetic-customers</h1>

  <p align="center">
    Test a product idea on AI-played customers before you pay for a real survey.
    <br />
    <a href="docs/install.md"><strong>Install guide »</strong></a>
    &middot;
    <a href="https://github.com/edouard38000/synthetic-customers/issues/new">Report a bug</a>
    &middot;
    <a href="https://github.com/edouard38000/synthetic-customers/issues/new">Request a feature</a>
  </p>
</div>

<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
      <ul>
        <li><a href="#what-this-is-and-isnt">What this is, and isn't</a></li>
        <li><a href="#built-with">Built With</a></li>
      </ul>
    </li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#installation">Installation</a></li>
      </ul>
    </li>
    <li><a href="#usage">Usage</a></li>
    <li><a href="#roadmap">Roadmap</a></li>
    <li><a href="#contributing">Contributing</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#contact">Contact</a></li>
    <li><a href="#acknowledgments">Acknowledgments</a></li>
  </ol>
</details>

## About The Project

You describe a product idea and who it is for. The tool draws a set of **Synthetic customers**
(AI-played personas, built from a **Customer mix** of ages, genders, incomes and regions that you
confirm), asks each one, in its own separate AI call, whether they would buy it, and scores
each answer on a 1–5 **Purchase intent** scale by meaning. It then writes a one-page report: a
ranking of your **Concepts**, who wants them (by age and income), the main reasons to buy and
objections in the customers' own words, and the limits of what you just learned.

The scoring is *semantic similarity rating* (SSR), from the paper
[*LLMs Reproduce Human Purchase Intent via Semantic Similarity Elicitation of Likert Ratings*](https://arxiv.org/abs/2510.08338)
(PyMC Labs and Colgate-Palmolive). Instead of asking a model for a number, which gives flat,
unrealistic ratings, the customer answers in free text and the answer is compared by meaning
with reference statements for each rating.

The project is two parts that work together:

- a **command-line tool** (`synthetic-customers`, written in Rust) that runs the survey and
  writes the report;
- a **skill** for Claude Code and Codex (`skills/synthetic-customers/`) that lets you do all of
  it in plain words: describe the idea, confirm the Customer mix, and read the report back.

### What this is, and isn't

- **It is a way to compare ideas early**: two versions of a product, two prices, two names, two
  wordings. The score means most as a comparison between Concepts, not on its own.
- **It is not a forecast.** Purchase intent is what the AI-played customers say they would do.
  Stated intent is not sales, and the report is not a market size, a validation or a guarantee.
- **It is not a replacement for research with real people.** The paper itself says so. Use it to
  decide what is worth testing with real customers.
- **It works for everyday products the AI knows well** (snacks, toothpaste, household goods). For
  an idea unlike anything in the model's training data, such as a new category or a niche
  technical product, the customers are guessing and the result means little.
- **Gender and region are not reported.** They shape who the Synthetic customers are, but the
  paper found the method doesn't reproduce real differences by gender or region, so the report
  breaks results down by age and income only.
- **Cowork is not tested yet.** The skill is written for Claude Code and Codex.

### Built With

- [Rust](https://www.rust-lang.org/) with [clap](https://github.com/clap-rs/clap) for the command line
- [tract](https://github.com/sonos/tract), a pure-Rust ONNX runtime, with Hugging Face
  [tokenizers](https://github.com/huggingface/tokenizers) and
  [hf-hub](https://github.com/huggingface/hf-hub), to run the local Scorer
- [bge-small-en-v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5) (BAAI, MIT licence), the
  local Scorer's embedding model
- `claude -p` (Claude Code) or `codex exec` (Codex) to play the customers

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Getting Started

### Prerequisites

- **Claude Code or Codex**, installed and logged in. Every Synthetic customer is one call made
  through your own Claude or Codex subscription, and counts against its usage limit.
- About 130 MB of disk for the local Scorer's model, downloaded once.
- **No API key.** The default Scorer runs on your computer, needs no account or key, and works
  offline once its model is downloaded. Two optional hosted Scorers (OpenAI, Voyage AI) use your
  own key, set as an environment variable; see [docs/scorer.md](docs/scorer.md).
- Rust is only needed if you build from source.

### Installation

**1. The command-line tool.** Prebuilt binaries for macOS (Apple Silicon and Intel), Linux x86_64
and Windows x86_64 are published on the
[releases page](https://github.com/edouard38000/synthetic-customers/releases). The install
scripts pick the right one, check its checksum and install it for your user only (no sudo, no
admin).

macOS or Linux (installs to `~/.local/bin`):

```bash
curl -fsSL https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.sh | sh
```

Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.ps1 | iex"
```

Or from source, with Rust installed:

```bash
cargo install --git https://github.com/edouard38000/synthetic-customers
```

More options (pinning a version, the install folder, platforms without a prebuilt binary) are in
[docs/install.md](docs/install.md).

Then download the local Scorer's model once:

```bash
synthetic-customers setup
```

**2. The skill.** Copy the `skills/synthetic-customers/` folder to where your assistant reads
skills:

```bash
git clone https://github.com/edouard38000/synthetic-customers.git
mkdir -p ~/.claude/skills ~/.codex/skills

# Claude Code
cp -R synthetic-customers/skills/synthetic-customers ~/.claude/skills/

# Codex
cp -R synthetic-customers/skills/synthetic-customers ~/.codex/skills/
```

Use `.claude/skills/` inside a project instead to make the skill available only there. The skill
checks for the tool itself and offers to install it if it is missing.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Usage

### With the skill

In Claude Code or Codex, describe the idea in plain words, for example:

> Would parents in the US buy an oat bar for kids at $4.99, or does $3.99 sell better?

The skill then:

1. **Describes the idea.** It writes each Concept as a short product description (it asks only
   for what it can't assume) and recommends at least two Concepts, or one against a known
   baseline.
2. **Confirms the Customer mix.** It proposes ages, genders, incomes and regions with shares for
   your audience, shows the whole plan (Concepts, Customer mix, number of customers, estimated
   time, limits) and **waits for your yes**. Nothing runs before you confirm.
3. **Runs.** It writes the run file and runs the tool, 200 Synthetic customers per Concept by
   default.
4. **Reads the report back**: the ranking (and whether two Concepts are too close to call),
   who wants it by age and income, reasons and objections in the customers' own words, and the
   limits.

### With the command line

The skill writes a run file for you. You can also write one yourself; the fields are listed in
[the run-file reference](skills/synthetic-customers/reference/run-file.md).

```toml
# oat-bar.toml
customers_per_concept = 200
respondent = "claude"   # or "codex"
model = "haiku"         # haiku, sonnet or opus; Claude only
concurrency = 8
scorer = "local"

[[concepts]]
name = "Oat bar"
text = "A soft oat bar for kids, made with oats and honey. Sold in packs of 6 for $4.99."

[[concepts]]
name = "Oat bar, $3.99"
text = "A soft oat bar for kids, made with oats and honey. Sold in packs of 6 for $3.99."

[customer_mix.age]
"18-34" = 0.40
"35-54" = 0.35
"55+" = 0.25

[customer_mix.income]
"low" = 0.30
"middle" = 0.45
"high" = 0.25
```

```bash
synthetic-customers run oat-bar.toml --html
```

This writes `oat-bar-results/` next to the run file, with `report.md`, `report.html` (one
self-contained page with charts), `answers.jsonl` (every answer and its score), `run.toml` and
`run-state.json`. The commands:

| Command | What it does |
|---|---|
| `synthetic-customers setup` | Checks that `claude` or `codex` is installed and downloads the local Scorer's model once. |
| `synthetic-customers run <run file>` | Asks every Synthetic customer, scores each answer and writes the report. `--out <folder>` picks the results folder, `--html` also writes `report.html`, `--resume` continues a stopped run and asks only the customers who haven't answered. |
| `synthetic-customers report <results folder>` | Rebuilds the report from saved answers without asking anyone again. `--rescore` scores the answers again first (with `--scorer` to pick another Scorer), `--html` also writes the page. |

If a run is stopped by a usage limit or the network, the tool keeps every answer it received and
writes a report marked as a short sample; `--resume` picks it up later.

### The limits, as the report states them

Every report ends with a Limits section. In short:

- Stated intent is not sales: real shoppers buy less often than they say they will.
- It is best for everyday consumer products the AI knows well. Niche, technical or very new
  products are where the AI knows least.
- A comparison means more than one score alone. Use the ranking of several Concepts, and treat
  "too close to call" as exactly that.
- The paper's numbers came from other models (GPT-4o and Gemini 2.0 Flash played the customers).
- A local Scorer is a different embedding model from the paper's OpenAI one, and gives different
  numbers.

### How well the local Scorer agrees with people

The local Scorer is checked against a gold set of 53 written answers to "How likely are you to
purchase the product?", each with the 1–5 rating a person would give it. On it, the default
model (bge-small-en-v1.5) reaches a **Spearman rank correlation of 0.923** and a **mean absolute
error of 0.801**, at about 7 ms per answer on a laptop CPU. The ranking is right; the spread is
narrower than a person's, because SSR pulls answers towards the middle of the scale, which is why
the report leads with comparisons. The hosted Scorers have not been measured yet. The model
comparison, the gold set and the test that guards these numbers are in
[docs/scorer.md](docs/scorer.md).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Roadmap

- [x] Command line: `setup`, `run`, `report`, with resume after a stopped run
- [x] Local Scorer that needs no key, checked against a gold set
- [x] OpenAI and Voyage AI Scorers, with keys from environment variables only
- [x] Report with ranking, age and income breakdowns, quotes, limits and an HTML page
- [x] Skill for Claude Code and Codex
- [x] Prebuilt binaries and install scripts for macOS, Linux x86_64 and Windows x86_64
- [ ] Measure the hosted Scorers against the gold set
- [ ] Prebuilt binaries for Linux on ARM and musl Linux
- [ ] Test the skill in Cowork

See the [open issues](https://github.com/edouard38000/synthetic-customers/issues) for the full
list of proposed features and known problems.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Contributing

Contributions are welcome. If you have a suggestion that would make this better, fork the repo
and create a pull request, or open an issue.

1. Fork the project
2. Create your feature branch (`git checkout -b feature/my-feature`)
3. Commit your changes
4. Push to the branch (`git push origin feature/my-feature`)
5. Open a pull request

### Top contributors

<a href="https://github.com/edouard38000/synthetic-customers/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=edouard38000/synthetic-customers" alt="Contributors" />
</a>

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## License

Distributed under the Apache License 2.0. See [`LICENSE`](LICENSE) for more information.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Contact

Edouard — [github.com/edouard38000](https://github.com/edouard38000)

Project link: [https://github.com/edouard38000/synthetic-customers](https://github.com/edouard38000/synthetic-customers)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Acknowledgments

- *LLMs Reproduce Human Purchase Intent via Semantic Similarity Elicitation of Likert Ratings*,
  PyMC Labs and Colgate-Palmolive, [arXiv 2510.08338](https://arxiv.org/abs/2510.08338): the
  method this tool implements.
- [semantic-similarity-rating](https://github.com/pymc-labs/semantic-similarity-rating) by
  PyMC Labs (Apache-2.0): the reference code for semantic similarity rating.
- [BAAI](https://huggingface.co/BAAI/bge-small-en-v1.5) for bge-small-en-v1.5, and
  [tract](https://github.com/sonos/tract) for the ONNX runtime.
- [Best-README-Template](https://github.com/othneildrew/Best-README-Template) by Othneil Drew
  (Unlicense), the structure of this README.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- MARKDOWN LINKS & IMAGES -->
[contributors-shield]: https://img.shields.io/github/contributors/edouard38000/synthetic-customers.svg?style=for-the-badge
[contributors-url]: https://github.com/edouard38000/synthetic-customers/graphs/contributors
[forks-shield]: https://img.shields.io/github/forks/edouard38000/synthetic-customers.svg?style=for-the-badge
[forks-url]: https://github.com/edouard38000/synthetic-customers/network/members
[stars-shield]: https://img.shields.io/github/stars/edouard38000/synthetic-customers.svg?style=for-the-badge
[stars-url]: https://github.com/edouard38000/synthetic-customers/stargazers
[issues-shield]: https://img.shields.io/github/issues/edouard38000/synthetic-customers.svg?style=for-the-badge
[issues-url]: https://github.com/edouard38000/synthetic-customers/issues
[license-shield]: https://img.shields.io/github/license/edouard38000/synthetic-customers.svg?style=for-the-badge
[license-url]: https://github.com/edouard38000/synthetic-customers/blob/main/LICENSE
