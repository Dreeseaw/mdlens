# Changelog

All notable changes to `mdlens` are documented here. This project adheres to
[Semantic Versioning](https://semver.org/).

## [0.1.3]

### Added
- `mdlens init` — wires mdlens guidance into AI coding harnesses by writing a
  small, idempotent instruction block into the files your tools already read
  (`CLAUDE.md`, `AGENTS.md`, etc.). Supports project- and user-level config,
  per-harness selection, and `--dry-run`.
- `mdlens gain` — reports accumulated token savings from a plain append-only
  history (`~/.local/share/mdlens/history.jsonl`) that `scout` and `read` write.
  Disable with `MDLENS_NO_GAIN=1`; reset with `mdlens gain --reset --yes`.

### Changed
- `scout` default `--max-tokens` lowered from 1400 to 1000, and the `[highlights]`
  block trimmed from 10 to 7 lines (it duplicates content already present in the
  `[evidence]` sections). Together these cut evidence-pack size ~20% on the
  held-out set with retrieval recall held flat, and ~9% less context in
  end-to-end Claude Code runs at unchanged accuracy.
- `scout` evidence selection now applies MMR near-duplicate suppression and a
  tail-aware adaptive-k cutoff, improving recall while reducing redundant tokens.
- Injected `init` guidance now tells agents that `scout` always returns the
  nearest sections even when the answer is absent, so they answer only from
  evidence that states the fact (and say the docs do not specify it otherwise).

### Removed
- De-overfit: removed memorized synthetic-corpus strings from scout ranking
  (eval-phrase query mappings, heading-score phrase pairs, and section-name
  priors). Held-out recall is unchanged; only the previously over-tuned family
  dips, confirming the removed signal was memorization rather than generalization.

### Fixed
- Deterministic neighbor-candidate iteration in `scout`, so identical inputs
  always produce identical packs.

## [0.1.2] and earlier

See the git history and GitHub releases.
