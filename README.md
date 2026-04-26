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
mdlens scout docs/ "Which config option controls authentication timeout?" --max-tokens 1400
```

`scout` is meant to replace the common agent pattern of `find`, `rg`, `cat`, and
large raw file reads when the target corpus is Markdown. It works over arbitrary
English Markdown collections: READMEs, runbooks, model cards, generated docs,
experiment logs, tables, stale notes, and multi-file policy docs.

## Agent Integration

For `AGENTS.md`, `CLAUDE.md`, plugin docs, system prompts, or tool routers, add:

```text
When answering questions over Markdown files or documentation directories, consider using `mdlens`.
If `mdlens` is available and you need command guidance, run `mdlens --help` once.
For question answering over a Markdown directory, prefer:
  mdlens scout <dir> "<question>" --max-tokens 1400
Answer from `[highlights]` and `[evidence]` when sufficient.
If exactly one detail is missing, use the section map from `[files]`:
  mdlens read <file> --id <section-id> --max-tokens 1200
Avoid reading whole Markdown files unless scout clearly found the wrong file.
Preserve exact short source terms such as flags, IDs, option names, metrics, table values, and policy phrases.
```

Detailed guidance lives in `mdlens --help` and `mdlens scout --help`, so harness
prompts can stay short while future agents can still discover the workflow.

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

## Evals

These evals are mostly Markdown search and question-answering workflows, not a
claim about general coding-agent performance. They measure whether agents answer
from documentation with fewer irrelevant reads, fewer calls, lower cost, and
better recall.

![Final Markdown QA eval results](docs/eval_results.svg)

Public eval notes and locked question sets live in [`evals/`](evals/). Corpora
and raw model outputs are omitted from the public repo; the questions and
methodology are included so readers can inspect the task shapes.

The final clean full-corpus runs:

| harness/model | baseline | mdlens | delta |
|---|---:|---:|---:|
| Pi + GPT-5.4 | 21/27, $3.2362 | 25/27, $0.6972 | +4 success, -78.5% cost |
| opencode + GPT-5.4 | 24/27, $2.4994 | 25/27, $0.9525 | +1 success, -61.9% cost |
| opencode + Sonnet 4.6 | 17/27, $2.1692 | 24/27, $0.9136 | +7 success, -57.9% cost |

Other eval families:

- `messy_markdown_v1`: 500 carefully curated synthetic Markdown files with
  malformed formatting, stale/current contradictions, copied distractors,
  multi-needle tables, and cross-file policy/rationale questions.
- `scicat_markdown_v1`: a scientific README/model-card proxy seeded from
  published SciCat research metadata, with Hugging Face and GitHub scientific
  Markdown fallback material.
- `codebase_markdown_v1`: repository-doc navigation over real project docs,
  runbooks, design notes, and experiment reports.

The planned next step, if the project gets traction, is a small mock-workflow
eval where each task combines Markdown analysis, a code edit, and JSON/data
inspection in a fresh branch.

## Installation

Requires Rust 1.70+.

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
