# Eval Notes

This directory contains public eval materials for `mdlens`.

The large generated Markdown corpora and model result files are intentionally not included in the public crate repository. The included files are enough to show the question shapes, scoring style, and methodology without shipping megabytes of synthetic data.

## Slop Markdown v1

`slop_markdown_v1/questions.jsonl` is the locked question set used for the messy Markdown retrieval eval.

The private/generated corpus had 500 synthetic Markdown files, about 8.8 MiB total, designed to resemble difficult agent-facing documentation:

- malformed and inconsistent Markdown
- stale notes and copied distractor blocks
- table rows with nearby distractors
- current-vs-stale loader/config sections
- multi-file policy comparisons
- rationale and negative questions where evidence is present but easy to under-answer

The main comparison was raw shell retrieval versus the `mdlens` workflow:

- baseline: `rg`, `find`, `sed`, `cat`, and similar shell tools
- mdlens: first call should be `mdlens scout <dir> "<question>" --max-tokens 1400`; follow up with `mdlens read` only when one exact section detail is missing

Runs were executed across multiple agent harnesses, including Pi, opencode, native Claude CLI, and native Codex CLI. Reports tracked success, keyword recall, tool calls, elapsed time, and whatever token/cost telemetry each harness exposed.

## Future Mock Workflow Eval

A planned follow-up is a five-task "mock workflow" eval using fresh branches. Each task should require some combination of Markdown analysis, light coding, and JSON inspection. This is expected to be a better intrinsic workflow test than pure QA because it measures whether `mdlens` helps an agent complete real repository work with fewer irrelevant file reads.
