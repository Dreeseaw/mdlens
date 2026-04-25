# mdlens

Token-efficient Markdown structure CLI for AI agents. Navigate, search, and pack Markdown documentation into bounded context windows.

## Quick start

```bash
# Get a rich overview of all commands and flags
mdlens --help

# See detailed usage for a specific command
mdlens <command> --help
```

## Commands

| Command | Purpose |
|---------|---------|
| `tree` | Show section hierarchy with token estimates |
| `read` | Extract a section by ID, heading path, or line range |
| `search` | Find sections matching a query across files |
| `pack` | Build a bounded context packet from selected sections |
| `stats` | Inspect file sizes and token estimates |

## Core concepts

- **Section IDs** — Dotted IDs reflect hierarchy: `1` (first H1), `1.2` (second child of section 1), `1.2.3` (third child of 1.2)
- **Heading paths** — Full path from document root, `>`-separated: `Setup>Configuration`
- **Token estimates** — ~1 token per 4 UTF-8 chars (approximate but deterministic)
- **`--json`** — All commands support JSON output with `schema_version` for machine parsing

## Typical agent workflow

1. `mdlens tree docs/` — Survey the documentation structure
2. `mdlens search docs/ "authentication"` — Find relevant sections
3. `mdlens pack docs/ --search "authentication" --max-tokens 4000` — Pack into context budget
