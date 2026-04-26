# Eval Notes

This directory contains public eval materials for `mdlens`.

Only locked question sets and methodology notes are included. The generated or
private corpora and raw model outputs are intentionally omitted from the public
crate repository.

These evals are mostly Markdown search and question-answering workflows. They
measure whether agents can answer from documentation with fewer irrelevant reads,
fewer calls, lower cost, and better recall. They are not a broad benchmark of
general coding-agent performance.

## `combined_markdown_v1`

The v1 headline eval combines all three corpora below into one larger Markdown
collection: 1,783 files and 17.0 MB of source text. The locked question set has
30 hard questions:

- 10 from `messy_markdown_v1`
- 8 from `scicat_markdown_v1`
- 7 from `codebase_markdown_v1`
- 5 new workflow-like cross-corpus questions

The workflow-like questions ask for cross-document analysis over Markdown and
adjacent structured facts, but still require no code edits. This keeps the eval
focused on Markdown retrieval while being closer to the analysis tasks agents
actually do in repositories.

Headline result across 15 harness/model pairs where both arms completed all
rows:

| metric | baseline shell retrieval | mdlens scout workflow |
|---|---:|---:|
| average success | 19.7/30 | 22.7/30 |
| average tool calls | 7.5 | 2.6 |
| average reported cost, priced pairs | $2.41 | $0.93 |

The public repo includes only `questions.jsonl`. Local reports used for the v1
README also covered Codex, opencode, Pi, and Claude harnesses over GPT-5.4,
Sonnet 4.6, GPT-5.4 Mini, Haiku 4.5, Kimi K2.6, GLM 5.1, and Qwen 3.6 Plus.
Native Claude Sonnet produced partial provider/credit failures late in the run,
so the headline aggregate excludes partial harness/model pairs.

## `messy_markdown_v1`

The headline eval used 500 carefully curated synthetic Markdown files, about
8.8 MiB total. The corpus was not mindless filler: it was designed to stress
real agent failure modes in documentation retrieval.

Included breakage:

- malformed and inconsistent Markdown
- stale notes and copied distractor blocks
- multi-needle tables with nearby wrong values
- current-vs-stale loader/config sections
- cross-file policy comparisons
- rationale and negative questions where evidence is present but easy to
  under-answer

The main comparison was raw shell retrieval versus the `mdlens` workflow:

- baseline: `rg`, `find`, `sed`, `cat`, and similar shell tools
- mdlens: first call should be `mdlens scout <dir> "<question>" --max-tokens 1400`;
  follow up with `mdlens read` only when one exact section detail is missing

Earlier clean full-corpus runs:

| harness/model | baseline | mdlens | delta |
|---|---:|---:|---:|
| Pi + GPT-5.4 | 21/27, $3.2362 | 25/27, $0.6972 | +4 success, -78.5% cost |
| opencode + GPT-5.4 | 24/27, $2.4994 | 25/27, $0.9525 | +1 success, -61.9% cost |
| opencode + Sonnet 4.6 | 17/27, $2.1692 | 24/27, $0.9136 | +7 success, -57.9% cost |

## `scicat_markdown_v1`

This eval is a scientific Markdown proxy seeded from published SciCat research
metadata. The local fixture combined SciCat-derived scientific README targets
with Hugging Face model/dataset cards and GitHub scientific/research Markdown
fallback material to reach a realistic Markdown corpus size.

The 25 locked questions cover install commands, configuration flags, conceptual
summaries, cross-section README synthesis, and negative/abstention cases.

This eval is useful because it is closer to public scientific documentation than
the synthetic messy corpus. It was also less favorable to `mdlens` in early runs,
which helped identify where the scout workflow was too narrow.

## `codebase_markdown_v1`

This eval was built from real repository docs: runbooks, task docs, design notes,
and experiment reports. It tests the kind of Markdown navigation agents do while
working in a codebase.

The 12 locked questions include exact value lookup, cross-section synthesis,
multi-file reasoning, and negative checks about repo policy. This eval is still
mostly Markdown QA; it does not require the agent to edit code.

## Future Mock Workflow Eval

A planned follow-up is a five-task mock workflow eval using fresh branches. Each
task should combine Markdown analysis, light coding, and JSON inspection. That
would be a stronger intrinsic workflow test than pure QA, but it is intentionally
out of scope for v1.
