---
name: mdlens
description: "Use mdlens for Markdown navigation and retrieval. Prefer `mdlens scout` for question answering over Markdown directories; use `mdlens read` for exact follow-up sections. Trigger on requests that require reading, searching, comparing, summarizing, or answering from .md/.markdown files."
---

# mdlens: Markdown retrieval skill

Use `mdlens` instead of raw full-file reads when working with Markdown.

## Fast QA workflow

For a question over a Markdown directory, start with:

```bash
mdlens scout "$DOCS_DIR" "$QUESTION" --max-tokens 1400
```

Read `[highlights]` first, then `[evidence]`. If the answer is present, stop and answer directly. Preserve exact short source terms such as flags, IDs, option names, metrics, table values, and policy phrases.

If exactly one detail is missing, use the section map from `[files]`:

```bash
mdlens read "$FILE" --id "$SECTION_ID" --max-tokens 1200
```

Avoid reading whole Markdown files unless `scout` clearly found the wrong file.

## Other useful commands

```bash
# Survey structure with section IDs and token estimates
mdlens tree docs/
mdlens tree path/to/file.md

# Read a known section
mdlens read file.md --id 1.2
mdlens read file.md --heading-path "Setup>Configuration"
mdlens read file.md --id 1 --no-children

# Search and pack when scout is not the right entry point
mdlens search docs/ "authentication"
mdlens pack docs/ --search "API reference" --max-tokens 4000 --parents
```

## When to use which command

- Use `scout` for natural-language QA, multi-file comparison, stale/current checks, table lookup, and policy/rationale questions.
- Use `tree` when you need only file structure.
- Use `read` when you already know the section ID or heading path.
- Use `search` when you need a second independent query after `scout`.
- Use `pack` when you intentionally want several selected sections under a hard token budget.

Run `mdlens --help` and `mdlens scout --help` for the detailed agent workflow.
