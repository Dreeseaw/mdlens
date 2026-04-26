# mdlens

[![CI](https://github.com/Dreeseaw/mdlens/actions/workflows/ci.yml/badge.svg)](https://github.com/Dreeseaw/mdlens/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/mdlens.svg)](https://crates.io/crates/mdlens)
[![docs.rs](https://docs.rs/mdlens/badge.svg)](https://docs.rs/mdlens)

Token-efficient Markdown CLI for AI agents. Navigate, search, and pack docs into bounded context windows without reading files you don't need.

`mdlens scout` is the recommended first call for agent question answering over arbitrary Markdown collections. It searches headings, paths, section bodies, parent context, and table rows, then returns a compact evidence pack with file maps, highlights, and bounded excerpts.

## Why

When an AI agent needs to check a doc, the naive approach is to read the whole file. That works fine for a 50-line README. It falls apart fast with real documentation: multi-file references, long guides, rolling experiment logs. You burn context budget on sections that have nothing to do with the task at hand.

`mdlens` gives agents a structured view of Markdown with section hierarchy, token estimates, and targeted extraction. Read only what matters.

## Demo

```
$ mdlens tree docs/
# (directory mode: depth ≤1 by default; use --max-depth N for more)

docs/guide.md  lines=312  tokens=1842

1 Guide lines 1-312  tokens=1842
  1.1 Installation lines 5-38  tokens=203
    1.1.1 Prerequisites lines 7-18  tokens=64
    1.1.2 Quick start lines 19-38  tokens=98
  1.2 Configuration lines 39-110  tokens=487
    1.2.1 Environment variables lines 42-78  tokens=201
    1.2.2 Config file lines 79-110  tokens=186
  1.3 API Reference lines 111-280  tokens=982
  1.4 Changelog lines 281-312  tokens=187
```

```
$ mdlens read docs/guide.md --id 1.1.2

Guide > Installation > Quick start
id=1.1.2  lines=19-38  tokens=98

### Quick start
...
```

```
$ mdlens search docs/ "authentication"

docs/guide.md > API Reference > Auth
id=1.3.1  lines=115-142  tokens=163  matches=4

115: ## Authentication
...
```

```
$ mdlens scout docs/ "Which configuration option controls authentication timeout?"
[scout] question="Which configuration option controls authentication timeout?" budget=~1400t candidates=4
[queries] configuration option | authentication timeout | timeout

[files]
- docs/guide.md
  picked: §1.2 Configuration · §1.2.1 Environment variables
  also: §1.1 Installation · §1.3 API Reference

[highlights]
- docs/guide.md §1.2.1 l64: `AUTH_TIMEOUT_SECONDS` controls token refresh and login timeout behavior.

[evidence]
--- docs/guide.md §1.2.1 Guide > Configuration > Environment variables l58-72 ~96t reason=lexical relevance: 3 query terms ---
...
```

## Commands

| Command | What it does |
|---------|-------------|
| `tree`  | Show section hierarchy with token estimates for a file or directory |
| `read`  | Extract a section by ID, heading path, or line range |
| `search`| Find sections matching a keyword or regex across files |
| `scout` | One-shot agent evidence pack for answering a natural-language question |
| `pack`  | Bundle selected sections into a hard token budget |
| `stats` | File-level sizes, word counts, and token estimates |
| `sections` | Read file paths from stdin and list section metadata or bodies |

## Usage

```bash
# Survey structure before reading anything
mdlens tree docs/

# Read a specific section by dotted ID
mdlens read docs/guide.md --id 1.2.1

# Read by full heading path
mdlens read docs/guide.md --heading-path "Configuration>Environment variables"

# Body only, no subsections
mdlens read docs/guide.md --id 1.2 --no-children

# Search across a directory
mdlens search docs/ "rate limit"

# Best first call for agent QA over a Markdown directory
mdlens scout docs/ "What does the guide recommend for rate limits?" --max-tokens 1400

# Step 1: see section structure of grep hits (~zero overhead)
grep -rl "rate limit" docs/ | mdlens sections --lines

# Or pass files directly (no pipe needed)
mdlens sections --lines docs/guide.md docs/api.md

# Whole-file structure mode defaults to depth <=2; opt out when needed
mdlens sections --lines --max-depth 3 docs/guide.md

# Step 2: map exact grep hits to their deepest matching sections
rg -nH "rate limit" docs/ | mdlens sections --preview 3 --max-sections 8 --max-files 5

# Step 3: pull content only for those matching sections
rg -nH "rate limit" docs/ | mdlens sections --content --max-sections 4 --max-tokens 4000 --max-files 5

# Hard-cap input to 5 files (errors if exceeded — recommended for --content)
grep -rl "rate limit" docs/ | mdlens sections --content --max-tokens 4000 --max-files 5

# Read one exact section by dotted ID
mdlens read docs/guide.md --id 1.2.1

# Pack a few sections into a 4k token budget
mdlens pack docs/guide.md --ids 1.1,1.2 --max-tokens 4000

# Pack by search results, include parent headings for context
mdlens pack docs/ --search "authentication" --max-tokens 8000 --parents
```

`mdlens sections` accepts either plain file paths or `rg -nH` line hits on stdin. When you pipe line hits, it maps each hit to the deepest matching section, which is much cheaper than expanding every section in a matched file.

In whole-file structure mode (`--lines`, or `--preview` without hit input), `mdlens sections` defaults to depth `<=2` to keep orientation cheap on large docs. Pass `--max-depth N` to override.

`--max-sections N` gives you a second guardrail after file selection: in hit-driven mode it keeps the highest-signal sections first, ranked by hit count and then section size.

`mdlens sections --content` emits each section's direct body by default, so parent sections do not duplicate all descendant text. Add `--children` if you explicitly want subtree content repeated under the parent entry.

`--max-files N` rejects the run if more than N files are piped in, so a stray `rg -l` over a large tree cannot accidentally dump megabytes of content. Recommended value for `--content` calls: `5`.

## Agent harness integration

For agent harnesses, plugins, `AGENTS.md`, `CLAUDE.md`, system prompts, or tool-router docs, add this exact minimal instruction:

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

The detailed workflow lives in `mdlens --help` and `mdlens scout --help` so harness prompts can stay short. This is intentional: future agent harnesses can discover usage at runtime instead of baking a long tool manual into every task prompt.

For direct use, the hot path is:

```bash
mdlens --help                         # optional, discover agent workflow
mdlens scout docs/ "$QUESTION" --max-tokens 1400
mdlens read docs/file.md --id 1.2 --max-tokens 1200  # only if scout leaves one detail missing
```

`scout` is designed for messy, general English Markdown, not only polished docs. It uses corpus-local lexical ranking, heading/path/body evidence, table-row context, parent heading/status context, source-authority conventions, and diversity/coverage selection for multi-file questions.

## Evaluations

Public eval notes and locked question sets live in [`evals/`](evals/). The generated corpora and model outputs are not included in this repository, but the question set and methodology are included so readers can see the task shapes.

The main messy-Markdown eval compared raw shell retrieval against the `mdlens scout` workflow on 500 generated Markdown files with stale notes, malformed tables, copied distractors, current-vs-stale config sections, and multi-file policy/rationale questions. Internal runs tracked success, keyword recall, tool calls, elapsed time, token telemetry, and available cost telemetry across Pi, opencode, native Claude CLI, and native Codex CLI.

A planned follow-up is a five-task mock workflow eval with fresh branches. Each task should combine Markdown analysis, a small code change, and JSON/data inspection to measure whether `mdlens` helps agents complete real repository work, not just answer retrieval questions.

## Section IDs

Dotted IDs reflect heading hierarchy:

```
1        = first H1
1.2      = second child of section 1
1.2.3    = third child of 1.2
```

Heading paths use `>` as a separator: `"Configuration>Environment variables"`. Escape a literal `>` as `\>`.

## JSON output

All commands support `--json` for stable machine-readable output with a `schema_version` field.

```bash
mdlens tree docs/ --json
mdlens search docs/ "config" --json
```

## Claude Code plugin

`mdlens` ships as a Claude Code plugin. Install it and Claude will automatically use `mdlens` instead of reading `.md` files raw.

```
/plugin install mdlens
```

## Installation

Requires Rust 1.70+. Install [rustup](https://rustup.rs/) if you don't have it.

```bash
# From crates.io (once published)
cargo install mdlens

# Directly from this repo
cargo install --git https://github.com/Dreeseaw/mdlens

# Build from source
git clone https://github.com/Dreeseaw/mdlens
cd mdlens && cargo build --release
# binary at target/release/mdlens
```

## License

MIT. See [LICENSE](LICENSE).
