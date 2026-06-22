# mdlens

[![CI](https://github.com/Dreeseaw/mdlens/actions/workflows/ci.yml/badge.svg)](https://github.com/Dreeseaw/mdlens/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/mdlens.svg)](https://crates.io/crates/mdlens)
[![docs.rs](https://docs.rs/mdlens/badge.svg)](https://docs.rs/mdlens)

`mdlens` is a Markdown retrieval CLI for AI agents.

The headline command is `mdlens scout`: give it a directory and a natural-language
question, and it returns a bounded evidence pack with likely files, section IDs,
highlights, table/context snippets, and enough structure for one precise follow-up
read.

```bash
mdlens scout docs/ "Which config option controls authentication timeout?" --max-tokens 1000
```

`scout` is meant to replace the common agent pattern of `find`, `rg`, `cat`, and
large raw file reads when the target corpus is Markdown. It works over arbitrary
English Markdown collections: READMEs, runbooks, model cards, generated docs,
experiment logs, tables, stale notes, and multi-file policy docs.

## Agent Integration

Run `mdlens init` to wire mdlens guidance into your AI coding harness. It writes a
small, idempotent instruction block into the files your tools already read
(`CLAUDE.md`, `AGENTS.md`, etc.), so you don't paste anything by hand.

```bash
mdlens init                       # project files in the current dir (CLAUDE.md + AGENTS.md)
mdlens init -g                    # user-level config (e.g. ~/.claude/CLAUDE.md)
mdlens init --gemini --cursor     # pick specific harnesses
mdlens init --dry-run             # preview without writing
```

Re-running `init` updates the block in place (it is managed by mdlens, so keep
any of your own edits outside the mdlens markers). Detailed guidance lives in
`mdlens --help` and `mdlens scout --help`, so harness prompts stay short while
future agents can still discover the workflow.

## Commands

`scout` should be the first call for natural-language QA, multi-file comparison,
stale/current checks, table lookup, and policy/rationale questions.

The other commands are digging tools:

| Command | Use when |
|---|---|
| `read` | `scout` found the right file and one exact section is missing |
| `search` | you need a second independent lexical query |
| `tree` | you only need Markdown structure and section IDs |
| `pack` | you intentionally want selected sections under a hard token budget |
| `sections` | you already have `rg -nH` hits and want section-aware excerpts |
| `stats` | you need file-level size/token estimates |

Examples:

```bash
mdlens read docs/guide.md --id 1.2 --max-tokens 1200
mdlens search docs/ "rate limit"
mdlens tree docs/
rg -nH "rate limit" docs/ | mdlens sections --preview 3 --max-sections 8
```

All commands support `--json` for machine-readable output.
For `scout`, JSON includes `schema_version`, query expansions, selected
candidates, and the same rendered evidence pack in `rendered_text`.

## Token savings (`mdlens gain`)

`scout` and `read` append a one-line usage record (baseline file tokens vs.
tokens returned) to `~/.local/share/mdlens/history.jsonl`, and `mdlens gain`
sums it into a savings report (`--json` for machine output). This is the one
stateful feature; disable it with `MDLENS_NO_GAIN=1`.

The history is plain append-only text. Clear it with `mdlens gain --reset --yes`,
or truncate it directly, e.g. `tail -n 1000 ~/.local/share/mdlens/history.jsonl | sponge ~/.local/share/mdlens/history.jsonl`.

## Evals

This is a Markdown QA/retrieval benchmark, not a claim about broad coding-agent
performance. It measures whether agents answer from documentation with fewer
irrelevant reads, fewer tool calls, and better recall.

![mdlens real-docs eval matrix](docs/eval_matrix.svg)

The v0.1.3 eval runs **28 hard, low-lexical-overlap questions** (needle,
multi-hop, abstention) over **673 real documentation files** from six
open-source projects (FastAPI, DuckDB, Polars, Pydantic, uv, TRL), comparing
plain shell retrieval (`rg`/`cat`) against the `mdlens scout` workflow. To test
generalization it spans **three harnesses and seven models**: Claude Code
(Opus 4.8, Sonnet 4.6), Codex (GPT-5.4, GPT-5.4-mini), and three open-source
models run through Pi on OpenRouter (Kimi K2.7, GLM 5.2, DeepSeek V4 Flash).

Across all seven models, mdlens:

- raises answer quality: needle **75% -> 86%**, multi-hop **51% -> 66%** [1]
- roughly halves tool calls (avg **7.8 -> 4.9** per question)
- lowers cost on every harness that reports it, with the largest cuts on
  open-source models where prompt caching does not hide them (Kimi K2.7 **-34%**,
  GLM 5.2 **-33%**, DeepSeek V4 Flash **-41%**), and cuts fresh input tokens
  sharply (Codex GPT-5.4 -53%, DeepSeek V4 Flash -89%)

Per model (baseline -> mdlens):

| harness / model | pass (of 28) | tool calls | cost / fresh input tokens |
|---|---:|---:|---:|
| Claude Code / Opus 4.8 | 19 -> 21 | 4.1 -> 2.2 | $0.158 -> $0.158 |
| Claude Code / Sonnet 4.6 | 18 -> 18 | 6.9 -> 5.0 | $0.142 -> $0.131 (-8%) |
| Codex / GPT-5.4 | 15 -> 20 | 8.5 -> 3.9 | fresh input 31.8k -> 14.9k |
| Codex / GPT-5.4-mini | 11 -> 13 | 10.4 -> 6.7 | fresh input 40.5k -> 30.9k |
| Pi (OSS) / Kimi K2.7 | 18 -> 21 | 9.3 -> 5.7 | $0.061 -> $0.040 (-34%) |
| Pi (OSS) / GLM 5.2 | 17 -> 21 | 5.8 -> 2.9 | $0.023 -> $0.016 (-33%) |
| Pi (OSS) / DeepSeek V4 Flash | 16 -> 18 | 9.7 -> 7.8 | $0.014 -> $0.009 (-41%) |

Cost is cache-aware: Claude and Pi/OpenRouter report dollar cost (cache reads
priced below fresh input); the Codex CLI reports tokens but not dollars, so its
rows show the fresh-input reduction instead.

Reproducibility dataset (corpus, questions, per-source licenses, and run
summaries): [`dreeseaw/mdlens-realdocs-v1`](https://huggingface.co/datasets/dreeseaw/mdlens-realdocs-v1).
Public eval notes and locked question sets also live in [`evals/`](evals/).

[1] Two honest caveats. (a) On Claude, dollar cost moves less than token counts
because prompt caching prices the baseline's repeated file reads at roughly a
tenth of fresh input; the token savings are real, but caching absorbs most of
their dollar value, which is why the clearest dollar signal shows up on the
open-source models (no equivalent masking) and in tool-call counts. (b) mdlens
slightly lowers abstention accuracy (36% -> 33%): a compact evidence pack can
make "the answer is not in the docs" harder to recognize, so models fabricate a
little more on truly unanswerable questions.

## Installation

Requires Rust 1.70+.

With Homebrew:

```bash
brew install Dreeseaw/tap/mdlens
```

Or:

```bash
brew tap Dreeseaw/tap
brew install mdlens
```

With Cargo:

```bash
cargo install mdlens
```

Or from source:

```bash
cargo install --git https://github.com/Dreeseaw/mdlens
```

## Claude Code Plugin

`mdlens` also ships as a Claude Code plugin:

```text
/plugin install mdlens
```

## License

MIT. See [LICENSE](LICENSE).
