//! Token-efficient Markdown navigation for AI agents.
//!
//! `mdlens` parses Markdown into a section tree with dotted IDs and token estimates,
//! so agents can read only the sections they need instead of loading entire files.
//!
//! # Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`parse`] | Load a Markdown file into a [`model::Document`] |
//! | [`model`] | [`model::Document`] and [`model::Section`] types |
//! | [`search`] | Section-level full-text search across files |
//! | [`pack`] | Bundle sections into a hard token budget |
//! | [`tokens`] | Token estimation and text truncation |
//! | [`render`] | Human-readable output formatting |
//!
//! # Example
//!
//! ```no_run
//! use mdlens::parse::parse_markdown;
//!
//! let doc = parse_markdown("docs/guide.md").unwrap();
//!
//! // Top-level stats
//! println!("{} sections, ~{} tokens", doc.sections.len(), doc.token_estimate);
//!
//! // Find a section by dotted ID
//! if let Some(section) = doc.find_section_by_id("1.2") {
//!     println!("Section: {} (~{} tokens)", section.title, section.token_estimate);
//! }
//! ```
//!
//! # Section IDs
//!
//! Every heading gets a dotted ID based on its position in the hierarchy:
//!
//! ```text
//! 1        first H1
//! 1.2      second child of section 1
//! 1.2.3    third child of 1.2
//! ```
//!
//! IDs are stable across re-parses of the same file as long as the heading
//! structure does not change.
//!
//! # Token estimates
//!
//! Counts use a ~1 token per 4 UTF-8 characters heuristic. Not exact, but
//! deterministic and fast — good enough for budgeting context windows.

use anyhow::Result;

pub mod cli;
pub mod errors;
pub mod model;
pub mod pack;
pub mod parse;
pub mod render;
pub mod search;
pub mod tokens;

pub fn run() -> Result<()> {
    cli::run()
}
