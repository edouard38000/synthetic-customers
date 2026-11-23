# Troubleshooting

What the tool's messages mean and what to do. Errors start with `error:` and
the command exits with a non-zero code.

## Contents

- Install
- Before the run starts
- During the run
- Resuming and re-running
- Rebuilding the report

## Install

| Message | What to do |
|---|---|
| `install.sh: error: could not download … (is there a published release latest?)` | No prebuilt binary to download. If `cargo` is installed: `cargo install --git https://github.com/edouard38000/synthetic-customers`. Otherwise tell the user the install failed and why. |
| The script prints how to add a folder to the PATH | Use the full path to the binary for this session; give the user that line for their shell profile. |

Linux on ARM and musl Linux (Alpine) have no prebuilt binary: use the
`cargo install` line above.

## Before the run starts

None of these ask any Synthetic customer, so nothing is spent.

| Message (start) | What to do |
|---|---|
| `the local Scorer's model (bge-small-en-v1.5) is not downloaded yet: run \`synthetic-customers setup\` once …` | Run `synthetic-customers setup` (needs the network once), then the same `run` command. |
| `` `claude` is not installed or not on the PATH `` | You are probably in Codex: propose `respondent = "codex"` (without `model`), show the changed plan and wait for a yes before running. If neither runner exists, the user needs Claude Code or Codex installed. |
| `` `codex` is not installed or not on the PATH `` | The reverse: propose `respondent = "claude"`, show the changed plan and wait for a yes. |
| `scorer = "openai" needs an OpenAI API key in the OPENAI_API_KEY environment variable, and it is not set …` | Either propose `scorer = "local"` (no key) and wait for a yes, or the user sets the variable themselves in the shell that runs the tool. Never ask for the key in the chat. |
| `scorer = "voyage" needs a Voyage AI API key in the VOYAGE_API_KEY environment variable …` | Same as above. |
| `invalid run file …`, or `the image of Concept … is not a file` | See the error table in [run-file.md](run-file.md). |
| `warning: neither \`claude\` nor \`codex\` is on the PATH` (from `setup`) | Real runs need one of them. |

## During the run

The tool prints `Asking N of M Synthetic customers, C at a time: about …`,
then progress lines `Answered n/M (p%), about … left` on stderr.

If a customer's call fails (usage limit reached, network down, a call that
gives no answer after its time limit), the run stops asking new customers
and exits with:

```
error: the run stopped with N of M Synthetic customers answered; their answers are kept and <folder>/report.md is marked as a short sample. Run the same command with --resume to score the saved answers and ask only the missing customers: <cause>
```

The `<cause>` at the end says why: for example `claude -p failed: exited
with …` followed by the runner's own message (a usage-limit notice says so).
The report is still written and starts with a **Short sample** note. Read it
back to the user as a smaller sample, tell them the cause, and offer
`--resume` once the limit resets.

If the run stopped with **0** customers answered, the report has nothing to
show. The cause tells you which:

| Cause contains | What to do |
|---|---|
| `401 Unauthorized` or a login message, from `codex exec` | Codex is not logged in: the user runs `codex login` themselves, then run again. |
| A login or authentication message from `claude -p` | Claude Code is not logged in: the user runs `claude` once and logs in, then run again. |
| Network or connection errors, in Codex | Codex's sandbox may be blocking the network: ask the user to approve running the command outside the sandbox. |

If your own command call was cut off (a tool timeout), the answers given so
far are saved line by line. Run the same command with `--resume`.

## Resuming and re-running

| Message (start) | What to do |
|---|---|
| `… already holds answers from an earlier run: pass --resume to continue that run, or choose another --out folder` | Add `--resume` to continue it. For a fresh run (which asks every customer again: check with the user first), use a new run file name or `--out <new folder>`. |
| `cannot resume: the run file has changed since the run in … started (its copy is …/run.toml); undo the change, or choose another --out folder` | Put the run file back as it was (the copy is in the results folder), or start a fresh run in another folder. |
| `All N Synthetic customers have already answered.` | Not an error: the run was complete. The report is rebuilt. |
| `Scoring N saved answers that were not scored yet.` | Not an error: a resume first scores answers the Scorer missed last time. |

## Rebuilding the report

`synthetic-customers report <results folder>` rebuilds `report.md` from the
saved answers without asking anyone (`--html` for the page too).

`synthetic-customers report <results folder> --rescore --scorer local`
scores every saved answer again (with `local`, `openai` or `voyage`) and saves
the new scores. Useful after switching Scorer; the key rules above apply.
