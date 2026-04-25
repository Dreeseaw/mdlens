use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::cmp::Reverse;

use std::collections::HashSet;
use std::io::{self, BufRead};

use crate::errors;
use crate::model::Section;
use crate::pack::{pack_by_ids, PackSearchOptions};
use crate::parse::{load_markdown, parse_markdown};
use crate::render::{
    render_pack, render_read, render_search, render_sections, render_stats, render_tree,
    PackIncluded, SectionsEntry, StatsEntry,
};
use crate::search::search_files;
use crate::tokens::{estimate_tokens, truncate_to_tokens};

const TRUNCATION_NOTICE: &str = "\n\n<!-- mdlens: truncated at token budget -->";

#[derive(Parser)]
#[command(name = "mdlens")]
#[command(about = "Token-efficient Markdown structure CLI for AI agents")]
#[command(long_about = "mdlens parses Markdown files into a hierarchical section tree with\ndotted IDs, token estimates, and bounded-context packing.\n\nDesigned for AI agents that need to navigate, search, and pack\nMarkdown documentation into context windows efficiently.\n\nRun `mdlens <command> --help` for detailed usage.")]
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
    /// Include full section body text (default: metadata only)
    #[arg(long)]
    content: bool,
    /// Cap total output tokens (truncates last section if exceeded)
    #[arg(long)]
    max_tokens: Option<usize>,
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

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Tree(args) => cmd_tree(args),
        Commands::Read(args) => cmd_read(args),
        Commands::Search(args) => cmd_search(args),
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
        // Multiple files
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
                        args.max_depth,
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
                    render_tree(&doc, args.max_depth, args.include_preamble)
                );
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
    let results = search_files(
        &args.path,
        &args.query,
        args.case_sensitive,
        args.regex,
        args.max_results,
        args.context_lines,
    )?;

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
        println!("{}", render_search(&results));
    }

    Ok(())
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
    let mut paths: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim().to_string();
        if !trimmed.is_empty() {
            paths.push(trimmed);
        }
    }

    if paths.is_empty() {
        return Ok(());
    }

    let dedupe = args.dedupe && !args.no_dedupe;
    if dedupe {
        let mut seen = HashSet::new();
        paths.retain(|p| seen.insert(p.clone()));
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
        collect_all_sections(&doc.sections, lines, &mut sections);

        if sections.is_empty() {
            continue;
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

    if omitted > 0 {
        eprintln!(
            "Warning: {} sections omitted, would exceed {} token limit",
            omitted,
            args.max_tokens.unwrap_or(0)
        );
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
                    body: if args.content { Some(s.body.clone()) } else { None },
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
}

struct SectionsFileOutput {
    path: String,
    sections: Vec<SectionsSectionOutput>,
}

fn collect_all_sections(
    sections: &[Section],
    lines: &[String],
    result: &mut Vec<SectionsSectionOutput>,
) {
    for section in sections {
        if section.title == "<preamble>" {
            continue;
        }
        let body = section.extract_content(lines).join("\n");
        result.push(SectionsSectionOutput {
            id: section.id.clone(),
            title: section.title.clone(),
            heading_path: section.path.clone(),
            line_start: section.line_start,
            line_end: section.line_end,
            token_estimate: section.token_estimate,
            body,
        });
        collect_all_sections(&section.children, lines, result);
    }
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
    snippets: Vec<SearchJsonSnippet>,
}

#[derive(Serialize)]
struct SearchJsonSnippet {
    line_start: usize,
    line_end: usize,
    text: String,
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
