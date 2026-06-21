//! `mdlens init` — wire mdlens guidance into AI coding harnesses.
//!
//! Writes a small, idempotent "managed block" of agent instructions into the
//! instruction file each harness reads (CLAUDE.md, AGENTS.md, GEMINI.md, etc.).
//! The block is delimited by HTML comment markers so re-running `init` updates
//! it in place instead of appending duplicates.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Marker bounding the managed region. Anything between these is owned by mdlens.
pub const BLOCK_START: &str = "<!-- mdlens:start -->";
pub const BLOCK_END: &str = "<!-- mdlens:end -->";

/// The guidance injected into each harness file. Kept short on purpose: detailed
/// help lives in `mdlens --help` / `mdlens scout --help`, so the block stays small.
pub const BLOCK_BODY: &str = "## mdlens (Markdown retrieval)

When answering questions over Markdown files or documentation directories, prefer `mdlens` over `find`/`rg`/`cat` plus whole-file reads.

For question answering over a Markdown directory, start with:

```bash
mdlens scout <dir> \"<question>\" --max-tokens 1400
```

Answer from `[highlights]` and `[evidence]` when they are sufficient. If exactly one detail is missing, use a section id from `[files]`:

```bash
mdlens read <file> --id <section-id> --max-tokens 1200
```

Avoid reading whole Markdown files unless scout clearly found the wrong file. Preserve exact short source terms such as flags, IDs, option names, metrics, table values, and policy phrases. Run `mdlens --help` once if you need command guidance.";

/// Cursor only loads rules from `.mdc` files with YAML frontmatter, so its
/// dedicated rule file is seeded with this when first created.
const CURSOR_FRONTMATTER: &str =
    "---\ndescription: Use mdlens for Markdown and documentation retrieval\nalwaysApply: true\n---";

/// A harness mdlens knows how to wire into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    Gemini,
    Copilot,
    Cursor,
}

impl Harness {
    /// Parse a `--agent <name>` value (or one of the `--claude`/`--codex`/... flags).
    pub fn from_name(name: &str) -> Option<Harness> {
        match name.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "claudecode" => Some(Harness::Claude),
            "codex" | "openai" | "agents" | "opencode" => Some(Harness::Codex),
            "gemini" => Some(Harness::Gemini),
            "copilot" | "github-copilot" => Some(Harness::Copilot),
            "cursor" => Some(Harness::Cursor),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex / AGENTS.md",
            Harness::Gemini => "Gemini CLI",
            Harness::Copilot => "GitHub Copilot",
            Harness::Cursor => "Cursor",
        }
    }

    /// Content to seed a freshly-created, mdlens-dedicated file with, before the
    /// managed block (e.g. Cursor's required `.mdc` frontmatter). `None` for
    /// shared instruction files we only append a block to.
    pub fn file_preamble(self) -> Option<&'static str> {
        match self {
            Harness::Cursor => Some(CURSOR_FRONTMATTER),
            _ => None,
        }
    }

    /// Resolve the instruction file this harness reads.
    ///
    /// `global` selects the user-level config location (e.g. `~/.claude/CLAUDE.md`)
    /// instead of the project-relative one. `home` and `root` are injected so this
    /// stays pure and testable.
    pub fn target_path(self, global: bool, home: Option<&Path>, root: &Path) -> Option<PathBuf> {
        if global {
            let home = home?;
            let p = match self {
                Harness::Claude => home.join(".claude").join("CLAUDE.md"),
                Harness::Codex => home.join(".codex").join("AGENTS.md"),
                Harness::Gemini => home.join(".gemini").join("GEMINI.md"),
                // Copilot/Cursor have no widely-standard global instruction file;
                // skip them for --global rather than guess.
                Harness::Copilot | Harness::Cursor => return None,
            };
            Some(p)
        } else {
            let p = match self {
                Harness::Claude => root.join("CLAUDE.md"),
                Harness::Codex => root.join("AGENTS.md"),
                Harness::Gemini => root.join("GEMINI.md"),
                Harness::Copilot => root.join(".github").join("copilot-instructions.md"),
                Harness::Cursor => root.join(".cursor").join("rules").join("mdlens.mdc"),
            };
            Some(p)
        }
    }
}

/// What happened to a single target file.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Created,
    UpdatedBlock,
    AlreadyCurrent,
    SkippedNoGlobal,
}

/// Result of planning/applying init for one harness.
#[derive(Debug)]
pub struct TargetOutcome {
    pub harness: Harness,
    pub path: Option<PathBuf>,
    pub change: Change,
}

fn managed_block() -> String {
    format!("{}\n{}\n{}", BLOCK_START, BLOCK_BODY, BLOCK_END)
}

/// Upsert the managed block into `existing` file content, returning the new
/// content and whether it differs from the input. Pure string transform — no IO.
///
/// The managed region spans from the first `BLOCK_START` to the *last*
/// `BLOCK_END`, so stray/duplicate blocks collapse into one on the next run.
pub fn upsert_block(existing: &str) -> (String, bool) {
    let block = managed_block();

    if let Some(start) = existing.find(BLOCK_START) {
        if let Some(end_marker) = existing.rfind(BLOCK_END) {
            if end_marker >= start {
                let end = end_marker + BLOCK_END.len();
                let mut out = String::with_capacity(existing.len() + block.len());
                out.push_str(&existing[..start]);
                out.push_str(&block);
                out.push_str(&existing[end..]);
                let changed = out != existing;
                return (out, changed);
            }
            // Reversed markers (END before START): fall through and append.
        }
    }

    // No usable block: append, separated by a blank line if needed.
    if existing.trim().is_empty() {
        (format!("{}\n", block), true)
    } else {
        let sep = if existing.ends_with("\n\n") {
            ""
        } else if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        (format!("{}{}{}\n", existing, sep, block), true)
    }
}

/// Write `content` to `path` atomically: write a sibling temp file, then rename
/// over the target so a crash mid-write can't truncate the user's file.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mdlens".to_string());
    let tmp_name = format!(".{}.mdlens.{}.tmp", file_name, std::process::id());
    let tmp = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(dir) => dir.join(tmp_name),
        None => PathBuf::from(tmp_name),
    };

    fs::write(&tmp, content).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        let _ = fs::remove_file(&tmp);
        format!("replacing {}", path.display())
    })?;
    Ok(())
}

/// Apply init to one resolved path on disk.
fn apply_to_path(path: &Path, preamble: Option<&str>, dry_run: bool) -> Result<Change> {
    // Refuse to write through a symlinked target — in a shared checkout that
    // would let a committed symlink redirect our write to an arbitrary file.
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(anyhow::anyhow!(
                "refusing to write through symlink: {}",
                path.display()
            ));
        }
    }

    let existing = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let file_existed = path.exists();

    // Seed a brand-new dedicated file (e.g. Cursor .mdc) with its frontmatter.
    let base = if existing.is_empty() {
        match preamble {
            Some(p) => format!("{}\n\n", p),
            None => String::new(),
        }
    } else {
        existing
    };

    let (new_content, changed) = upsert_block(&base);
    let change = if !file_existed {
        Change::Created
    } else if !changed {
        Change::AlreadyCurrent
    } else {
        Change::UpdatedBlock
    };

    if change == Change::AlreadyCurrent || dry_run {
        return Ok(change);
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    atomic_write(path, &new_content)?;
    Ok(change)
}

/// Plan + (unless dry_run) apply init across the selected harnesses.
pub fn run_init(
    harnesses: &[Harness],
    global: bool,
    dry_run: bool,
    root: PathBuf,
) -> Result<Vec<TargetOutcome>> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut outcomes = Vec::new();

    for &harness in harnesses {
        let path = harness.target_path(global, home.as_deref(), &root);
        let change = match &path {
            None => Change::SkippedNoGlobal,
            Some(p) => apply_to_path(p, harness.file_preamble(), dry_run)?,
        };
        outcomes.push(TargetOutcome {
            harness,
            path,
            change,
        });
    }

    Ok(outcomes)
}

/// Default harness set when the user names none: the two most common files.
pub fn default_harnesses() -> Vec<Harness> {
    vec![Harness::Claude, Harness::Codex]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn upsert_into_empty_creates_block() {
        let (out, changed) = upsert_block("");
        assert!(changed);
        assert!(out.contains(BLOCK_START));
        assert!(out.contains(BLOCK_END));
        assert!(out.contains("mdlens scout"));
    }

    #[test]
    fn upsert_appends_to_existing_content() {
        let existing = "# My project\n\nSome notes.\n";
        let (out, changed) = upsert_block(existing);
        assert!(changed);
        assert!(out.starts_with("# My project"));
        assert!(out.contains(BLOCK_START));
        assert!(out.contains("Some notes."));
    }

    #[test]
    fn upsert_appends_at_eof_without_trailing_newline() {
        let existing = "# My project\n\nNo trailing newline";
        let (out, changed) = upsert_block(existing);
        assert!(changed);
        assert!(out.contains("No trailing newline\n\n<!-- mdlens:start -->"));
        assert_eq!(out.matches(BLOCK_START).count(), 1);
    }

    #[test]
    fn upsert_is_idempotent() {
        let (once, _) = upsert_block("# Title\n");
        let (twice, changed) = upsert_block(&once);
        assert!(!changed);
        assert_eq!(once, twice);
        assert_eq!(once.matches(BLOCK_START).count(), 1);
    }

    #[test]
    fn upsert_replaces_stale_block_in_place() {
        let stale = format!(
            "# Title\n\n{}\nOLD GUIDANCE\n{}\n\ntrailing\n",
            BLOCK_START, BLOCK_END
        );
        let (out, changed) = upsert_block(&stale);
        assert!(changed);
        assert!(!out.contains("OLD GUIDANCE"));
        assert!(out.contains("mdlens scout"));
        assert!(out.contains("trailing"));
        assert_eq!(out.matches(BLOCK_START).count(), 1);
    }

    #[test]
    fn upsert_collapses_duplicate_blocks() {
        let dupes = format!(
            "{}\nfirst\n{}\nmid\n{}\nsecond\n{}\n",
            BLOCK_START, BLOCK_END, BLOCK_START, BLOCK_END
        );
        let (out, changed) = upsert_block(&dupes);
        assert!(changed);
        // Spanning first-start..last-end collapses both stale blocks into one.
        assert_eq!(out.matches(BLOCK_START).count(), 1);
        assert_eq!(out.matches(BLOCK_END).count(), 1);
        assert!(!out.contains("first"));
        assert!(!out.contains("second"));
        assert!(!out.contains("mid")); // "mid" lived between the two blocks
    }

    #[test]
    fn harness_from_name_aliases() {
        assert_eq!(Harness::from_name("claude"), Some(Harness::Claude));
        assert_eq!(Harness::from_name("Codex"), Some(Harness::Codex));
        assert_eq!(Harness::from_name("opencode"), Some(Harness::Codex));
        assert_eq!(Harness::from_name("cursor"), Some(Harness::Cursor));
        assert_eq!(Harness::from_name("nope"), None);
    }

    #[test]
    fn project_paths_resolve() {
        let root = PathBuf::from("/proj");
        assert_eq!(
            Harness::Claude.target_path(false, None, &root),
            Some(PathBuf::from("/proj/CLAUDE.md"))
        );
        assert_eq!(
            Harness::Codex.target_path(false, None, &root),
            Some(PathBuf::from("/proj/AGENTS.md"))
        );
        assert_eq!(
            Harness::Copilot.target_path(false, None, &root),
            Some(PathBuf::from("/proj/.github/copilot-instructions.md"))
        );
        assert_eq!(
            Harness::Cursor.target_path(false, None, &root),
            Some(PathBuf::from("/proj/.cursor/rules/mdlens.mdc"))
        );
    }

    #[test]
    fn global_skips_harnesses_without_global_file() {
        let root = PathBuf::from("/proj");
        let home = PathBuf::from("/home/u");
        assert_eq!(
            Harness::Claude.target_path(true, Some(home.as_path()), &root),
            Some(PathBuf::from("/home/u/.claude/CLAUDE.md"))
        );
        assert_eq!(
            Harness::Cursor.target_path(true, Some(home.as_path()), &root),
            None
        );
    }

    // ---- apply_to_path / IO layer ----

    #[test]
    fn apply_creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let change = apply_to_path(&path, None, false).unwrap();
        assert_eq!(change, Change::Created);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains(BLOCK_START));
        assert!(written.contains("mdlens scout"));
    }

    #[test]
    fn apply_updates_existing_nonempty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, "# Existing\n").unwrap();
        let change = apply_to_path(&path, None, false).unwrap();
        assert_eq!(change, Change::UpdatedBlock);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("# Existing"));
        assert!(written.contains(BLOCK_START));
    }

    #[test]
    fn apply_empty_existing_file_is_updated_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.md");
        fs::write(&path, "").unwrap();
        let change = apply_to_path(&path, None, false).unwrap();
        assert_eq!(change, Change::UpdatedBlock);
    }

    #[test]
    fn apply_is_idempotent_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        apply_to_path(&path, None, false).unwrap();
        let change = apply_to_path(&path, None, false).unwrap();
        assert_eq!(change, Change::AlreadyCurrent);
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written.matches(BLOCK_START).count(), 1);
    }

    #[test]
    fn apply_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let change = apply_to_path(&path, None, true).unwrap();
        assert_eq!(change, Change::Created);
        assert!(!path.exists());
    }

    #[test]
    fn apply_seeds_cursor_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mdlens.mdc");
        apply_to_path(&path, Harness::Cursor.file_preamble(), false).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("---\n"));
        assert!(written.contains("alwaysApply: true"));
        assert!(written.contains(BLOCK_START));
    }

    #[cfg(unix)]
    #[test]
    fn apply_refuses_symlinked_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.md");
        fs::write(&real, "secret\n").unwrap();
        let link = dir.path().join("CLAUDE.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = apply_to_path(&link, None, false).unwrap_err();
        assert!(err.to_string().contains("symlink"));
        // The real file was not touched.
        assert_eq!(fs::read_to_string(&real).unwrap(), "secret\n");
    }
}
