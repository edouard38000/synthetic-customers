---
name: synthetic-customers
description: Tests a product idea on AI-played Synthetic customers and reports how much they would want to buy it, using semantic similarity rating (SSR, arXiv 2510.08338). The user describes one or several Concepts and who they are for in plain words; the skill proposes a Customer mix (ages, genders, incomes, regions with shares), waits for the user to confirm it, runs the synthetic-customers command-line tool and reads back the ranked report with its limits. Use when someone wants to know whether people would buy an idea, or which version, price, name or wording people prefer, before paying for a real survey. Works in Claude Code (customers played through `claude -p`) and Codex (through `codex exec`). Needs no API key.
---

# Synthetic customers

Test a product idea on AI-played customers. Each Synthetic customer is a
persona drawn from a Customer mix the user confirms; each one is asked, in its
own separate AI call, whether they would buy the Concept, answers in their own
words, and the answer is scored on the 1–5 Purchase intent scale by meaning
(SSR). The tool writes a one-page report; you read it back in plain words.

Words to use with the user (the report uses them too):

- **Concept**: the idea being tested, as a short text. Several Concepts in one
  run are compared with each other (two versions, two prices, two names).
- **Synthetic customer**: one AI-played persona.
- **Customer mix**: the shares of ages, genders, incomes and regions the
  Synthetic customers are drawn from.
- **Purchase intent**: 1 = would definitely not buy, 5 = would definitely buy.
- **Scorer**: the model that scores the answers. The local Scorer (default)
  runs on the user's computer and needs no key.

## Hard rules

1. **Never run before the user confirms the Customer mix and the run plan.**
   An explicit yes to the plan you showed. Any change → show the new plan and
   ask again.
2. **Never ask for an API key in the chat, and never use one pasted there.**
   The default local Scorer needs none. Hosted Scorers read keys only from
   environment variables the user sets in their own shell. If a user pastes a
   key anyway, don't repeat it, don't write it anywhere, tell them to set it as
   an environment variable themselves and to consider the pasted key exposed.
   Check whether a key is set with `[ -n "$OPENAI_API_KEY" ] && echo set`,
   never by printing it.
3. **Be honest about limits.** Say them before the run (step 3) and after it
   (step 6). Never drop or shorten the report's Limits section when you read it
   back.

## Workflow

Copy this checklist and tick it off as you go:

```
- [ ] 1. Tool installed and Scorer ready
- [ ] 2. Concepts and audience understood
- [ ] 3. Plan shown: Concepts, Customer mix, run size, time, limits
- [ ] 4. User confirmed → run file written
- [ ] 5. Run finished (or stopped and reported)
- [ ] 6. Report read back in plain words, with the limits
```

### 1. Install the tool and get the Scorer ready

Check with `synthetic-customers --version`. If it is missing, install it:

- macOS or Linux:
  `curl -fsSL https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.sh | sh`
  (installs to `~/.local/bin`, no sudo).
- Windows (PowerShell):
  `powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.ps1 | iex"`

If the script prints that the folder is not on the PATH, call the binary by its
full path for this session (`~/.local/bin/synthetic-customers`) and tell the
user the line to add to their shell profile. If the script fails with
`could not download ... (is there a published release latest?)` or says the
platform has no prebuilt binary, and `cargo` is installed, build it instead:
`cargo install --git https://github.com/edouard38000/synthetic-customers`
(a few minutes). With neither, stop and tell the user what failed.

Then run `synthetic-customers setup` once. It checks that `claude` or `codex`
is on the PATH and downloads the local Scorer's model (about 130 MB, once;
later runs score offline). It ends with `Ready: runs with scorer = "local" now
score offline.`

### 2. Understand the Concepts and the audience

From the user's words, get:

- **The Concepts.** For each: a short unique `name` and a `text` a customer
  would read: what it is, what it does, the price if known, how it's sold. Two
  to five sentences, written like a product description, not a pitch deck. Keep
  the user's facts; don't invent features or prices. If they gave a price,
  keep it in the text.
- **Who it is for**, in their words ("parents in the US with young kids").

Ask only what you can't reasonably assume, in one message. Recommend testing
**at least two Concepts, or one Concept against a known baseline** (an
existing product the customer would recognise, written as a Concept too):
the scores mean most as a comparison. If the user wants one Concept only, run
it, but say the score is hard to read on its own.

An image can be attached to a Concept with `image = "file.png"` (a path
relative to the run file) if the user has one.

### 3. Propose the plan and wait

Draft a Customer mix for their audience, then show the whole plan in one
message and ask for a yes or changes. Use this layout:

```
Concepts
  A. <name>: <text>
  B. <name>: <text>

Customer mix
  age:    18-34 40% · 35-54 35% · 55+ 25%
  gender: woman 50% · man 50%
  income: low 30% · middle 45% · high 25%
  region: Northeast 20% · Midwest 20% · South 35% · West 25%

Run: 200 Synthetic customers per Concept (400 in all), played by
Claude Haiku, 8 at a time, scored by the local Scorer (no key, nothing leaves
your computer). Estimated time: about 7 minutes.

Before you confirm: ...limits, see below...
```

**Customer mix rules** (the tool rejects anything else):

- Use the attribute names `age`, `gender`, `income`, `region`. The report breaks
  down Purchase intent by attributes named with *age* or *income*; a different
  name for those two loses the breakdown.
- Values are short labels a person would recognise ("18-34", "middle",
  "Midwest"); fit them to the audience and country the user named. Leave out
  an attribute only if the user wants it out.
- Each attribute's shares are fractions that add up to 1 (0.25, not 25).
  Show them as percentages to the user, write fractions in the file.
- Base shares on what you know of the real audience and say they are your
  estimate. The user can change them in words ("more students, fewer
  retirees"): adjust, re-check that each attribute still adds up to 1, and
  show the full mix again.

**Run size and model:**

- 200 Synthetic customers per Concept by default. 150–400 is the recommended
  range (the sample sizes of the paper's real surveys); fewer is quicker but
  noisier, and the tool warns outside that range.
- In **Claude Code**, customers are played by `claude -p` with Haiku by
  default; Sonnet or Opus if the user asks (slower, uses more of their plan).
- In **Codex**, customers are played by `codex exec` with the user's default
  Codex model; there is no model choice in the run file.
- 8 customers at a time by default (`concurrency`, 1–64). Lower protects the
  user's usage limit; higher is faster.
- Every customer is one call on the user's own subscription and counts
  against its usage limit.

**Time estimate** (the tool only prints it once the run starts, so work it out
here): rounds = ceil(customers per Concept × Concepts ÷ concurrency);
time ≈ rounds × seconds per answer, with 8 s for Haiku, 12 s for Sonnet, 20 s
for Opus and 20 s for Codex. 2 Concepts × 200 with Haiku at 8 at a time:
ceil(400 ÷ 8) × 8 s = 400 s, about 7 minutes.

**Limits to state before the user confirms**, in your own words, short:

- Results are a **comparison between Concepts, not a forecast** of sales or
  market size. Stated intent is not buying.
- The method works for things like **everyday products the AI knows well**.
  For an idea unlike anything in the model's training data (a new category, a
  niche technical product), the customers are guessing and the result means
  little. If the idea looks like that, say so plainly before the run.
- Gender and region shape who the customers are, but the report shows no
  results by gender or region: the paper found the method doesn't reproduce
  those differences.

Then stop and wait for the user's answer.

### 4. Write the run file

Only after the user's yes. Write it to a folder named `synthetic-customers` in
the current directory, with a short name for the idea, e.g.
`synthetic-customers/oat-bar.toml`:

```toml
customers_per_concept = 200
respondent = "claude"        # "codex" when you are running in Codex
model = "haiku"              # haiku, sonnet or opus; leave this line out with codex
concurrency = 8
scorer = "local"

[[concepts]]
name = "Oat bar"
text = "A soft oat bar with ... Sold in packs of 6 for $4.99."

[[concepts]]
name = "Oat bar, $3.99"
text = "..."

[customer_mix.age]
"18-34" = 0.40
"35-54" = 0.35
"55+" = 0.25

[customer_mix.gender]
"woman" = 0.5
"man" = 0.5
```

Use TOML basic strings and escape `"` inside a `text` as `\"`, or use
`'''...'''` for text with quotes. The full list of fields and the tool's error
messages are in [reference/run-file.md](reference/run-file.md).

### 5. Run it

```bash
synthetic-customers run synthetic-customers/oat-bar.toml --html
```

- Results go to `synthetic-customers/oat-bar-results/` (the run file's name
  plus `-results`): `report.md`, `report.html` (with `--html`),
  `answers.jsonl` (every answer and its score), `run.toml` (a copy of the run
  file) and `run-state.json`.
- The tool prints `Asking N of N Synthetic customers, 8 at a time: about …`
  and then progress lines `Answered 40/400 (10%), about 6 minutes left` on
  stderr. Relay progress to the user now and then.
- A real run takes minutes. Run the command in the background, or with a
  timeout longer than your estimate, so your own tool call doesn't cut it
  off. If it does get cut off, nothing is lost: see `--resume` below.
- Any warning the tool prints (`warning: …`) goes to the user.
- **In Codex**, `setup` and `run` need the network (the model download, and
  every customer is its own `codex exec` call). If Codex's sandbox blocks
  the network, ask the user to approve running these commands outside the
  sandbox.
- If the run stops with **0** customers answered, nothing was learned: don't
  present that report. Tell the user the cause from the end of the error
  (for example the runner is not logged in: `claude` or `codex login`) and
  run again once it's fixed.

**If the run stops early** (usage limit reached, network down), the tool exits
with an error that starts `the run stopped with N of M Synthetic customers
answered`. It still wrote `report.md`, marked **Short sample**, from the
answers it has. Read that report back as usual, say clearly that it is a
smaller sample, and offer to continue later with:

```bash
synthetic-customers run synthetic-customers/oat-bar.toml --resume --html
```

`--resume` asks only the customers who haven't answered; nobody is asked
twice. Don't change the run file between a run and its resume: the tool
refuses with `cannot resume: the run file has changed since the run in …
started`; undo the change, or start a fresh run.

Running again into a folder that already has answers, without `--resume`,
stops with `… already holds answers from an earlier run`. Use `--resume` to
continue it; for a fresh run, save the run file under a new name (or pass
`--out <new folder>`).

Other errors and what to do about them: [reference/troubleshooting.md](reference/troubleshooting.md).

### 6. Read the report back

Read `report.md` (sections: Ranking, Purchase intent per Concept, Who wants
it: age and income, Reasons and objections in their own words, Limits, Run
details, Credits) and tell the user, in plain words and in this order:

1. **The ranking** (several Concepts): which Concept leads, by how much, and
   whether the tool marked any pair **too close to call**. If it did, say the
   run can't tell them apart; don't read an order into it.
2. **Each Concept's average Purchase intent** (1–5) and its spread, in a
   sentence each. Averages bunch towards the middle of the scale with this
   method, so a 3.4 vs 3.1 gap can matter while "3.4 out of 5" on its own
   means little.
3. **Who wants it most** by age and by income, only where the report shows a
   difference. Groups marked `few customers` are hints, not findings; say so.
   The report leaves gender and region out on purpose; don't fill them in
   from `answers.jsonl` either.
4. **The main reasons to buy and the main objections**, quoting the customers'
   own words from the report. These are often the most useful part: what to
   fix or say differently.
5. **The limits: the report's whole Limits section**, all five points and the
   closing line, in plain words. Never drop it, even if the user only asks for
   the winner.
6. The run details in one line: model, Scorer, customers who answered, date.

Then give the paths of `report.md` (and `report.html`, which has charts and
can be shared). Offer next steps that fit: test a reworded or re-priced
Concept against the winner, try another Customer mix, or take the best idea
to real customers.

Don't add confidence the report doesn't have. Don't call it a market
forecast, a validation or a guarantee.

## Scorers and keys

The local Scorer is the default and the right choice for almost everyone. Two
hosted Scorers exist for users who already have a key and want them:

- `scorer = "openai"` (OpenAI `text-embedding-3-small`, the paper's
  embedding model): needs `OPENAI_API_KEY`. A ChatGPT subscription is **not**
  an OpenAI API key; the API is a separate pay-per-use account.
- `scorer = "voyage"` (Voyage AI, free tier): needs `VOYAGE_API_KEY`.

Both send every customer's answer to that provider. Only switch when the user
asks. Tell them to set the variable in the shell that will run the tool, e.g.
`export OPENAI_API_KEY=...` typed themselves in their terminal (not in this
chat), then confirm it's set with the `[ -n ... ]` check from the hard rules.
If you can't see their shell's environment, they can run the `run` command
themselves. A missing key stops the run before any customer is asked.

To score saved answers again with another Scorer, without asking anyone again:

```bash
synthetic-customers report synthetic-customers/oat-bar-results --rescore --scorer openai --html
```

`report <folder>` alone rebuilds the report from saved answers (add `--html`
for the page).
