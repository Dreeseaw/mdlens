# Implement: grep | mdlens sections

## Context
- Project: `mdlens/` — Rust CLI for token-efficient markdown navigation
- Existing: `tree`, `read`, `search`, `pack`, `stats` commands
- Parser: `src/parse.rs` — already parses markdown into sections with IDs, line ranges, headings
- Goal: One-command structured section retrieval piped from grep

## Feature: `mdlens sections`

Reads file paths from stdin (one per line, as output by `grep -rl`), parses only those files, maps match context to sections, returns structured section output.

### CLI

```
mdlens sections [OPTIONS]
```

Options:
- `--content` — Include full section body text (default: metadata only)
- `--max-tokens N` — Cap total output tokens (truncates last section if exceeded)
- `--json` — Machine-readable output
- `--heading-paths` — Include heading path (e.g. "SGOCR Champion > Candidate Quality")
- `--lines` — Include original line numbers (start-end)
- `--dedupe` — Deduplicate sections if same section matches multiple lines (default: true)

### Input

File paths from stdin, one per line:
```
tasks/mm_bridge/docs/SGOCR_CHAMPION.md
tasks/vlm_cleo/docs/CLEO_STATE.md
```

### Output (default, --content)

```
tasks/mm_bridge/docs/SGOCR_CHAMPION.md
  §1.4.1 Candidate Quality (lines 345-381, ~890 tokens)
    candidate_quality = 0.38 * ocr_confidence + 0.12 * anchor_score + ...
    [full section body]

  §1.4.2 Selection Quality After Candidate Quality (lines 399-423, ~520 tokens)
    selection_score = candidate_quality + 1.7 if ...
    [full section body]
```

### Output (--json)

```json
{
  "schema_version": 1,
  "files": [
    {
      "path": "tasks/mm_bridge/docs/SGOCR_CHAMPION.md",
      "sections": [
        {
          "id": "1.4.1",
          "title": "Candidate Quality",
          "heading_path": ["SGOCR Champion Sheet", "Candidate Quality"],
          "line_start": 345,
          "line_end": 381,
          "token_estimate": 890,
          "body": "candidate_quality = ..."
        }
      ]
    }
  ]
}
```

### Token capping (--max-tokens N)

- Accumulate sections in order
- Before adding each section, check if total would exceed N
- If exceeded, skip remaining sections (don't partially truncate mid-section)
- Print a warning to stderr: "Warning: 3 sections omitted, would exceed 4000 token limit"

## Implementation

1. **New command in `src/cli.rs`** — `Sections` struct with the options above
2. **New logic in `src/lib.rs` or new `src/sections.rs`** — reads stdin, parses files, maps to sections, formats output
3. **Reuse existing** — `src/parse.rs` for parsing, `src/tokens.rs` for token estimates, `src/render.rs` for formatting
4. **stdin handling** — use `std::io::stdin()` with `BufRead::lines()`, skip empty lines, handle missing files gracefully (warn to stderr, continue)

## Edge cases

- Empty stdin → print nothing, exit 0
- File doesn't exist → warn to stderr, continue with remaining files
- File has no sections (no headings) → treat entire file as section "1"
- Duplicate paths in stdin → deduplicate before processing
- Non-markdown files → skip with warning (or parse anyway, markdown parser is lenient)

## Testing

- Add `tests/cli_sections.rs` with fixtures:
  - Basic: 2 files, 3 matching sections
  - Max tokens: verify capping works
  - JSON output: verify schema
  - Missing files: verify graceful handling
  - Empty stdin: verify clean exit
  - Duplicate paths: verify dedup

## SKILL.md / README updates

Add to SKILL.md:
```bash
# Find sections matching a term (one command)
grep -rl "term" docs/ | mdlens sections --content

# Cap output to token budget
grep -rl "term" docs/ | mdlens sections --content --max-tokens 4000

# JSON for programmatic use
grep -rl "term" docs/ | mdlens sections --json
```

## What NOT to do

- Don't add a query language or pipe syntax inside mdlens
- Don't embed grep functionality — grep stays external
- Don't add caching, state, or dotfiles
- Don't change existing commands — this is additive only
