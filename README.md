# mdlens

Token-efficient Markdown structure CLI for AI agents. Navigate, search, and pack Markdown documentation into bounded context windows.

## Commands

- **tree** — Show section hierarchy with token estimates
- **read** — Extract a section by ID, heading path, or line range
- **search** — Find sections matching a query across files
- **pack** — Build a bounded context packet from selected sections
- **stats** — Inspect file sizes and token estimates

## Usage

```bash
# Show section tree
mdlens tree docs/

# Read a specific section
mdlens read docs/guide.md --id 1.2

# Read by full heading path
mdlens read docs/guide.md --heading-path "Setup>Configuration"

# Read only the section's direct body
mdlens read docs/guide.md --id 1 --no-children

# Search across files
mdlens search docs/ "authentication" --json

# Pack sections into token budget
mdlens pack docs/guide.md --ids 1.1,1.2 --max-tokens 4000

# Pack by search results
mdlens pack docs/ --search "API reference" --max-tokens 8000 --parents

# Pack by regex search and keep duplicate selections
mdlens pack docs/ --search "API|Reference" --regex --no-dedupe --max-tokens 8000
```

## Section IDs

Sections are identified by dotted IDs reflecting hierarchy:
- `1` — first H1 heading
- `1.1` — first child of section 1
- `1.2.3` — third child of section 1.2

Heading paths are exact section paths from the document root. Escape literal `>` characters inside a title as `\>`.

## JSON Output

All commands support `--json` for machine-readable output.

## Installation

```bash
cargo install --path .
```

## Benchmarks

```bash
cargo bench
```
