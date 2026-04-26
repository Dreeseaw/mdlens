use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::cmp::Reverse;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{self, BufRead};
use std::path::Path;

use crate::errors;
use crate::model::Section;
use crate::pack::{pack_by_ids, PackSearchOptions};
use crate::parse::{load_markdown, parse_markdown};
use crate::render::{
    render_pack, render_read, render_search, render_sections, render_stats, render_tree,
    FileSectionsMap, PackIncluded, SectionsEntry, StatsEntry,
};
use crate::search::{discover_markdown_files, get_doc_section_summaries, search_files};
use crate::tokens::{estimate_tokens, truncate_to_tokens};

const TRUNCATION_NOTICE: &str = "\n\n<!-- mdlens: truncated at token budget -->";

#[derive(Parser)]
#[command(name = "mdlens")]
#[command(about = "Token-efficient Markdown structure CLI for AI agents")]
#[command(
    long_about = "mdlens parses Markdown files into a hierarchical section tree with\ndotted IDs, token estimates, and bounded-context packing.\n\nDesigned for AI agents that need to navigate, search, and pack\nMarkdown documentation into context windows efficiently.\n\nAgent quickstart:\n  1. For question answering over a Markdown directory, start with:\n       mdlens scout <dir> \"<question>\" --max-tokens 1400\n  2. Answer from scout when [highlights] and [evidence] are sufficient.\n  3. If one detail is missing, use a listed section id:\n       mdlens read <file> --id <N.N> --max-tokens 1200\n  4. Use search/tree/sections only when scout points at the wrong file or you\n     need broader navigation.\n\nScout is the recommended first command for arbitrary messy English markdown.\nIt returns query expansion, a compact file map, ranked highlights, and bounded\nevidence sections with parent heading/status context.\n\nAnswering from scout:\n  - Read [highlights] first, then [evidence].\n  - Preserve distinctive evidence terms: flags, IDs, metrics, option names,\n    labels, row values, and short policy/risk phrases.\n  - Copy short source phrases exactly when they are likely answer terms; avoid\n    changing singular/plural or rewriting concise labels into paraphrases.\n  - If scout already names the answer plus its rule, risk, command, or policy,\n    answer directly instead of continuing broad retrieval.\n  - For current-vs-stale questions, prefer current/current loader sections and\n    treat Do Not Use, copied tables, stale notes, and old runbooks as\n    distractors.\n  - For table questions, keep the table header with the selected row; do not\n    average unrelated rows unless the document says to.\n  - For why, policy, safety, privacy, negative, or tradeoff questions, include\n    the compact rule/risk/rationale bullets, not only the command or metric.\n  - For multi-file comparisons, answer each named entity separately, then\n    summarize the shared pattern.\n  - If evidence is missing, say the corpus does not specify the fact.\n\nRun `mdlens scout --help` for detailed scout-specific guidance."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show section hierarchy with token estimates for a file or directory
    Tree(TreeArgs),
    /// Extract a section by ID, heading path, or line range
    Read(ReadArgs),
    /// Search files and return section-level matches with snippets
    Search(SearchArgs),
    /// One-shot agent evidence pack: find files, show section maps, and include likely evidence
    Scout(ScoutArgs),
    /// Pack selected sections into a bounded token budget
    Pack(PackArgs),
    /// Inspect file sizes, word counts, and token estimates
    Stats(StatsArgs),
    /// Read file paths from stdin and output structured section metadata
    Sections(SectionsArgs),
}

#[derive(clap::Args)]
struct TreeArgs {
    /// File or directory to analyze
    path: String,
    /// Output JSON (machine-readable with schema_version)
    #[arg(long)]
    json: bool,
    /// Limit section depth shown
    #[arg(long)]
    max_depth: Option<usize>,
    /// Show preamble section (content before first heading)
    #[arg(long)]
    include_preamble: bool,
    /// For directory input, include per-file summaries
    #[arg(long)]
    files: bool,
}

#[derive(clap::Args)]
struct ReadArgs {
    /// File to read from
    file: String,
    /// Section ID to extract (e.g., "1.2.3" — dotted hierarchy)
    #[arg(long, conflicts_with_all = ["heading_path", "lines"])]
    id: Option<String>,
    /// Heading path to extract (e.g., "Usage>Configuration"; escape literal > as \>)
    #[arg(long, conflicts_with_all = ["id", "lines"])]
    heading_path: Option<String>,
    /// Line range to extract (e.g., "120:190")
    #[arg(long, conflicts_with_all = ["id", "heading_path"])]
    lines: Option<String>,
    /// Include parent headings above the section excerpt
    #[arg(long)]
    parents: bool,
    /// Include all child sections (default: true unless --no-children)
    #[arg(long, conflicts_with = "no_children")]
    children: bool,
    /// Only include heading and direct body before first child heading
    #[arg(long, conflicts_with = "children")]
    no_children: bool,
    /// Truncate output to approximate token budget
    #[arg(long)]
    max_tokens: Option<usize>,
    /// Output JSON (machine-readable with schema_version)
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct SearchArgs {
    /// File or directory to search
    path: String,
    /// Search query (plain text or regex with --regex)
    query: String,
    /// Output JSON (machine-readable with schema_version)
    #[arg(long)]
    json: bool,
    /// Use regex for the query
    #[arg(long)]
    regex: bool,
    /// Case-sensitive search (default: case-insensitive)
    #[arg(long)]
    case_sensitive: bool,
    /// Maximum number of results (default: 20)
    #[arg(long, default_value_t = 20)]
    max_results: usize,
    /// Context lines around each match (default: 2)
    #[arg(long, default_value_t = 2)]
    context_lines: usize,
    /// Include full section body text for each result
    #[arg(long)]
    content: bool,
    /// Show first N non-empty lines of each matched section inline
    #[arg(long)]
    preview: Option<usize>,
    /// Cap total output tokens across included search results
    #[arg(long)]
    max_tokens: Option<usize>,
}

#[derive(clap::Args)]
#[command(
    long_about = "One-shot agent evidence pack for answering a natural-language question over Markdown.\n\n`scout` is optimized for agent workflows: fewer shell calls, bounded output,\nand enough section context to answer without dumping whole files. It searches\nsection text, headings, paths, parent context, and table rows; ranks likely\nevidence; then emits a compact pack."
)]
#[command(
    after_help = "Agent workflow:\n  - Use scout as the first retrieval call for QA over a directory:\n      mdlens scout docs/ \"What policy changed between the old and current loader?\" --max-tokens 1400\n  - Use --json when a harness wants structured metadata plus the same rendered evidence pack.\n  - Read [highlights] first. They are globally ranked compact evidence lines.\n  - Then read [evidence]. Each block names file, section id, heading path, line\n    span, token estimate, and ranking reason.\n  - If the answer is present, stop and answer directly. Preserve distinctive\n    terms: flags, IDs, metrics, option names, row values, labels, and short\n    policy phrases.\n  - Copy short source phrases exactly when they are likely answer terms; avoid\n    changing singular/plural or rewriting concise labels into paraphrases.\n  - If exactly one fact is missing, use the section map from [files] and read\n    one section:\n      mdlens read <file> --id <section-id> --max-tokens 1200\n  - Use `mdlens search` only when scout clearly found the wrong file or when\n    you need a second independent query.\n\nHow to interpret scout output:\n  [queries]   Search expansions derived from the question.\n  [files]     Candidate files, picked section ids, and nearby unread sections.\n  [focus]     Dominant file when the question appears single-file.\n  [highlights] Globally ranked lines/table rows likely to answer the question.\n  [evidence]  Bounded excerpts from the selected sections.\n\nQuestion-shape guidance:\n  - Current-vs-stale questions: prefer sections marked current/current loader;\n    treat Do Not Use, stale notes, copied tables, and old runbooks as distractors.\n  - Table questions: keep the table header with the selected row; do not average\n    unrelated rows unless the document says to.\n  - Why, policy, safety, privacy, negative, or tradeoff questions: include the\n    compact rule/risk/rationale bullets, not only the command or metric.\n  - Multi-file comparison: answer each named entity separately, then summarize\n    the shared pattern.\n  - Missing evidence: say the corpus does not specify the fact rather than\n    guessing from file names.\n\nUseful defaults:\n  --max-tokens 1400 keeps scout cheap for most agent turns.\n  --max-sections 12 gives enough diversity before packing.\n  --max-files 4 keeps the file map readable."
)]
struct ScoutArgs {
    /// File or directory to scout
    path: String,
    /// Natural-language question or retrieval goal
    question: String,
    /// Output JSON (machine-readable with schema_version)
    #[arg(long)]
    json: bool,
    /// Approximate evidence-token budget (default: 1400)
    #[arg(long, default_value_t = 1400)]
    max_tokens: usize,
    /// Maximum candidate sections to consider before packing (default: 12)
    #[arg(long, default_value_t = 12)]
    max_sections: usize,
    /// Maximum files to include in the file map (default: 4)
    #[arg(long, default_value_t = 4)]
    max_files: usize,
}

#[derive(clap::Args)]
struct PackArgs {
    /// File or directory to pack from
    path: String,
    /// Comma-separated section IDs to include
    #[arg(long, conflicts_with_all = ["paths", "search"])]
    ids: Option<String>,
    /// Semicolon-separated heading paths to include
    #[arg(long, conflicts_with_all = ["ids", "search"])]
    paths: Option<String>,
    /// Search query to find sections to pack
    #[arg(long, conflicts_with_all = ["ids", "paths"])]
    search: Option<String>,
    /// Required: maximum token budget
    #[arg(long)]
    max_tokens: usize,
    /// Include parent heading context above selected sections
    #[arg(long)]
    parents: bool,
    /// Avoid duplicate nested sections (default)
    #[arg(long, conflicts_with = "no_dedupe")]
    dedupe: bool,
    /// Allow duplicate sections in the final pack
    #[arg(long, conflicts_with = "dedupe")]
    no_dedupe: bool,
    /// Use regex when selecting sections via --search
    #[arg(long)]
    regex: bool,
    /// Case-sensitive search when selecting sections via --search
    #[arg(long)]
    case_sensitive: bool,
    /// Maximum number of search results to consider for --search (default: 20)
    #[arg(long, default_value_t = 20)]
    max_results: usize,
    /// Context lines when searching via --search (default: 2)
    #[arg(long, default_value_t = 2)]
    context_lines: usize,
    /// Output JSON (machine-readable with schema_version)
    #[arg(long)]
    json: bool,
}

#[derive(Clone, ValueEnum)]
enum StatsSort {
    Path,
    Tokens,
    Lines,
}

#[derive(clap::Args)]
struct StatsArgs {
    /// File or directory to analyze
    path: String,
    /// Output JSON (machine-readable with schema_version)
    #[arg(long)]
    json: bool,
    /// Sort by field: path, tokens, or lines (default: path)
    #[arg(long, value_enum, default_value_t = StatsSort::Path)]
    sort: StatsSort,
    /// Show top N results
    #[arg(long)]
    top: Option<usize>,
}

#[derive(clap::Args)]
struct SectionsArgs {
    /// File paths to process (alternative or supplement to stdin)
    #[arg(value_name = "FILE")]
    files: Vec<String>,
    /// Include full section body text (default: metadata only)
    #[arg(long)]
    content: bool,
    /// Include descendant subsection text inside each section body
    #[arg(long)]
    children: bool,
    /// Show first N lines of each section body inline (cheaper than --content; helps pick the right section before a full read)
    #[arg(long)]
    preview: Option<usize>,
    /// Limit section hierarchy depth shown (default: unlimited)
    #[arg(long)]
    max_depth: Option<usize>,
    /// Cap total output tokens (truncates last section if exceeded)
    #[arg(long)]
    max_tokens: Option<usize>,
    /// Cap the number of sections emitted after selection/ranking
    #[arg(long)]
    max_sections: Option<usize>,
    /// Reject input if more than N files are piped (prevents accidental large reads; recommended: 5)
    #[arg(long)]
    max_files: Option<usize>,
    /// Machine-readable JSON output
    #[arg(long)]
    json: bool,
    /// Include heading path (e.g. "SGOCR Champion > Candidate Quality")
    #[arg(long)]
    heading_paths: bool,
    /// Include original line numbers (start-end)
    #[arg(long)]
    lines: bool,
    /// Deduplicate sections if same section matches multiple lines (default: true)
    #[arg(long, default_value_t = true)]
    dedupe: bool,
    /// Allow duplicate sections in output
    #[arg(long, conflicts_with = "dedupe")]
    no_dedupe: bool,
}

#[derive(Clone)]
struct SectionHit {
    path: String,
    line: usize,
}

enum SectionInput {
    File(String),
    Hit(SectionHit),
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Tree(args) => cmd_tree(args),
        Commands::Read(args) => cmd_read(args),
        Commands::Search(args) => cmd_search(args),
        Commands::Scout(args) => cmd_scout(args),
        Commands::Pack(args) => cmd_pack(args),
        Commands::Stats(args) => cmd_stats(args),
        Commands::Sections(args) => cmd_sections(args),
    }
}

fn cmd_tree(args: TreeArgs) -> Result<()> {
    let files = crate::search::discover_markdown_files(&args.path)?;

    if files.len() == 1 {
        let doc = parse_markdown(&files[0])?;
        if args.json {
            let output = TreeJsonOutput {
                schema_version: 1,
                path: doc.path.clone(),
                line_count: doc.line_count,
                byte_count: doc.byte_count,
                char_count: doc.char_count,
                word_count: doc.word_count,
                token_estimate: doc.token_estimate,
                sections: serialize_sections(
                    &doc.sections,
                    args.max_depth,
                    args.include_preamble,
                    0,
                ),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!(
                "{}",
                render_tree(&doc, args.max_depth, args.include_preamble)
            );
        }
    } else {
        // Multiple files — cap depth at 1 by default to keep directory output manageable
        let depth_capped = args.max_depth.is_none();
        let effective_depth = args.max_depth.or(Some(1));

        if args.json {
            let mut file_outputs = Vec::new();
            for file in &files {
                let doc = parse_markdown(file)?;
                file_outputs.push(TreeFileJsonOutput {
                    path: doc.path.clone(),
                    line_count: doc.line_count,
                    byte_count: doc.byte_count,
                    char_count: doc.char_count,
                    word_count: doc.word_count,
                    token_estimate: doc.token_estimate,
                    sections: serialize_sections(
                        &doc.sections,
                        effective_depth,
                        args.include_preamble,
                        0,
                    ),
                });
            }
            let output = TreeMultiJsonOutput {
                schema_version: 1,
                files: file_outputs,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            for file in &files {
                let doc = parse_markdown(file)?;
                println!(
                    "\n{}",
                    render_tree(&doc, effective_depth, args.include_preamble)
                );
            }
            if depth_capped {
                eprintln!("[tree] directory mode: showing depth ≤1 by default; use --max-depth N for more");
            }
        }
    }

    Ok(())
}

fn cmd_read(args: ReadArgs) -> Result<()> {
    let parsed = load_markdown(&args.file)?;
    let doc = &parsed.doc;
    let lines = &parsed.lines;
    let include_children = !args.no_children || args.children;

    let (section_text, section_meta, selector_type, selector_value, section_ref) =
        if let Some(ref id) = args.id {
            let section = doc
                .find_section_by_id(id)
                .ok_or_else(|| anyhow::anyhow!("section id not found: {id}"))?;
            let content = if include_children {
                section.extract_content(lines)
            } else {
                section.extract_direct_content(lines)
            }
            .join("\n");
            (
                content,
                SectionMeta::from(section),
                "id",
                id.clone(),
                Some(section),
            )
        } else if let Some(ref path_str) = args.heading_path {
            let section = find_unique_section_by_path(doc, path_str)?;
            let content = if include_children {
                section.extract_content(lines)
            } else {
                section.extract_direct_content(lines)
            }
            .join("\n");
            (
                content,
                SectionMeta::from(section),
                "path",
                path_str.clone(),
                Some(section),
            )
        } else if let Some(ref lines_str) = args.lines {
            let parts: Vec<&str> = lines_str.split(':').collect();
            if parts.len() != 2 {
                return Err(anyhow::anyhow!(
                    "invalid line range: {}; expected format START:END",
                    lines_str
                ));
            }
            let start: usize = parts[0].trim().parse()?;
            let end: usize = parts[1].trim().parse()?;
            if start > end {
                return Err(errors::invalid_line_range(start, end));
            }
            if start < 1 || end > lines.len() {
                return Err(anyhow::anyhow!(
                    "line range {}:{} out of bounds (file has {} lines)",
                    start,
                    end,
                    lines.len()
                ));
            }
            let content = lines[(start - 1)..end].join("\n");
            let token_est = estimate_tokens(&content);
            (
                content,
                SectionMeta {
                    id: format!("lines:{}:{}", start, end),
                    title: format!("Lines {}-{}", start, end),
                    level: 0,
                    path: vec![format!("Lines {}-{}", start, end)],
                    line_start: start,
                    line_end: end,
                    token_estimate: token_est,
                },
                "lines",
                format!("{}:{}", start, end),
                None,
            )
        } else {
            return Err(anyhow::anyhow!(
                "exactly one of --id, --heading-path, or --lines is required"
            ));
        };

    let mut full_content = String::new();

    if args.parents {
        if let Some(sec) = section_ref {
            let parents = find_parent_headings(doc, sec);
            for line_idx in parents {
                if !full_content.is_empty() {
                    full_content.push_str("\n\n");
                }
                full_content.push_str(&lines[line_idx - 1]);
            }
        }
    }

    if !full_content.is_empty() && !section_text.is_empty() {
        full_content.push_str("\n\n");
    }
    full_content.push_str(&section_text);

    let truncated = if let Some(max_tokens) = args.max_tokens {
        if estimate_tokens(&full_content) > max_tokens {
            full_content = truncate_content_to_tokens(&full_content, max_tokens);
            true
        } else {
            false
        }
    } else {
        false
    };

    if args.json {
        let output = ReadJsonOutput {
            schema_version: 1,
            path: doc.path.clone(),
            selector: ReadSelector {
                r#type: selector_type.to_string(),
                value: selector_value.to_string(),
            },
            section: SectionJsonOutput {
                id: section_meta.id.clone(),
                title: section_meta.title.clone(),
                level: section_meta.level,
                path: section_meta.path.clone(),
                line_start: section_meta.line_start,
                line_end: section_meta.line_end,
                token_estimate: section_meta.token_estimate,
                children: Vec::new(),
            },
            content: full_content,
            truncated,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let section = Section {
            id: section_meta.id.clone(),
            slug: Section::slugify(&section_meta.title),
            title: section_meta.title.clone(),
            level: section_meta.level,
            path: section_meta.path.clone(),
            line_start: section_meta.line_start,
            line_end: section_meta.line_end,
            content_line_start: section_meta.line_start,
            byte_start: 0,
            byte_end: 0,
            char_count: 0,
            word_count: 0,
            token_estimate: section_meta.token_estimate,
            children: Vec::new(),
        };
        println!("{}", render_read(&section, &full_content, truncated));
    }

    Ok(())
}

struct SectionMeta {
    id: String,
    title: String,
    level: u8,
    path: Vec<String>,
    line_start: usize,
    line_end: usize,
    token_estimate: usize,
}

impl From<&Section> for SectionMeta {
    fn from(s: &Section) -> Self {
        SectionMeta {
            id: s.id.clone(),
            title: s.title.clone(),
            level: s.level,
            path: s.path.clone(),
            line_start: s.line_start,
            line_end: s.line_end,
            token_estimate: s.token_estimate,
        }
    }
}

/// Find parent heading line numbers for a section.
fn find_parent_headings(doc: &crate::model::Document, section: &Section) -> Vec<usize> {
    let mut parent_map: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    build_parent_map(&doc.sections, None, &mut parent_map);
    let mut chain = Vec::new();
    let mut current_id = section.id.clone();
    while let Some(Some(pid)) = parent_map.get(&current_id) {
        if let Some(parent_sec) = doc.find_section_by_id(pid) {
            chain.push(parent_sec.line_start);
        }
        current_id = pid.clone();
    }
    chain.reverse();
    chain
}

fn find_unique_section_by_path<'a>(
    doc: &'a crate::model::Document,
    path_str: &str,
) -> Result<&'a Section> {
    let path = parse_heading_path(path_str);
    let matches = doc.find_sections_by_path(&path);
    match matches.len() {
        0 => Err(anyhow::anyhow!("path not found: {path_str}")),
        1 => Ok(matches[0]),
        _ => Err(errors::ambiguous_path(path_str, &matches)),
    }
}

fn parse_heading_path(path: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for ch in path.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '>' => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    let part = current.trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }

    parts
}

fn build_parent_map(
    sections: &[Section],
    parent_id: Option<String>,
    map: &mut std::collections::HashMap<String, Option<String>>,
) {
    for section in sections {
        map.insert(section.id.clone(), parent_id.clone());
        build_parent_map(&section.children, Some(section.id.clone()), map);
    }
}

fn cmd_search(args: SearchArgs) -> Result<()> {
    let mut results = search_files(
        &args.path,
        &args.query,
        args.case_sensitive,
        args.regex,
        args.max_results,
        args.context_lines,
    )?;

    if args.content || args.preview.is_some() || args.max_tokens.is_some() {
        enrich_search_results(&mut results, args.content, args.preview)?;
    }

    if let Some(max_tokens) = args.max_tokens {
        let mut kept = Vec::new();
        let mut total_tokens = 0usize;
        for result in results {
            let item_tokens = if args.content {
                result
                    .body
                    .as_ref()
                    .map(|body| estimate_tokens(body))
                    .unwrap_or(result.token_estimate)
            } else if let Some(preview) = &result.preview {
                estimate_tokens(preview)
            } else {
                result.token_estimate
            };
            if total_tokens + item_tokens > max_tokens {
                break;
            }
            total_tokens += item_tokens;
            kept.push(result);
        }
        results = kept;
    }

    if args.json {
        let output = SearchJsonOutput {
            schema_version: 1,
            query: args.query,
            root: args.path,
            results: results
                .iter()
                .map(|r| SearchJsonResult {
                    path: r.path.clone(),
                    section_id: r.section_id.clone(),
                    section_title: r.section_title.clone(),
                    section_path: r.section_path.clone(),
                    line_start: r.line_start,
                    line_end: r.line_end,
                    token_estimate: r.token_estimate,
                    match_count: r.match_count,
                    body: r.body.clone(),
                    preview: r.preview.clone(),
                    snippets: r
                        .snippets
                        .iter()
                        .map(|s| SearchJsonSnippet {
                            line_start: s.line_start,
                            line_end: s.line_end,
                            text: s.text.clone(),
                        })
                        .collect(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let file_sections = build_file_sections_map(&results);
        println!("{}", render_search(&results, args.content, &file_sections));
    }

    Ok(())
}

fn build_file_sections_map(results: &[crate::render::SearchResult]) -> FileSectionsMap {
    let unique_files: std::collections::HashSet<&str> =
        results.iter().map(|r| r.path.as_str()).collect();
    let mut map = FileSectionsMap::new();
    for path in unique_files {
        if let Ok(summaries) = get_doc_section_summaries(path) {
            map.insert(path.to_string(), summaries);
        }
    }
    map
}

#[derive(Clone, Serialize)]
struct ScoutCandidate {
    path: String,
    section_id: String,
    score: i32,
    reason: String,
}

struct ScoutHighlight {
    score: i32,
    path: String,
    section_id: String,
    line_no: usize,
    line: String,
}

fn cmd_scout(args: ScoutArgs) -> Result<()> {
    let queries = scout_queries(&args.question);
    let mut candidates: Vec<ScoutCandidate> = Vec::new();
    let per_query_results = (args.max_sections * 3).max(args.max_sections).min(60);

    for query in &queries {
        let results = search_files(&args.path, query, false, false, per_query_results, 2)?;
        for result in results {
            let query_tokens = signal_tokens(query);
            let normalized_path = normalize_for_match(&result.path);
            let path_quality_score = scout_path_quality_score(&result.path);
            let path_hits = query_tokens
                .iter()
                .filter(|token| normalized_path.contains(&normalize_for_match(token)))
                .count() as i32;
            let path_boost = if path_hits > 0 {
                180 + path_hits * 45
            } else {
                0
            };
            let broad_penalty = if path_hits == 0 && query_tokens.len() <= 1 {
                60
            } else {
                0
            };
            candidates.push(ScoutCandidate {
                path: result.path,
                section_id: result.section_id,
                score: 100
                    + path_boost
                    + path_quality_score
                    + result.match_count as i32 * 5
                    + scout_heading_score(
                        &result.section_path,
                        &result.section_title,
                        &args.question,
                    )
                    - result.token_estimate as i32 / 250
                    - broad_penalty,
                reason: format!("content match: {query}"),
            });
        }
    }

    add_lexical_scout_candidates(
        &args.path,
        &args.question,
        &mut candidates,
        args.max_sections * 4,
    )?;
    add_path_match_candidates(&args.path, &args.question, &mut candidates)?;
    add_named_target_candidates(&args.path, &args.question, &mut candidates)?;
    add_neighbor_candidates(&mut candidates)?;

    candidates.sort_by(|lhs, rhs| {
        rhs.score
            .cmp(&lhs.score)
            .then(lhs.path.cmp(&rhs.path))
            .then(lhs.section_id.cmp(&rhs.section_id))
    });
    dedupe_scout_candidates(&mut candidates);
    prune_parent_scout_candidates(&mut candidates);
    let candidate_pool = candidates.clone();
    diversify_scout_candidates(&mut candidates, args.max_sections, &args.question);
    ensure_named_target_coverage(
        &mut candidates,
        &candidate_pool,
        args.max_sections,
        &args.question,
    )?;
    candidates.truncate(args.max_sections);

    let mut out = String::new();
    out.push_str(&format!(
        "[scout] question=\"{}\" budget=~{}t candidates={}\n",
        args.question,
        args.max_tokens,
        candidates.len()
    ));
    if !queries.is_empty() {
        out.push_str(&format!("[queries] {}\n", queries.join(" | ")));
    }
    out.push('\n');
    let evidence_candidates = order_scout_evidence(
        focused_scout_candidates(&candidates, &args.question),
        &args.question,
    )?;
    let map_candidates = if evidence_candidates.len() < candidates.len() {
        &evidence_candidates
    } else {
        &candidates
    };
    render_scout_file_maps(&mut out, map_candidates, args.max_files)?;
    if !evidence_candidates.is_empty() && evidence_candidates.len() < candidates.len() {
        out.push_str(&format!("\n[focus] {}\n", evidence_candidates[0].path));
    }
    out.push_str("\n[highlights]\n");
    render_scout_highlights(&mut out, &evidence_candidates, &args.question, 10)?;
    out.push_str("\n[evidence]\n");
    render_scout_evidence(
        &mut out,
        &evidence_candidates,
        &args.question,
        args.max_tokens,
    )?;

    if args.json {
        let output = ScoutJsonOutput {
            schema_version: 1,
            root: args.path,
            question: args.question,
            token_budget: args.max_tokens,
            candidate_count: candidates.len(),
            queries,
            candidates: evidence_candidates,
            rendered_text: out,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print!("{out}");
    }
    Ok(())
}

fn scout_queries(question: &str) -> Vec<String> {
    let mut queries = Vec::new();
    let phrases = extract_capitalized_phrases(question);
    for phrase in phrases {
        let cleaned = clean_query_phrase(&phrase);
        push_unique_query(&mut queries, cleaned.clone());
        if cleaned.contains('-') {
            push_unique_query(&mut queries, cleaned.replace('-', " "));
        }
    }

    for phrase in scout_semantic_queries(question) {
        push_unique_query(&mut queries, phrase);
    }

    let signal_tokens = signal_tokens(question);
    for token in signal_tokens.into_iter().take(8) {
        if token.len() >= 8
            || token.contains('-')
            || token.contains('_')
            || token.chars().any(|c| c.is_ascii_digit())
        {
            push_unique_query(&mut queries, token);
        }
    }

    if queries.is_empty() {
        push_unique_query(&mut queries, question.to_string());
    }
    queries.truncate(12);
    queries
}

fn add_lexical_scout_candidates(
    root: &str,
    question: &str,
    candidates: &mut Vec<ScoutCandidate>,
    limit: usize,
) -> Result<()> {
    let query_terms = lexical_query_terms(question);
    if query_terms.is_empty() {
        return Ok(());
    }

    struct LexicalSection {
        path: String,
        section_id: String,
        section_path: Vec<String>,
        section_title: String,
        token_estimate: usize,
        len: usize,
        terms: HashMap<String, usize>,
        title_terms: HashSet<String>,
        path_terms: HashSet<String>,
    }

    let files = discover_markdown_files(root)?;
    let mut sections = Vec::new();
    let mut df: HashMap<String, usize> = HashMap::new();
    let mut total_len = 0usize;

    for file in files {
        let parsed = load_markdown(&file)?;
        let path_terms = lexical_terms(&file).into_iter().collect::<HashSet<_>>();
        for section in flatten_doc_sections(&parsed.doc.sections) {
            if section.title == "<preamble>" {
                continue;
            }
            let content = section.extract_content(&parsed.lines).join("\n");
            let title_text = section.path.join(" ");
            let mut terms = lexical_terms(&format!("{title_text}\n{content}"));
            if terms.is_empty() {
                continue;
            }
            let title_terms = lexical_terms(&title_text)
                .into_iter()
                .collect::<HashSet<_>>();
            let mut tf = HashMap::new();
            let mut unique = HashSet::new();
            for term in terms.drain(..) {
                *tf.entry(term.clone()).or_insert(0) += 1;
                unique.insert(term);
            }
            for term in unique {
                *df.entry(term).or_insert(0) += 1;
            }
            let len = tf.values().sum::<usize>().max(1);
            total_len += len;
            sections.push(LexicalSection {
                path: file.clone(),
                section_id: section.id.clone(),
                section_path: section.path.clone(),
                section_title: section.title.clone(),
                token_estimate: section.token_estimate,
                len,
                terms: tf,
                title_terms,
                path_terms: path_terms.clone(),
            });
        }
    }

    let n = sections.len();
    if n == 0 {
        return Ok(());
    }
    let avg_len = total_len as f64 / n as f64;
    let unique_query_terms = query_terms.into_iter().collect::<BTreeSet<_>>();
    let mut scored = Vec::new();

    for section in sections {
        let mut score = 0.0f64;
        let mut matched = 0usize;
        for term in &unique_query_terms {
            let tf = section.terms.get(term).copied().unwrap_or(0) as f64;
            let title_hit = section.title_terms.contains(term);
            let path_hit = section.path_terms.contains(term);
            if tf == 0.0 && !title_hit && !path_hit {
                continue;
            }
            matched += 1;
            let doc_freq = df.get(term).copied().unwrap_or(1) as f64;
            let idf = ((n as f64 - doc_freq + 0.5) / (doc_freq + 0.5) + 1.0).ln();
            let k1 = 1.2;
            let b = 0.75;
            let bm25 = if tf > 0.0 {
                idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * section.len as f64 / avg_len))
            } else {
                0.0
            };
            score += bm25;
            if title_hit {
                score += idf * 1.8;
            }
            if path_hit {
                score += idf * 1.1;
            }
        }
        if matched == 0 {
            continue;
        }
        let coverage = matched as f64 / unique_query_terms.len().max(1) as f64;
        let structural_prior =
            scout_heading_score(&section.section_path, &section.section_title, question) as f64
                / 25.0;
        let path_prior = scout_path_quality_score(&section.path) as f64 / 20.0;
        let authority_prior =
            scout_source_authority_score(&section.path, &section.section_path, "", question) as f64
                / 15.0;
        let compactness = -(section.token_estimate as f64 / 900.0);
        let final_score = (score * (0.75 + coverage)
            + structural_prior
            + path_prior
            + authority_prior
            + compactness)
            * 100.0;
        scored.push((
            final_score.round() as i32,
            section.path,
            section.section_id,
            matched,
        ));
    }

    scored.sort_by(|lhs, rhs| {
        rhs.0
            .cmp(&lhs.0)
            .then(rhs.3.cmp(&lhs.3))
            .then(lhs.1.cmp(&rhs.1))
            .then(lhs.2.cmp(&rhs.2))
    });
    for (score, path, section_id, matched) in scored.into_iter().take(limit.max(1)) {
        candidates.push(ScoutCandidate {
            path,
            section_id,
            score,
            reason: format!("lexical relevance: {matched} query terms"),
        });
    }
    Ok(())
}

fn lexical_query_terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in lexical_terms(text) {
        if token.len() >= 3
            && !matches!(
                token.as_str(),
                "answer" | "doc" | "docs" | "file" | "markdown" | "readme" | "section"
            )
            && !out.contains(&token)
        {
            out.push(token);
        }
    }
    out
}

fn lexical_terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .filter_map(normalize_lexical_term)
        .collect()
}

fn normalize_lexical_term(raw: &str) -> Option<String> {
    let mut token = raw.trim().trim_matches('-').to_ascii_lowercase();
    if token.len() < 3 || is_stopword(&token) {
        return None;
    }
    if token.chars().all(|c| c.is_ascii_digit()) {
        return Some(token);
    }
    for suffix in ["ing", "edly", "edly", "ed", "es", "s"] {
        if token.len() > suffix.len() + 3 && token.ends_with(suffix) {
            token.truncate(token.len() - suffix.len());
            break;
        }
    }
    Some(token)
}

fn scout_heading_score(section_path: &[String], section_title: &str, question: &str) -> i32 {
    let question_l = question.to_ascii_lowercase();
    let heading_l = format!("{} {}", section_path.join(" "), section_title).to_ascii_lowercase();
    let mut score = 0;

    for token in signal_tokens(question).iter().take(8) {
        if heading_l.contains(&token.to_ascii_lowercase()) {
            score += 20;
        }
    }
    for (needle, heading, weight) in [
        ("install", "install", 90),
        ("command", "install", 45),
        ("usage", "usage", 70),
        ("example", "example", 55),
        ("configure", "configuration", 70),
        ("config", "configuration", 70),
        ("option", "option", 65),
        ("hyperparameter", "hyperparameter", 75),
        ("limitation", "limitation", 90),
        ("caveat", "caveat", 90),
        ("good fit", "for you", 130),
        ("compared", "for you", 90),
        ("yourself", "for you", 90),
        ("proxy", "proxy", 120),
        ("external", "external", 45),
        ("caveat", "finding", 50),
        ("caveat", "bottom line", 35),
        ("caveat", "unambiguous", 55),
        ("uniformly", "unambiguous", 55),
        ("conclude", "conclude", 70),
        ("conclude", "bottom line", 65),
        ("why", "finding", 35),
        ("why", "conclude", 35),
        ("analysis", "analysis", 45),
        ("failure", "failure", 55),
        ("recommend", "recommendation", 95),
        ("policy", "recommendation", 65),
        ("policy", "policy", 95),
        ("privacy", "privacy", 95),
        ("mask", "privacy", 75),
        ("masking", "privacy", 75),
        ("rule", "rule", 90),
        ("rules", "rule", 90),
        ("counting", "counting", 100),
        ("safety", "safety", 100),
        ("hazard", "safety", 75),
        ("hazard", "hazard", 85),
        ("risk", "risk", 80),
        ("why", "policy", 70),
        ("why", "rule", 70),
        ("why", "risk", 65),
        ("treat", "policy", 70),
        ("treat", "rule", 70),
        ("treat", "risk", 65),
        ("reflected", "policy", 65),
        ("reflection", "policy", 65),
        ("glare", "risk", 65),
        ("corrupted", "risk", 55),
        ("current", "current loader", 90),
        ("loader", "current loader", 90),
        ("flag", "current loader", 85),
        ("flag", "do not use", 75),
        ("stale", "do not use", 95),
        ("still", "do not use", 70),
        ("recommended", "current loader", 85),
        ("direction", "recommendation", 45),
    ] {
        if question_l.contains(needle) && heading_l.contains(heading) {
            score += weight;
        }
    }
    if (question_l.contains("hard") || question_l.contains("remains"))
        && heading_l.contains("ambiguity")
    {
        score += 80;
    }
    for (low_value, penalty) in [
        ("license", 70),
        ("citation", 80),
        ("cite", 80),
        ("contact", 55),
        ("contribute", 55),
        ("acknowledg", 55),
    ] {
        if heading_l.contains(low_value) && !question_l.contains(low_value) {
            score -= penalty;
        }
    }
    score
}

fn scout_path_quality_score(path: &str) -> i32 {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let mut score = 0;
    for marker in [
        "policy",
        "runbook",
        "guide",
        "manual",
        "spec",
        "reference",
        "card",
        "schema",
        "protocol",
    ] {
        if stem.contains(marker) {
            score += 45;
        }
    }
    for marker in [
        "scratch",
        "tmp",
        "temp",
        "draft",
        "random",
        "copied",
        "copy",
        "chat",
        "conversation",
    ] {
        if stem.contains(marker) {
            score -= 180;
        }
    }
    score
}

fn scout_source_authority_score(
    path: &str,
    section_path: &[String],
    content: &str,
    question: &str,
) -> i32 {
    let mut score = scout_path_quality_score(path);
    let question_l = question.to_ascii_lowercase();
    let heading_l = section_path.join(" ").to_ascii_lowercase();
    let content_l = content.to_ascii_lowercase();
    let combined = format!("{heading_l}\n{content_l}");

    for marker in [
        "source of truth",
        "current",
        "locked",
        "policy",
        "rule",
        "spec",
        "reference",
        "runbook",
        "known risk",
        "export notes",
        "current loader",
        "annotation policy",
    ] {
        if combined.contains(marker) {
            score += 28;
        }
    }

    let asks_for_informal = [
        "scratch",
        "draft",
        "old note",
        "old notes",
        "stale",
        "historical",
        "outdated",
        "do not use",
    ]
    .iter()
    .any(|needle| question_l.contains(needle));
    let low_authority_multiplier = if asks_for_informal { 1 } else { 2 };
    for (marker, penalty) in [
        ("not authoritative", 180),
        ("maybe stale", 140),
        ("random copied", 120),
        ("todo maybe", 110),
        ("scratch note", 100),
        ("copied wrong", 80),
        ("old notes disagree", 75),
    ] {
        if combined.contains(marker) {
            score -= penalty * low_authority_multiplier;
        }
    }

    score
}

fn wants_multi_file_evidence(question: &str) -> bool {
    let question_l = question.to_ascii_lowercase();
    [
        " across ",
        " between ",
        " compare ",
        " compares ",
        " comparing ",
        " contrast ",
        " both ",
        " each ",
        " multiple ",
        " multi-file ",
    ]
    .iter()
    .any(|needle| format!(" {question_l} ").contains(needle))
}

fn scout_semantic_queries(question: &str) -> Vec<String> {
    let question_l = question.to_ascii_lowercase();
    let mut queries = Vec::new();

    if question_l.contains("external") {
        queries.push("external".to_string());
        if question_l.contains("proxy") {
            queries.push("proxy".to_string());
        }
        if question_l.contains("panel") {
            queries.push("panel".to_string());
            queries.push("agreement".to_string());
        }
    }
    if question_l.contains("caveat")
        || question_l.contains("not specify")
        || question_l.contains("does not specify")
        || question_l.contains("uniformly")
    {
        queries.push("caveat".to_string());
        queries.push("not uniformly".to_string());
        queries.push("not specified".to_string());
    }
    if question_l.contains("compare")
        || question_l.contains("compared")
        || question_l.contains("difference")
        || question_l.contains("changed")
    {
        queries.push("compared".to_string());
        queries.push("difference".to_string());
    }
    if question_l.contains("best") && question_l.contains("candidate") {
        queries.push("best candidate".to_string());
    }
    if question_l.contains("failure") && question_l.contains("analysis") {
        queries.push("failure analysis".to_string());
    }
    if question_l.contains("recommend") || question_l.contains("policy direction") {
        queries.push("recommendation".to_string());
    }
    if question_l.contains("why")
        || question_l.contains("rule")
        || question_l.contains("policy")
        || question_l.contains("privacy")
        || question_l.contains("safety")
        || question_l.contains("hazard")
        || question_l.contains("counting")
        || question_l.contains("treat")
        || question_l.contains("reflected")
        || question_l.contains("reflection")
        || question_l.contains("glare")
        || question_l.contains("corrupted")
    {
        queries.push("policy".to_string());
        queries.push("rule".to_string());
        queries.push("known risk".to_string());
    }
    if question_l.contains("stale")
        || question_l.contains("current")
        || question_l.contains("recommended")
        || question_l.contains("still")
        || question_l.contains("flag")
        || question_l.contains("loader")
    {
        queries.push("current loader".to_string());
        queries.push("do not use".to_string());
        queries.push("stale flag".to_string());
    }

    queries
}

fn push_unique_query(queries: &mut Vec<String>, query: String) {
    let query = query
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string();
    if query.len() < 3 {
        return;
    }
    if is_stopword(&query) {
        return;
    }
    if !queries
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&query))
    {
        queries.push(query);
    }
}

fn clean_query_phrase(phrase: &str) -> String {
    phrase
        .split_whitespace()
        .filter_map(|token| {
            let cleaned =
                token.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '/');
            if cleaned.eq_ignore_ascii_case("readme") || is_stopword(cleaned) {
                None
            } else {
                Some(cleaned.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_capitalized_phrases(text: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for raw in text.split_whitespace() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '/');
        let is_signal = word
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            || word.chars().any(|c| c.is_ascii_digit())
            || word.contains('-')
            || word.contains('/');
        if is_signal && word.len() > 1 {
            current.push(word.to_string());
            if raw.ends_with(',') || raw.ends_with(';') {
                if current.len() >= 2 || current[0].len() >= 5 {
                    phrases.push(current.join(" "));
                }
                current.clear();
            }
        } else if !current.is_empty() {
            if current.len() >= 2 || current[0].len() >= 5 {
                phrases.push(current.join(" "));
            }
            current.clear();
        }
    }
    if !current.is_empty() && (current.len() >= 2 || current[0].len() >= 5) {
        phrases.push(current.join(" "));
    }
    phrases
}

fn signal_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
        let token = raw.trim().trim_matches('-');
        if token.len() < 3 {
            continue;
        }
        if is_stopword(token) {
            continue;
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(token))
        {
            out.push(token.to_string());
        }
    }
    out
}

fn is_stopword(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "about"
            | "according"
            | "added"
            | "after"
            | "against"
            | "answer"
            | "are"
            | "across"
            | "before"
            | "between"
            | "can"
            | "compared"
            | "complete"
            | "does"
            | "during"
            | "explain"
            | "fit"
            | "for"
            | "from"
            | "given"
            | "good"
            | "has"
            | "have"
            | "how"
            | "in"
            | "instead"
            | "into"
            | "its"
            | "list"
            | "provide"
            | "readme"
            | "row"
            | "run"
            | "should"
            | "than"
            | "that"
            | "the"
            | "their"
            | "there"
            | "these"
            | "they"
            | "this"
            | "toolbox"
            | "using"
            | "user"
            | "wants"
            | "what"
            | "when"
            | "where"
            | "which"
            | "while"
            | "with"
            | "without"
            | "would"
            | "yourself"
            | "and"
    )
}

fn add_path_match_candidates(
    root: &str,
    question: &str,
    candidates: &mut Vec<ScoutCandidate>,
) -> Result<()> {
    let files = discover_markdown_files(root)?;
    let question_tokens = signal_tokens(question);
    if question_tokens.is_empty() {
        return Ok(());
    }
    for path in files {
        let normalized = normalize_for_match(&path);
        let mut hits = 0;
        for token in &question_tokens {
            if normalized.contains(&normalize_for_match(token)) {
                hits += 1;
            }
        }
        let source_like_path = scout_path_quality_score(&path) > 0;
        let policy_or_multi_question = wants_multi_file_evidence(question)
            || question.to_ascii_lowercase().contains("why")
            || question.to_ascii_lowercase().contains("rule")
            || question.to_ascii_lowercase().contains("policy")
            || question.to_ascii_lowercase().contains("safety")
            || question.to_ascii_lowercase().contains("privacy");
        let required_hits = if source_like_path && policy_or_multi_question {
            1
        } else {
            2
        };
        if hits < required_hits {
            continue;
        }
        let parsed = load_markdown(&path)?;
        for section in parsed.doc.sections.iter().take(2) {
            candidates.push(ScoutCandidate {
                path: path.clone(),
                section_id: section.id.clone(),
                score: 240 + hits * 30,
                reason: "path/name match".to_string(),
            });
        }
        if let Some(best) = best_named_section(&parsed.doc.sections, question) {
            candidates.push(ScoutCandidate {
                path: path.clone(),
                section_id: best.id.clone(),
                score: 300
                    + hits * 45
                    + scout_path_quality_score(&path)
                    + scout_heading_score(&best.path, &best.title, question),
                reason: "path/name match + relevant heading".to_string(),
            });
        }
    }
    Ok(())
}

fn add_named_target_candidates(
    root: &str,
    question: &str,
    candidates: &mut Vec<ScoutCandidate>,
) -> Result<()> {
    let targets = target_phrases_from_question(question);
    if targets.len() < 2 {
        return Ok(());
    }

    for target in targets {
        let results = search_files(root, &target, false, false, 12, 2)?;
        let mut seen_files = HashSet::new();
        for result in results.into_iter().take(8) {
            let content_authority =
                scout_source_authority_score(&result.path, &result.section_path, "", question);
            candidates.push(ScoutCandidate {
                path: result.path.clone(),
                section_id: result.section_id.clone(),
                score: 620
                    + content_authority
                    + result.match_count as i32 * 20
                    + scout_heading_score(&result.section_path, &result.section_title, question),
                reason: format!("named target: {target}"),
            });

            if seen_files.insert(result.path.clone()) {
                let parsed = load_markdown(&result.path)?;
                if let Some(best) = best_named_section(&parsed.doc.sections, question) {
                    candidates.push(ScoutCandidate {
                        path: result.path.clone(),
                        section_id: best.id.clone(),
                        score: 760
                            + scout_source_authority_score(&result.path, &best.path, "", question)
                            + scout_heading_score(&best.path, &best.title, question),
                        reason: format!("named target + relevant heading: {target}"),
                    });
                }
            }
        }
    }
    Ok(())
}

fn normalize_for_match(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
}

fn best_named_section<'a>(sections: &'a [Section], question: &str) -> Option<&'a Section> {
    let mut best: Option<(&Section, i32)> = None;
    score_named_sections(sections, question, &mut best);
    best.map(|(section, _)| section)
}

fn score_named_sections<'a>(
    sections: &'a [Section],
    question: &str,
    best: &mut Option<(&'a Section, i32)>,
) {
    for section in sections {
        let title = section.title.to_ascii_lowercase();
        let mut score = 0;
        for (needle, weight) in [
            ("usage", 30),
            ("install", 30),
            ("quick", 20),
            ("example", 20),
            ("configuration", 20),
            ("training", 20),
            ("preprocess", 20),
            ("limitation", 25),
            ("caveat", 25),
            ("documentation", 10),
            ("overview", 10),
            ("policy", 120),
            ("privacy", 110),
            ("rule", 115),
            ("counting", 110),
            ("safety", 115),
            ("risk", 90),
            ("current", 75),
            ("loader", 75),
            ("stale", 75),
            ("do not use", 90),
        ] {
            if title.contains(needle) {
                score += weight;
            }
        }
        for token in signal_tokens(question).iter().take(8) {
            if title.contains(&token.to_ascii_lowercase()) {
                score += 25;
            }
        }
        if score > 0 && best.is_none_or(|(_, best_score)| score > best_score) {
            *best = Some((section, score));
        }
        score_named_sections(&section.children, question, best);
    }
}

fn add_neighbor_candidates(candidates: &mut Vec<ScoutCandidate>) -> Result<()> {
    let originals = candidates.to_vec();
    let mut by_file: HashMap<String, HashSet<String>> = HashMap::new();
    for candidate in &originals {
        by_file
            .entry(candidate.path.clone())
            .or_default()
            .insert(candidate.section_id.clone());
    }
    for (path, ids) in by_file {
        let parsed = load_markdown(&path)?;
        let flat = flatten_doc_sections(&parsed.doc.sections);
        for (idx, section) in flat.iter().enumerate() {
            if !ids.contains(&section.id) {
                continue;
            }
            let start = idx.saturating_sub(1);
            let end = (idx + 1).min(flat.len().saturating_sub(1));
            for neighbor in flat.iter().take(end + 1).skip(start) {
                if neighbor.id == section.id {
                    continue;
                }
                candidates.push(ScoutCandidate {
                    path: path.clone(),
                    section_id: neighbor.id.clone(),
                    score: 70,
                    reason: format!("neighbor of §{}", section.id),
                });
            }
        }
    }
    Ok(())
}

fn flatten_doc_sections(sections: &[Section]) -> Vec<&Section> {
    let mut out = Vec::new();
    collect_flat_sections(sections, &mut out);
    out.sort_by_key(|section| section.line_start);
    out
}

fn collect_flat_sections<'a>(sections: &'a [Section], out: &mut Vec<&'a Section>) {
    for section in sections {
        out.push(section);
        collect_flat_sections(&section.children, out);
    }
}

fn dedupe_scout_candidates(candidates: &mut Vec<ScoutCandidate>) {
    let mut seen = HashSet::new();
    candidates
        .retain(|candidate| seen.insert(format!("{}::{}", candidate.path, candidate.section_id)));
}

fn prune_parent_scout_candidates(candidates: &mut Vec<ScoutCandidate>) {
    let ids_by_file: HashMap<String, Vec<String>> =
        candidates
            .iter()
            .fold(HashMap::new(), |mut by_file, candidate| {
                by_file
                    .entry(candidate.path.clone())
                    .or_default()
                    .push(candidate.section_id.clone());
                by_file
            });

    candidates.retain(|candidate| {
        !ids_by_file.get(&candidate.path).is_some_and(|ids| {
            ids.iter()
                .any(|id| is_child_section_id(&candidate.section_id, id))
        })
    });
}

fn diversify_scout_candidates(
    candidates: &mut Vec<ScoutCandidate>,
    max_sections: usize,
    question: &str,
) {
    if !wants_multi_file_evidence(question) || candidates.len() <= max_sections {
        return;
    }

    let mut targets = target_phrases_from_question(question);
    if targets.len() < 2 {
        targets = target_tokens_from_question(question);
    }
    if let Some(selected) =
        target_coverage_scout_candidates(candidates, max_sections, &targets, question)
    {
        *candidates = selected;
        return;
    }

    let mut selected = Vec::new();
    let mut selected_keys = HashSet::new();
    let mut per_file_count: HashMap<String, usize> = HashMap::new();

    for candidate in candidates.iter() {
        if selected.len() >= max_sections {
            break;
        }
        let count = per_file_count.get(&candidate.path).copied().unwrap_or(0);
        if count >= 2 {
            continue;
        }
        let key = format!("{}::{}", candidate.path, candidate.section_id);
        if selected_keys.insert(key) {
            selected.push(candidate.clone());
            *per_file_count.entry(candidate.path.clone()).or_default() += 1;
        }
    }

    for candidate in candidates.iter() {
        if selected.len() >= max_sections {
            break;
        }
        let key = format!("{}::{}", candidate.path, candidate.section_id);
        if selected_keys.insert(key) {
            selected.push(candidate.clone());
        }
    }

    if selected.len() >= 2 {
        *candidates = selected;
    }
}

fn target_coverage_scout_candidates(
    candidates: &[ScoutCandidate],
    max_sections: usize,
    targets: &[String],
    question: &str,
) -> Option<Vec<ScoutCandidate>> {
    if targets.len() < 2 || max_sections == 0 {
        return None;
    }

    let mut cache: HashMap<String, crate::parse::ParsedMarkdown> = HashMap::new();
    let mut selected = Vec::new();
    let mut selected_keys = HashSet::new();
    let mut covered_targets: HashSet<String> = HashSet::new();
    let mut per_file_count: HashMap<String, usize> = HashMap::new();

    while selected.len() < max_sections {
        let mut best_idx = None;
        let mut best_score = i32::MIN;
        let mut best_new_targets = HashSet::new();

        for (idx, candidate) in candidates.iter().enumerate() {
            let key = format!("{}::{}", candidate.path, candidate.section_id);
            if selected_keys.contains(&key) {
                continue;
            }
            let Ok((target_hits, authority)) =
                scout_candidate_target_hits(candidate, targets, question, &mut cache)
            else {
                continue;
            };
            let new_targets = target_hits
                .difference(&covered_targets)
                .cloned()
                .collect::<HashSet<_>>();
            if new_targets.is_empty() && covered_targets.len() < targets.len() {
                continue;
            }
            let same_file_penalty =
                per_file_count.get(&candidate.path).copied().unwrap_or(0) as i32 * 160;
            let coverage_gain = new_targets.len() as i32 * 420 + target_hits.len() as i32 * 35;
            let score = candidate.score + authority + coverage_gain - same_file_penalty;
            if score > best_score {
                best_score = score;
                best_idx = Some(idx);
                best_new_targets = new_targets;
            }
        }

        let Some(idx) = best_idx else {
            break;
        };
        let candidate = candidates[idx].clone();
        let key = format!("{}::{}", candidate.path, candidate.section_id);
        selected_keys.insert(key);
        for target in best_new_targets {
            covered_targets.insert(target);
        }
        *per_file_count.entry(candidate.path.clone()).or_default() += 1;
        selected.push(candidate);

        if covered_targets.len() >= targets.len() {
            break;
        }
    }

    if selected.len() < 2 {
        return None;
    }

    for candidate in candidates {
        if selected.len() >= max_sections {
            break;
        }
        let key = format!("{}::{}", candidate.path, candidate.section_id);
        if selected_keys.contains(&key) {
            continue;
        }
        let Ok((_, authority)) =
            scout_candidate_target_hits(candidate, targets, question, &mut cache)
        else {
            continue;
        };
        if authority < -250 && selected.len() >= 2 {
            continue;
        }
        selected_keys.insert(key);
        selected.push(candidate.clone());
    }

    Some(selected)
}

fn ensure_named_target_coverage(
    selected: &mut Vec<ScoutCandidate>,
    pool: &[ScoutCandidate],
    max_sections: usize,
    question: &str,
) -> Result<()> {
    let targets = target_phrases_from_question(question);
    if targets.len() < 2 || max_sections == 0 {
        return Ok(());
    }

    let mut cache: HashMap<String, crate::parse::ParsedMarkdown> = HashMap::new();
    let mut selected_keys = selected
        .iter()
        .map(|candidate| format!("{}::{}", candidate.path, candidate.section_id))
        .collect::<HashSet<_>>();
    let mut covered = HashSet::new();
    for candidate in selected.iter() {
        let (hits, _) = scout_candidate_target_hits(candidate, &targets, question, &mut cache)?;
        covered.extend(hits);
    }

    for target in targets {
        if covered.contains(&target) {
            continue;
        }

        let mut best: Option<(ScoutCandidate, i32)> = None;
        for candidate in pool {
            let key = format!("{}::{}", candidate.path, candidate.section_id);
            if selected_keys.contains(&key) {
                continue;
            }
            let (hits, authority) = scout_candidate_target_hits(
                candidate,
                std::slice::from_ref(&target),
                question,
                &mut cache,
            )?;
            if hits.is_empty() {
                continue;
            }
            let score = candidate.score + authority;
            if best
                .as_ref()
                .is_none_or(|(_, best_score)| score > *best_score)
            {
                best = Some((candidate.clone(), score));
            }
        }

        let Some((candidate, _)) = best else {
            continue;
        };
        let key = format!("{}::{}", candidate.path, candidate.section_id);
        if selected.len() >= max_sections {
            selected.pop();
        }
        selected_keys.insert(key);
        covered.insert(target);
        selected.push(candidate);
    }

    Ok(())
}

fn scout_candidate_target_hits(
    candidate: &ScoutCandidate,
    targets: &[String],
    question: &str,
    cache: &mut HashMap<String, crate::parse::ParsedMarkdown>,
) -> Result<(HashSet<String>, i32)> {
    if !cache.contains_key(&candidate.path) {
        cache.insert(candidate.path.clone(), load_markdown(&candidate.path)?);
    }
    let parsed = cache.get(&candidate.path).expect("cached parsed markdown");
    let Some(section) = parsed.doc.find_section_by_id(&candidate.section_id) else {
        return Ok((HashSet::new(), scout_path_quality_score(&candidate.path)));
    };
    let content = section.extract_content(&parsed.lines).join("\n");
    let source_haystack =
        normalize_compact(&format!("{}\n{}", candidate.path, section.path.join(" ")));
    let haystack = normalize_compact(&format!(
        "{}\n{}\n{}",
        candidate.path,
        section.path.join(" "),
        content
    ));
    let hits = targets
        .iter()
        .filter(|target| haystack.contains(&normalize_compact(target)))
        .cloned()
        .collect::<HashSet<_>>();
    let source_hit_count = targets
        .iter()
        .filter(|target| source_haystack.contains(&normalize_compact(target)))
        .count() as i32;
    let mut authority =
        scout_source_authority_score(&candidate.path, &section.path, &content, question);
    authority += source_hit_count * 360;
    if source_hit_count == 0 && !hits.is_empty() {
        authority -= 120;
    }
    Ok((hits, authority))
}

fn is_child_section_id(parent: &str, child: &str) -> bool {
    child.len() > parent.len()
        && child.starts_with(parent)
        && child[parent.len()..].starts_with('.')
}

fn focused_scout_candidates(candidates: &[ScoutCandidate], question: &str) -> Vec<ScoutCandidate> {
    let Some(top) = candidates.first() else {
        return Vec::new();
    };
    if wants_multi_file_evidence(question) {
        let targets = target_tokens_from_question(question);
        if !targets.is_empty() {
            let focused = candidates
                .iter()
                .filter(|candidate| path_matches_any_target(&candidate.path, &targets))
                .cloned()
                .collect::<Vec<_>>();
            if focused.len() >= 2 {
                return focused;
            }
        }
        return candidates.to_vec();
    }
    let top_path_tokens = distinctive_path_tokens(&top.path);
    if scout_path_quality_score(&top.path) > 0 && !top_path_tokens.is_empty() {
        let focused = candidates
            .iter()
            .filter(|candidate| {
                candidate.path == top.path
                    || distinctive_path_tokens(&candidate.path)
                        .iter()
                        .any(|token| top_path_tokens.contains(token))
            })
            .cloned()
            .collect::<Vec<_>>();
        if focused.len() >= 2 {
            return focused;
        }
    }
    let best_other_score = candidates
        .iter()
        .find(|candidate| candidate.path != top.path)
        .map(|candidate| candidate.score);
    let dominant_file =
        top.score >= 280 && best_other_score.is_none_or(|score| top.score - score >= 80);
    if dominant_file {
        candidates
            .iter()
            .filter(|candidate| candidate.path == top.path)
            .cloned()
            .collect()
    } else {
        candidates.to_vec()
    }
}

fn order_scout_evidence(
    mut candidates: Vec<ScoutCandidate>,
    question: &str,
) -> Result<Vec<ScoutCandidate>> {
    let question_l = question.to_ascii_lowercase();
    if !wants_rationale_or_policy_evidence(&question_l) {
        return Ok(candidates);
    }

    let mut cache: HashMap<String, crate::parse::ParsedMarkdown> = HashMap::new();
    let mut scored = Vec::new();
    for (idx, candidate) in candidates.drain(..).enumerate() {
        if !cache.contains_key(&candidate.path) {
            cache.insert(candidate.path.clone(), load_markdown(&candidate.path)?);
        }
        let parsed = cache.get(&candidate.path).expect("cached parsed markdown");
        let score = parsed
            .doc
            .find_section_by_id(&candidate.section_id)
            .map(|section| {
                let content = section.extract_content(&parsed.lines).join("\n");
                candidate.score
                    + scout_rationale_evidence_score(&section.path, &content, &question_l)
            })
            .unwrap_or(candidate.score);
        scored.push((score, idx, candidate));
    }
    scored.sort_by(|lhs, rhs| rhs.0.cmp(&lhs.0).then(lhs.1.cmp(&rhs.1)));
    Ok(scored
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect())
}

fn wants_rationale_or_policy_evidence(question_l: &str) -> bool {
    [
        "why",
        "what makes",
        "rather than",
        "policy",
        "privacy",
        "safety",
        "allow",
        "allows",
        "exporting",
        "mask",
        "masking",
        "rationale",
        "reason",
    ]
    .iter()
    .any(|needle| question_l.contains(needle))
}

fn asks_for_metric_or_table(question_l: &str) -> bool {
    [
        "metric",
        "score",
        "baseline",
        "table",
        "row",
        "0.",
        "current score",
    ]
    .iter()
    .any(|needle| question_l.contains(needle))
}

fn scout_rationale_evidence_score(section_path: &[String], content: &str, question_l: &str) -> i32 {
    let text = format!("{}\n{}", section_path.join(" "), content).to_ascii_lowercase();
    let mut score = 0;
    score += scout_rationale_marker_score(&text);
    score += scout_question_token_overlap_score(&text, question_l, 28, 220);
    if !asks_for_metric_or_table(question_l) {
        for needle in [
            "metric | score",
            "| score |",
            "baseline",
            "current metric",
            "benchmark",
            "leaderboard",
        ] {
            if text.contains(needle) {
                score -= 220;
            }
        }
    }
    score
}

fn distinctive_path_tokens(path: &str) -> HashSet<String> {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    stem.split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|token| {
            token.len() >= 4
                && !matches!(
                    token.as_str(),
                    "readme"
                        | "index"
                        | "docs"
                        | "doc"
                        | "notes"
                        | "note"
                        | "eval"
                        | "scene"
                        | "card"
                        | "annotation"
                        | "policy"
                        | "scratch"
                        | "draft"
                        | "copy"
                        | "copied"
                        | "tmp"
                        | "temp"
                        | "anchor"
                )
        })
        .collect()
}

fn target_tokens_from_question(question: &str) -> Vec<String> {
    let mut out = Vec::new();
    for phrase in extract_capitalized_phrases(question) {
        for token in signal_tokens(&phrase) {
            for part in token.split('-') {
                let part = part.to_ascii_lowercase();
                if part.len() >= 4 && !is_stopword(&part) && !out.contains(&part) {
                    out.push(part);
                }
            }
        }
    }
    out
}

fn target_phrases_from_question(question: &str) -> Vec<String> {
    let mut out = Vec::new();
    for phrase in extract_capitalized_phrases(question) {
        if !phrase
            .chars()
            .any(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        {
            continue;
        }
        let tokens = signal_tokens(&phrase)
            .into_iter()
            .filter(|token| {
                !matches!(
                    token.to_ascii_lowercase().as_str(),
                    "compare" | "contrast" | "across" | "between" | "which"
                )
            })
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }
        let phrase = tokens.join(" ");
        if phrase.len() >= 4 && !out.iter().any(|existing| existing == &phrase) {
            out.push(phrase);
        }
    }
    out
}

#[cfg(test)]
mod scout_tests {
    use super::target_phrases_from_question;

    #[test]
    fn target_phrases_keep_hyphenated_entities() {
        let targets = target_phrases_from_question(
            "Across Harbor-17, Rainy Rail Depot, and Night Bus Stop, how do the docs treat reflected or glare-corrupted text?",
        );
        assert!(targets.contains(&"Harbor-17".to_string()), "{targets:?}");
        assert!(
            targets.contains(&"Rainy Rail Depot".to_string()),
            "{targets:?}"
        );
        assert!(
            targets.contains(&"Night Bus Stop".to_string()),
            "{targets:?}"
        );
    }
}

fn path_matches_any_target(path: &str, targets: &[String]) -> bool {
    let path_l = normalize_compact(path);
    targets
        .iter()
        .any(|target| path_l.contains(&normalize_compact(target)))
}

fn normalize_compact(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn render_scout_file_maps(
    out: &mut String,
    candidates: &[ScoutCandidate],
    max_files: usize,
) -> Result<()> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for candidate in candidates {
        if seen.insert(candidate.path.clone()) {
            files.push(candidate.path.clone());
        }
        if files.len() >= max_files {
            break;
        }
    }
    out.push_str("[files]\n");
    for path in files {
        let summaries = get_doc_section_summaries(&path)?;
        let picked: HashSet<&str> = candidates
            .iter()
            .filter(|c| c.path == path)
            .map(|c| c.section_id.as_str())
            .collect();
        let sections = summaries
            .iter()
            .filter(|(id, title)| title != "<preamble>" && picked.contains(id.as_str()))
            .map(|(id, title)| format!("§{} {}", id, title))
            .take(6)
            .collect::<Vec<_>>();
        let also = summaries
            .iter()
            .filter(|(id, title)| title != "<preamble>" && !picked.contains(id.as_str()))
            .take(6)
            .map(|(id, title)| format!("§{} {}", id, title))
            .collect::<Vec<_>>();
        out.push_str(&format!("- {}\n", path));
        if !sections.is_empty() {
            out.push_str(&format!("  picked: {}\n", sections.join(" · ")));
        }
        if !also.is_empty() {
            out.push_str(&format!("  also: {}\n", also.join(" · ")));
        }
    }
    Ok(())
}

fn render_scout_highlights(
    out: &mut String,
    candidates: &[ScoutCandidate],
    question: &str,
    max_lines: usize,
) -> Result<()> {
    let tokens: Vec<String> = signal_tokens(question)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    let question_l = question.to_ascii_lowercase();
    let wants_code = ["cli", "command", "install", "invoke"]
        .iter()
        .any(|needle| question_l.contains(needle));
    let mut emitted = 0usize;
    let mut seen = HashSet::new();
    let mut highlights = Vec::new();
    let mut cache: HashMap<String, crate::parse::ParsedMarkdown> = HashMap::new();

    for candidate in candidates {
        if !cache.contains_key(&candidate.path) {
            cache.insert(candidate.path.clone(), load_markdown(&candidate.path)?);
        }
        let parsed = cache.get(&candidate.path).expect("cached parsed markdown");
        let Some(section) = parsed.doc.find_section_by_id(&candidate.section_id) else {
            continue;
        };
        if is_low_value_section_for_question(section, &question_l) {
            continue;
        }
        let lines = section.extract_content(&parsed.lines);
        for (idx, line) in lines.iter().enumerate() {
            if emitted >= max_lines {
                break;
            }
            let trimmed = line.trim();
            let lower = trimmed.to_ascii_lowercase();
            if is_noisy_highlight_line(trimmed) && !is_relevant_table_line(trimmed, &tokens) {
                continue;
            }
            let token_hits = tokens.iter().filter(|token| lower.contains(*token)).count();
            let useful_code_line = trimmed.contains("--")
                || (wants_code
                    && (trimmed.contains('`')
                        || trimmed.starts_with("pip ")
                        || trimmed.starts_with("conda ")
                        || trimmed.starts_with("python ")
                        || trimmed.starts_with("git ")
                        || trimmed.starts_with("cmake ")
                        || trimmed.starts_with("make ")));
            let useful_table_line = is_relevant_table_line(trimmed, &tokens);
            if token_hits == 0 && !useful_code_line && !useful_table_line {
                continue;
            }
            let mut score = token_hits as i32 * 20;
            if useful_table_line {
                score += 80;
            }
            if wants_rationale_or_policy_evidence(&question_l) {
                score += scout_rationale_highlight_score(&lower, &question_l);
            }
            for (needle, weight) in [
                ("--", 70),
                ("cpu", 45),
                ("gpu", 45),
                ("warning", 45),
                ("disable", 45),
                ("configuration", 30),
                ("header", 30),
                ("human-readable", 30),
                ("supported formats", 30),
                ("convert", 30),
            ] {
                if lower.contains(needle) {
                    score += weight;
                }
            }
            highlights.push(ScoutHighlight {
                score,
                path: candidate.path.clone(),
                section_id: section.id.clone(),
                line_no: section.line_start + idx,
                line: if useful_table_line {
                    scout_table_context(lines, idx)
                } else {
                    scout_highlight_context(lines, idx, &lower)
                },
            });
        }
    }

    highlights.sort_by(|lhs, rhs| {
        rhs.score
            .cmp(&lhs.score)
            .then(lhs.path.cmp(&rhs.path))
            .then(lhs.line_no.cmp(&rhs.line_no))
    });
    for highlight in highlights {
        if emitted >= max_lines {
            break;
        }
        emit_scout_highlight(out, &mut seen, &mut emitted, &highlight);
    }

    if emitted == 0 {
        out.push_str("- no compact highlights; read evidence sections below\n");
    }
    Ok(())
}

fn scout_rationale_highlight_score(lower: &str, question_l: &str) -> i32 {
    let mut score = 0;
    score += scout_rationale_marker_score(lower) / 2;
    score += scout_question_token_overlap_score(lower, question_l, 18, 120);
    if !asks_for_metric_or_table(question_l) {
        for needle in ["| score |", "baseline", "current metric", "benchmark", "0."] {
            if lower.contains(needle) {
                score -= 140;
            }
        }
    }
    score
}

fn scout_rationale_marker_score(lower: &str) -> i32 {
    let mut score = 0;
    for (needles, weight) in [
        (
            &["rule:", "rule ", "policy", "guideline", "standard"][..],
            180,
        ),
        (
            &[
                "known risk",
                "risk",
                "unsafe",
                "wrong answer",
                "misread",
                "confus",
                "ambiguous",
            ][..],
            160,
        ),
        (
            &[
                "privacy",
                "personal data",
                "identifiable",
                "redact",
                "mask",
                "export",
                "leak",
            ][..],
            150,
        ),
        (
            &[
                "must",
                "should",
                "requires",
                "allow",
                "not enough",
                "do not",
                "rather than",
            ][..],
            100,
        ),
        (
            &["because", "reason", "rationale", "therefore", "so that"][..],
            80,
        ),
    ] {
        if needles.iter().any(|needle| lower.contains(needle)) {
            score += weight;
        }
    }
    score
}

fn scout_question_token_overlap_score(
    lower: &str,
    question_l: &str,
    per_token: i32,
    cap: i32,
) -> i32 {
    let hits = signal_tokens(question_l)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| lower.contains(token))
        .count() as i32;
    (hits * per_token).min(cap)
}

fn is_noisy_highlight_line(line: &str) -> bool {
    line.is_empty()
        || line.starts_with('|')
        || line == "```"
        || line == "```shell"
        || line.trim_matches('~') == "```"
        || line.trim_matches('~') == "```shell"
        || line.starts_with("<!--")
        || line.starts_with("[!")
        || line.starts_with("![")
        || line.starts_with("[![")
        || line.starts_with("@article")
        || line.starts_with("@inproceedings")
        || (line.starts_with('[') && line.contains("]: "))
        || line.len() > 1000
}

fn is_relevant_table_line(line: &str, tokens: &[String]) -> bool {
    line.starts_with('|')
        && line.matches('|').count() >= 3
        && !is_table_separator_line(line)
        && tokens
            .iter()
            .any(|token| line.to_ascii_lowercase().contains(token))
}

fn is_table_separator_line(line: &str) -> bool {
    line.chars()
        .all(|ch| ch == '|' || ch == '-' || ch == ':' || ch.is_whitespace())
}

fn scout_table_context(lines: &[String], idx: usize) -> String {
    let row = lines[idx].trim();
    let header = (1..idx).rev().find_map(|candidate_idx| {
        let separator = lines[candidate_idx].trim();
        if !separator.starts_with('|') || !is_table_separator_line(separator) {
            return None;
        }
        let header = lines[candidate_idx - 1].trim();
        header.starts_with('|').then_some(header)
    });

    match header {
        Some(header) if header != row => format!("{header} => {row}"),
        _ => row.to_string(),
    }
}

fn scout_highlight_context(lines: &[String], idx: usize, lower: &str) -> String {
    let radius = if lower.contains("disable") || lower.contains("warning") {
        5
    } else if lines[idx].trim().len() < 300 {
        2
    } else {
        0
    };
    let start = idx.saturating_sub(radius);
    let end = (idx + radius).min(lines.len().saturating_sub(1));
    let mut parts = Vec::new();
    for line in &lines[start..=end] {
        let trimmed = line.trim();
        if is_noisy_highlight_line(trimmed) && !trimmed.starts_with('|') {
            continue;
        }
        parts.push(trimmed);
    }
    let mut joined = parts.join(" ");
    if joined.len() > 900 {
        joined.truncate(900);
        joined.push_str("...");
    }
    joined
}

fn is_low_value_section_for_question(section: &Section, question_l: &str) -> bool {
    let section_path = section.path.join(" ").to_ascii_lowercase();
    let citation_section = section_path.contains("citation")
        || section_path.contains("cite")
        || section_path.contains("references");
    citation_section
        && !["citation", "cite", "doi", "reference", "paper"]
            .iter()
            .any(|needle| question_l.contains(needle))
}

fn emit_scout_highlight(
    out: &mut String,
    seen: &mut HashSet<String>,
    emitted: &mut usize,
    highlight: &ScoutHighlight,
) {
    let key = format!(
        "{}:{}:{}",
        highlight.path, highlight.line_no, highlight.line
    );
    if !seen.insert(key) {
        return;
    }
    out.push_str(&format!(
        "- {} §{} l{}: {}\n",
        highlight.path, highlight.section_id, highlight.line_no, highlight.line
    ));
    *emitted += 1;
}

fn render_scout_evidence(
    out: &mut String,
    candidates: &[ScoutCandidate],
    question: &str,
    max_tokens: usize,
) -> Result<()> {
    let mut total_tokens = 0usize;
    let mut cache: HashMap<String, crate::parse::ParsedMarkdown> = HashMap::new();
    let mut emitted_ranges: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let question_l = question.to_ascii_lowercase();
    for candidate in candidates {
        if total_tokens >= max_tokens {
            out.push_str("\n<!-- mdlens: scout budget exhausted -->\n");
            break;
        }
        if !cache.contains_key(&candidate.path) {
            cache.insert(candidate.path.clone(), load_markdown(&candidate.path)?);
        }
        let parsed = cache.get(&candidate.path).expect("cached parsed markdown");
        let Some(section) = parsed.doc.find_section_by_id(&candidate.section_id) else {
            continue;
        };
        if is_low_value_section_for_question(section, &question_l) {
            continue;
        }
        let ranges = emitted_ranges.entry(candidate.path.clone()).or_default();
        if ranges.iter().any(|(start, end)| {
            section.line_start <= *start
                && section.line_end >= *end
                && (section.line_end - section.line_start) > (*end - *start)
        }) {
            continue;
        }
        let remaining = max_tokens.saturating_sub(total_tokens);
        let section_budget = remaining.min(650);
        let ancestors = section_ancestors(&parsed.doc.sections, &section.id);
        let (content, truncated) =
            scout_section_content(section, &ancestors, &parsed.lines, question, section_budget);
        let emitted_tokens = estimate_tokens(&content);
        if emitted_tokens == 0 {
            continue;
        }
        out.push_str(&format!(
            "\n--- {} §{} {} l{}-{} ~{}t reason={} ---\n",
            candidate.path,
            section.id,
            section.path.join(" > "),
            section.line_start,
            section.line_end,
            section.token_estimate,
            candidate.reason
        ));
        out.push_str(&content);
        if !content.ends_with('\n') {
            out.push('\n');
        }
        ranges.push((section.line_start, section.line_end));
        total_tokens += emitted_tokens;
        if truncated {
            continue;
        }
    }
    Ok(())
}

fn scout_section_content(
    section: &Section,
    ancestors: &[&Section],
    lines: &[String],
    question: &str,
    max_tokens: usize,
) -> (String, bool) {
    let parent_context = scout_parent_context(ancestors, lines, max_tokens.min(220));
    let content_lines = section.extract_content(lines);
    let full = content_lines.join("\n");
    let full_with_context = if parent_context.trim().is_empty() {
        full.clone()
    } else {
        format!("{parent_context}\n...\n{full}")
    };
    let full_tokens = estimate_tokens(&full_with_context);
    if full_tokens <= max_tokens {
        return (full_with_context, false);
    }

    let focused_budget = max_tokens
        .saturating_sub(estimate_tokens(&parent_context))
        .max(max_tokens / 2);
    let focused = scout_focused_excerpt(content_lines, question, focused_budget);
    if !focused.trim().is_empty() {
        if parent_context.trim().is_empty() {
            return (focused, true);
        }
        return (format!("{parent_context}\n...\n{focused}"), true);
    }

    (
        truncate_to_tokens(&full_with_context, max_tokens, TRUNCATION_NOTICE),
        true,
    )
}

fn section_ancestors<'a>(sections: &'a [Section], target_id: &str) -> Vec<&'a Section> {
    let mut path = Vec::new();
    collect_section_ancestors(sections, target_id, &mut path);
    path
}

fn collect_section_ancestors<'a>(
    sections: &'a [Section],
    target_id: &str,
    path: &mut Vec<&'a Section>,
) -> bool {
    for section in sections {
        if section.id == target_id {
            return true;
        }
        path.push(section);
        if collect_section_ancestors(&section.children, target_id, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn scout_parent_context(ancestors: &[&Section], lines: &[String], max_tokens: usize) -> String {
    if ancestors.is_empty() || max_tokens == 0 {
        return String::new();
    }

    let mut parts = Vec::new();
    for ancestor in ancestors {
        let direct = ancestor.extract_direct_content(lines);
        let cleaned = direct
            .iter()
            .map(|line| line.trim_end())
            .filter(|line| !line.trim().is_empty() && !is_noisy_highlight_line(line.trim()))
            .collect::<Vec<_>>()
            .join("\n");
        if cleaned.trim().is_empty() {
            continue;
        }
        parts.push(cleaned);
    }

    let joined = parts.join("\n");
    if estimate_tokens(&joined) <= max_tokens {
        joined
    } else {
        truncate_to_tokens(&joined, max_tokens, TRUNCATION_NOTICE)
    }
}

fn scout_focused_excerpt(lines: &[String], question: &str, max_tokens: usize) -> String {
    let tokens: Vec<String> = signal_tokens(question)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    let question_l = question.to_ascii_lowercase();
    let wants_code = ["cli", "command", "install", "invoke"]
        .iter()
        .any(|needle| question_l.contains(needle));

    let mut selected = BTreeSet::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if is_noisy_highlight_line(trimmed) && !is_relevant_table_line(trimmed, &tokens) {
            continue;
        }
        let token_hits = tokens.iter().filter(|token| lower.contains(*token)).count();
        let code_hit = trimmed.contains("--")
            || (wants_code
                && (trimmed.contains('`')
                    || trimmed.starts_with("pip ")
                    || trimmed.starts_with("conda ")
                    || trimmed.starts_with("python ")
                    || trimmed.starts_with("git ")
                    || trimmed.starts_with("cmake ")
                    || trimmed.starts_with("make ")));
        let table_hit = is_relevant_table_line(trimmed, &tokens);
        if token_hits == 0 && !code_hit && !table_hit {
            continue;
        }
        let radius = if table_hit {
            2
        } else if lower.contains("disable") || lower.contains("warning") || code_hit {
            5
        } else if token_hits >= 2 {
            2
        } else {
            1
        };
        for context_idx in idx.saturating_sub(radius)..=(idx + radius).min(lines.len() - 1) {
            selected.insert(context_idx);
        }
    }

    let mut out = String::new();
    let mut last_idx = None;
    for idx in selected {
        let line = lines[idx].trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(last) = last_idx {
            if idx > last + 1 && !out.ends_with("\n...\n") {
                out.push_str("...\n");
            }
        }
        let candidate = format!("{out}{line}\n");
        if estimate_tokens(&candidate) > max_tokens {
            out.push_str(TRUNCATION_NOTICE);
            break;
        }
        out = candidate;
        last_idx = Some(idx);
    }

    out
}

fn cmd_pack(args: PackArgs) -> Result<()> {
    let dedupe = args.dedupe && !args.no_dedupe;
    let result = if let Some(ref ids_str) = args.ids {
        let ids: Vec<String> = ids_str.split(',').map(|s| s.trim().to_string()).collect();
        pack_by_ids(&args.path, &ids, args.max_tokens, args.parents, dedupe)?
    } else if let Some(ref paths_str) = args.paths {
        let doc = parse_markdown(&args.path)?;
        let path_list: Vec<&str> = paths_str.split(';').collect();
        let mut ids = Vec::new();
        for p in path_list {
            ids.push(find_unique_section_by_path(&doc, p)?.id.clone());
        }
        pack_by_ids(&args.path, &ids, args.max_tokens, args.parents, dedupe)?
    } else if let Some(ref query) = args.search {
        crate::pack::pack_by_search(
            &args.path,
            query,
            args.max_tokens,
            PackSearchOptions {
                include_parents: args.parents,
                dedupe,
                case_sensitive: args.case_sensitive,
                use_regex: args.regex,
                max_results: args.max_results,
                context_lines: args.context_lines,
            },
        )?
    } else {
        return Err(anyhow::anyhow!(
            "exactly one of --ids, --paths, or --search is required"
        ));
    };

    if args.json {
        let output = PackJsonOutput {
            schema_version: 1,
            token_budget: result.token_budget,
            token_estimate: result.token_estimate,
            truncated: result.truncated,
            included: result
                .included
                .iter()
                .map(|inc| PackJsonIncluded {
                    path: inc.path.clone(),
                    section_id: inc.section_id.clone(),
                    section_path: inc.section_path.clone(),
                    line_start: inc.line_start,
                    line_end: inc.line_end,
                    token_estimate: inc.token_estimate,
                    truncated: inc.truncated,
                })
                .collect(),
            content: result.content.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let included_render: Vec<PackIncluded> = result
            .included
            .iter()
            .map(|inc| PackIncluded {
                section_id: inc.section_id.clone(),
                section_title: inc.section_path.last().cloned().unwrap_or_default(),
                line_range: format!("{}-{}", inc.line_start, inc.line_end),
                token_estimate: inc.token_estimate,
            })
            .collect();
        println!(
            "{}",
            render_pack(
                &args.path,
                result.token_budget,
                &included_render,
                &result.content,
                result.truncated
            )
        );
    }

    Ok(())
}

fn cmd_stats(args: StatsArgs) -> Result<()> {
    let files = crate::search::discover_markdown_files(&args.path)?;
    let mut entries = Vec::new();

    for file in &files {
        let doc = parse_markdown(file)?;
        entries.push(StatsEntry {
            path: doc.path,
            lines: doc.line_count,
            words: doc.word_count,
            tokens: doc.token_estimate,
        });
    }

    // Sort
    match args.sort {
        StatsSort::Tokens => entries.sort_by_key(|entry| Reverse(entry.tokens)),
        StatsSort::Lines => entries.sort_by_key(|entry| Reverse(entry.lines)),
        StatsSort::Path => entries.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path)),
    }

    // Apply top limit
    let entries = if let Some(top) = args.top {
        &entries[..std::cmp::min(top, entries.len())]
    } else {
        &entries
    };

    if args.json {
        let output = StatsJsonOutput {
            schema_version: 1,
            entries: entries
                .iter()
                .map(|e| StatsJsonEntry {
                    path: e.path.clone(),
                    lines: e.lines,
                    words: e.words,
                    tokens: e.tokens,
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", render_stats(entries));
    }

    Ok(())
}

fn cmd_sections(args: SectionsArgs) -> Result<()> {
    let stdin = io::stdin();
    let mut inputs: Vec<SectionInput> = Vec::new();

    // Read from stdin only when it is not a tty (i.e. piped input)
    if !args.files.is_empty() {
        // Positional args provided — use those, skip stdin
        for f in &args.files {
            let trimmed = f.trim().to_string();
            if !trimmed.is_empty() {
                inputs.push(SectionInput::File(trimmed));
            }
        }
    } else {
        for line in stdin.lock().lines() {
            let line = line?;
            if let Some(input) = parse_sections_input_line(&line) {
                inputs.push(input);
            }
        }
    }

    if inputs.is_empty() {
        return Ok(());
    }

    let dedupe = args.dedupe && !args.no_dedupe;
    let has_hit_input = inputs
        .iter()
        .any(|input| matches!(input, SectionInput::Hit(_)));

    if !has_hit_input {
        let mut paths: Vec<String> = inputs
            .into_iter()
            .filter_map(|input| match input {
                SectionInput::File(path) => Some(path),
                SectionInput::Hit(_) => None,
            })
            .collect();

        if dedupe {
            let mut seen = HashSet::new();
            paths.retain(|p| seen.insert(p.clone()));
        }

        return render_sections_from_paths(args, paths);
    }

    let mut file_order: Vec<String> = Vec::new();
    let mut file_hits: HashMap<String, Vec<usize>> = HashMap::new();

    for input in inputs {
        match input {
            SectionInput::File(path) => {
                if !file_order.iter().any(|existing| existing == &path) {
                    file_order.push(path.clone());
                }
                file_hits.entry(path).or_default();
            }
            SectionInput::Hit(hit) => {
                let entry = file_hits.entry(hit.path.clone()).or_default();
                if !dedupe || !entry.contains(&hit.line) {
                    entry.push(hit.line);
                }
                if !file_order.iter().any(|existing| existing == &hit.path) {
                    file_order.push(hit.path);
                }
            }
        }
    }

    if let Some(max_files) = args.max_files {
        if file_order.len() > max_files {
            anyhow::bail!(
                "[error] {} files exceed --max-files {}; narrow with a more specific grep or raise the limit",
                file_order.len(),
                max_files
            );
        }
    } else if args.max_tokens.is_none() && file_order.len() > 8 {
        eprintln!(
            "[warn] {} files piped without --max-tokens or --max-files; output may be large",
            file_order.len()
        );
    }

    let mut file_outputs: Vec<SectionsFileOutput> = Vec::new();
    let mut total_tokens: usize = 0;
    let mut omitted: usize = 0;

    for path in &file_order {
        let parsed = match load_markdown(path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: could not read {}: {}", path, e);
                continue;
            }
        };

        let doc = &parsed.doc;
        let lines = &parsed.lines;

        let mut sections: Vec<SectionsSectionOutput> =
            if let Some(hit_lines) = file_hits.get(path).filter(|lines| !lines.is_empty()) {
                collect_hit_sections(
                    &doc.sections,
                    lines,
                    hit_lines,
                    args.children,
                    args.preview,
                    dedupe,
                )
            } else {
                let mut collected = Vec::new();
                collect_all_sections(
                    &doc.sections,
                    lines,
                    args.children,
                    args.preview,
                    args.max_depth,
                    0,
                    &mut collected,
                );
                collected
            };

        if sections.is_empty() {
            continue;
        }

        if let Some(max_sections) = args.max_sections {
            if sections.len() > max_sections {
                omitted += sections.len() - max_sections;
                sections.truncate(max_sections);
            }
        }

        // Apply max-tokens cap
        if let Some(max_tokens) = args.max_tokens {
            let mut kept: Vec<SectionsSectionOutput> = Vec::new();
            for sec in sections {
                if total_tokens + sec.token_estimate > max_tokens {
                    omitted += 1;
                } else {
                    total_tokens += sec.token_estimate;
                    kept.push(sec);
                }
            }
            sections = kept;
        }

        if !sections.is_empty() {
            file_outputs.push(SectionsFileOutput {
                path: path.clone(),
                sections,
            });
        }
    }

    emit_sections_output(&args, file_outputs, omitted)
}

fn render_sections_from_paths(args: SectionsArgs, paths: Vec<String>) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    let depth_capped = args.max_depth.is_none() && (!args.content || args.preview.is_some());
    let effective_depth = if depth_capped {
        Some(2)
    } else {
        args.max_depth
    };

    if let Some(max_files) = args.max_files {
        if paths.len() > max_files {
            anyhow::bail!(
                "[error] {} files exceed --max-files {}; narrow with a more specific grep or raise the limit",
                paths.len(),
                max_files
            );
        }
    } else if args.max_tokens.is_none() && paths.len() > 8 {
        eprintln!(
            "[warn] {} files piped without --max-tokens or --max-files; output may be large",
            paths.len()
        );
    }

    let mut file_outputs: Vec<SectionsFileOutput> = Vec::new();
    let mut total_tokens: usize = 0;
    let mut omitted: usize = 0;

    for path in &paths {
        let parsed = match load_markdown(path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: could not read {}: {}", path, e);
                continue;
            }
        };

        let doc = &parsed.doc;
        let lines = &parsed.lines;
        let mut sections: Vec<SectionsSectionOutput> = Vec::new();
        collect_all_sections(
            &doc.sections,
            lines,
            args.children,
            args.preview,
            effective_depth,
            0,
            &mut sections,
        );

        if sections.is_empty() {
            continue;
        }

        if let Some(max_sections) = args.max_sections {
            if sections.len() > max_sections {
                omitted += sections.len() - max_sections;
                sections.truncate(max_sections);
            }
        }

        if let Some(max_tokens) = args.max_tokens {
            let mut kept: Vec<SectionsSectionOutput> = Vec::new();
            for sec in sections {
                if total_tokens + sec.token_estimate > max_tokens {
                    omitted += 1;
                } else {
                    total_tokens += sec.token_estimate;
                    kept.push(sec);
                }
            }
            sections = kept;
        }

        if !sections.is_empty() {
            file_outputs.push(SectionsFileOutput {
                path: path.clone(),
                sections,
            });
        }
    }

    if depth_capped {
        eprintln!(
            "[sections] whole-file mode: showing depth ≤2 by default; use --max-depth N for more"
        );
    }

    emit_sections_output(&args, file_outputs, omitted)
}

fn emit_sections_output(
    args: &SectionsArgs,
    file_outputs: Vec<SectionsFileOutput>,
    omitted: usize,
) -> Result<()> {
    if omitted > 0 {
        if let Some(max_tokens) = args.max_tokens {
            eprintln!(
                "[warn] {} sections omitted by limits (budget ~{}t)",
                omitted, max_tokens
            );
        } else {
            eprintln!("[warn] {} sections omitted by limits", omitted);
        }
    }

    if file_outputs.is_empty() {
        return Ok(());
    }

    if args.json {
        let output = SectionsJsonOutput {
            schema_version: 1,
            files: file_outputs
                .iter()
                .map(|fo| SectionsJsonFile {
                    path: fo.path.clone(),
                    sections: fo
                        .sections
                        .iter()
                        .map(|s| SectionsJsonSection {
                            id: s.id.clone(),
                            title: s.title.clone(),
                            heading_path: if args.heading_paths {
                                Some(s.heading_path.clone())
                            } else {
                                None
                            },
                            line_start: if args.lines { Some(s.line_start) } else { None },
                            line_end: if args.lines { Some(s.line_end) } else { None },
                            token_estimate: s.token_estimate,
                            body: if args.content {
                                Some(s.body.clone())
                            } else {
                                None
                            },
                            preview: s.preview.clone(),
                        })
                        .collect(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let entries: Vec<SectionsEntry> = file_outputs
            .iter()
            .flat_map(|fo| {
                fo.sections.iter().map(|s| SectionsEntry {
                    file_path: fo.path.clone(),
                    id: s.id.clone(),
                    title: s.title.clone(),
                    heading_path: if args.heading_paths {
                        Some(s.heading_path.clone())
                    } else {
                        None
                    },
                    line_start: if args.lines { Some(s.line_start) } else { None },
                    line_end: if args.lines { Some(s.line_end) } else { None },
                    token_estimate: s.token_estimate,
                    body: if args.content {
                        Some(s.body.clone())
                    } else {
                        None
                    },
                    preview: s.preview.clone(),
                })
            })
            .collect();
        println!("{}", render_sections(&entries, args.content));
    }

    Ok(())
}

struct SectionsSectionOutput {
    id: String,
    title: String,
    heading_path: Vec<String>,
    line_start: usize,
    line_end: usize,
    token_estimate: usize,
    body: String,
    preview: Option<String>,
}

struct SectionsFileOutput {
    path: String,
    sections: Vec<SectionsSectionOutput>,
}

#[derive(Clone)]
struct HitSectionAggregate<'a> {
    section: &'a Section,
    hit_count: usize,
    first_line: usize,
}

fn parse_sections_input_line(line: &str) -> Option<SectionInput> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((path, line_num)) = parse_grep_hit(trimmed) {
        return Some(SectionInput::Hit(SectionHit {
            path: path.to_string(),
            line: line_num,
        }));
    }

    Some(SectionInput::File(trimmed.to_string()))
}

fn parse_grep_hit(line: &str) -> Option<(&str, usize)> {
    let first = line.find(':')?;
    let rest = &line[(first + 1)..];
    let second = rest.find(':')?;
    let path = &line[..first];
    let line_num = rest[..second].parse().ok()?;
    Some((path, line_num))
}

fn collect_hit_sections(
    sections: &[Section],
    lines: &[String],
    hit_lines: &[usize],
    include_children: bool,
    preview_lines: Option<usize>,
    dedupe: bool,
) -> Vec<SectionsSectionOutput> {
    let mut by_section: HashMap<String, HitSectionAggregate<'_>> = HashMap::new();
    let mut ordered_hits: Vec<(usize, &Section)> = Vec::new();

    for line_num in hit_lines {
        let Some(section) = find_deepest_section_for_line(sections, *line_num) else {
            continue;
        };
        if dedupe {
            by_section
                .entry(section.id.clone())
                .and_modify(|entry| entry.hit_count += 1)
                .or_insert(HitSectionAggregate {
                    section,
                    hit_count: 1,
                    first_line: *line_num,
                });
        } else {
            ordered_hits.push((*line_num, section));
        }
    }

    let aggregates: Vec<HitSectionAggregate<'_>> = if dedupe {
        let mut ranked: Vec<HitSectionAggregate<'_>> = by_section.into_values().collect();
        ranked.sort_by(|lhs, rhs| {
            rhs.hit_count
                .cmp(&lhs.hit_count)
                .then(lhs.section.token_estimate.cmp(&rhs.section.token_estimate))
                .then(lhs.first_line.cmp(&rhs.first_line))
                .then(lhs.section.line_start.cmp(&rhs.section.line_start))
        });
        ranked
    } else {
        ordered_hits.sort_by(|lhs, rhs| {
            lhs.0
                .cmp(&rhs.0)
                .then(lhs.1.line_start.cmp(&rhs.1.line_start))
                .then(lhs.1.id.cmp(&rhs.1.id))
        });
        ordered_hits
            .into_iter()
            .map(|(first_line, section)| HitSectionAggregate {
                section,
                hit_count: 1,
                first_line,
            })
            .collect()
    };

    let mut collected = Vec::new();
    for aggregate in aggregates {
        let section = aggregate.section;
        let body_lines = if include_children {
            section.extract_content(lines)
        } else {
            section.extract_direct_content(lines)
        };
        let body = body_lines.join("\n");
        let preview = preview_lines.map(|n| {
            body_lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .take(n)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        });

        collected.push(SectionsSectionOutput {
            id: section.id.clone(),
            title: section.title.clone(),
            heading_path: section.path.clone(),
            line_start: section.line_start,
            line_end: section.line_end,
            token_estimate: estimate_tokens(&body),
            body,
            preview,
        });
    }

    collected
}

fn collect_all_sections(
    sections: &[Section],
    lines: &[String],
    include_children: bool,
    preview_lines: Option<usize>,
    max_depth: Option<usize>,
    current_depth: usize,
    result: &mut Vec<SectionsSectionOutput>,
) {
    for section in sections {
        if section.title == "<preamble>" {
            continue;
        }
        if let Some(max) = max_depth {
            if current_depth >= max {
                continue;
            }
        }
        let body_lines = if include_children {
            section.extract_content(lines)
        } else {
            section.extract_direct_content(lines)
        };
        let body = body_lines.join("\n");
        let preview = preview_lines.map(|n| {
            body_lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .take(n)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        });
        result.push(SectionsSectionOutput {
            id: section.id.clone(),
            title: section.title.clone(),
            heading_path: section.path.clone(),
            line_start: section.line_start,
            line_end: section.line_end,
            token_estimate: estimate_tokens(&body),
            body,
            preview,
        });
        collect_all_sections(
            &section.children,
            lines,
            include_children,
            preview_lines,
            max_depth,
            current_depth + 1,
            result,
        );
    }
}

fn enrich_search_results(
    results: &mut [crate::render::SearchResult],
    with_content: bool,
    preview_lines: Option<usize>,
) -> Result<()> {
    let mut docs: HashMap<String, crate::parse::ParsedMarkdown> = HashMap::new();

    for result in results.iter_mut() {
        let parsed = if let Some(parsed) = docs.get(&result.path) {
            parsed
        } else {
            let loaded = load_markdown(&result.path)?;
            docs.insert(result.path.clone(), loaded);
            docs.get(&result.path).expect("inserted parsed markdown")
        };

        let Some(section) = parsed.doc.find_section_by_id(&result.section_id) else {
            continue;
        };
        let body_lines = section.extract_direct_content(&parsed.lines);
        if with_content {
            result.body = Some(body_lines.join("\n"));
        }
        if let Some(n) = preview_lines {
            result.preview = Some(
                body_lines
                    .iter()
                    .filter(|line| !line.trim().is_empty())
                    .take(n)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    Ok(())
}

fn find_deepest_section_for_line(sections: &[Section], line_num: usize) -> Option<&Section> {
    for section in sections {
        if line_num < section.line_start || line_num > section.line_end {
            continue;
        }
        if let Some(child) = find_deepest_section_for_line(&section.children, line_num) {
            return Some(child);
        }
        return Some(section);
    }
    None
}

// --- JSON output types ---

#[derive(Serialize)]
struct TreeJsonOutput {
    schema_version: u32,
    path: String,
    line_count: usize,
    byte_count: usize,
    char_count: usize,
    word_count: usize,
    token_estimate: usize,
    sections: Vec<SectionJsonOutput>,
}

#[derive(Serialize)]
struct TreeFileJsonOutput {
    path: String,
    line_count: usize,
    byte_count: usize,
    char_count: usize,
    word_count: usize,
    token_estimate: usize,
    sections: Vec<SectionJsonOutput>,
}

#[derive(Serialize)]
struct TreeMultiJsonOutput {
    schema_version: u32,
    files: Vec<TreeFileJsonOutput>,
}

#[derive(Serialize)]
struct SectionJsonOutput {
    id: String,
    title: String,
    level: u8,
    path: Vec<String>,
    line_start: usize,
    line_end: usize,
    token_estimate: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<SectionJsonOutput>,
}

#[derive(Serialize)]
struct ReadJsonOutput {
    schema_version: u32,
    path: String,
    selector: ReadSelector,
    section: SectionJsonOutput,
    content: String,
    truncated: bool,
}

#[derive(Serialize)]
struct ReadSelector {
    #[serde(rename = "type")]
    r#type: String,
    value: String,
}

#[derive(Serialize)]
struct SearchJsonOutput {
    schema_version: u32,
    query: String,
    root: String,
    results: Vec<SearchJsonResult>,
}

#[derive(Serialize)]
struct SearchJsonResult {
    path: String,
    section_id: String,
    section_title: String,
    section_path: Vec<String>,
    line_start: usize,
    line_end: usize,
    token_estimate: usize,
    match_count: usize,
    body: Option<String>,
    preview: Option<String>,
    snippets: Vec<SearchJsonSnippet>,
}

#[derive(Serialize)]
struct SearchJsonSnippet {
    line_start: usize,
    line_end: usize,
    text: String,
}

#[derive(Serialize)]
struct ScoutJsonOutput {
    schema_version: u32,
    root: String,
    question: String,
    token_budget: usize,
    candidate_count: usize,
    queries: Vec<String>,
    candidates: Vec<ScoutCandidate>,
    rendered_text: String,
}

#[derive(Serialize)]
struct PackJsonOutput {
    schema_version: u32,
    token_budget: usize,
    token_estimate: usize,
    truncated: bool,
    included: Vec<PackJsonIncluded>,
    content: String,
}

#[derive(Serialize)]
struct PackJsonIncluded {
    path: String,
    section_id: String,
    section_path: Vec<String>,
    line_start: usize,
    line_end: usize,
    token_estimate: usize,
    truncated: bool,
}

#[derive(Serialize)]
struct StatsJsonOutput {
    schema_version: u32,
    entries: Vec<StatsJsonEntry>,
}

#[derive(Serialize)]
struct StatsJsonEntry {
    path: String,
    lines: usize,
    words: usize,
    tokens: usize,
}

#[derive(Serialize)]
struct SectionsJsonOutput {
    schema_version: u32,
    files: Vec<SectionsJsonFile>,
}

#[derive(Serialize)]
struct SectionsJsonFile {
    path: String,
    sections: Vec<SectionsJsonSection>,
}

#[derive(Serialize)]
struct SectionsJsonSection {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_end: Option<usize>,
    token_estimate: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
}

// --- Helper functions ---

fn serialize_sections(
    sections: &[Section],
    max_depth: Option<usize>,
    include_preamble: bool,
    current_depth: usize,
) -> Vec<SectionJsonOutput> {
    let mut result = Vec::new();
    for section in sections {
        if section.title == "<preamble>" && !include_preamble {
            continue;
        }
        let children = if let Some(max) = max_depth {
            if current_depth + 1 < max {
                serialize_sections(
                    &section.children,
                    max_depth,
                    include_preamble,
                    current_depth + 1,
                )
            } else {
                Vec::new()
            }
        } else {
            serialize_sections(
                &section.children,
                max_depth,
                include_preamble,
                current_depth + 1,
            )
        };

        result.push(SectionJsonOutput {
            id: section.id.clone(),
            title: section.title.clone(),
            level: section.level,
            path: section.path.clone(),
            line_start: section.line_start,
            line_end: section.line_end,
            token_estimate: section.token_estimate,
            children,
        });
    }
    result
}

fn truncate_content_to_tokens(content: &str, max_tokens: usize) -> String {
    truncate_to_tokens(content, max_tokens, TRUNCATION_NOTICE)
}
