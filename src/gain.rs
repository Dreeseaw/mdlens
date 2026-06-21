//! `mdlens gain` — accumulated token-savings tracking, in the spirit of RTK.
//!
//! NOTE: this is the one stateful corner of mdlens. The core commands are
//! deliberately stateless (same input → same output, everywhere). `gain` is a
//! peripheral convenience, so it keeps a small append-only log of how many
//! tokens each `scout`/`read` call saved versus reading the whole file(s), and
//! sums it on demand. RTK stores this in a SQLite DB; we keep it simpler: one
//! JSON object per line in `~/.local/share/mdlens/history.jsonl` (no new deps).
//!
//! Recording is best-effort and never affects command output, so determinism of
//! results is preserved. Set `MDLENS_NO_GAIN=1` to disable tracking entirely.
//! The history file is plain append-only text — truncate it any time with
//! `mdlens gain --reset --yes`, or `tail -n 1000 <file> | sponge <file>`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One recorded invocation. Mirrors RTK's `commands` row (minus exec time).
///
/// All fields are `#[serde(default)]` so that if this struct gains fields in a
/// future version, old history lines still deserialize instead of silently
/// dropping (which would zero a user's accumulated history). Keep it that way.
#[derive(Serialize, Deserialize, Default)]
pub struct Record {
    #[serde(default)]
    pub ts: u64,
    #[serde(default)]
    pub cmd: String,
    #[serde(default)]
    pub input_tokens: usize,
    #[serde(default)]
    pub output_tokens: usize,
}

/// Where the append-only history lives (XDG data dir; overridable for tests).
fn history_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MDLENS_HISTORY") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("mdlens").join("history.jsonl"))
}

/// Append a pre-serialized line to the history file, refusing to follow a
/// symlinked target (so a planted symlink can't redirect our writes elsewhere).
fn append_record(path: &Path, line: &str) -> std::io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::other("history path is a symlink"));
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())
}

/// Append one usage record. Best-effort: any failure is silently ignored so
/// tracking can never break or slow down the actual command.
pub fn record(cmd: &str, input_tokens: usize, output_tokens: usize) {
    if std::env::var_os("MDLENS_NO_GAIN").is_some() {
        return;
    }
    let Some(path) = history_path() else {
        return;
    };
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rec = Record {
        ts,
        cmd: cmd.to_string(),
        input_tokens,
        output_tokens,
    };
    let Ok(mut line) = serde_json::to_string(&rec) else {
        return;
    };
    line.push('\n');
    let _ = append_record(&path, &line);
}

/// Per-command rollup.
pub struct CmdAgg {
    pub cmd: String,
    pub count: u64,
    pub input: u128,
    pub output: u128,
}

/// Whole-history rollup.
#[derive(Default)]
pub struct Summary {
    pub count: u64,
    pub input: u128,
    pub output: u128,
    pub by_cmd: Vec<CmdAgg>,
}

/// Aggregate records into totals + per-command rollups (sorted by tokens saved).
pub fn aggregate(records: &[Record]) -> Summary {
    let mut s = Summary::default();
    for r in records {
        s.count += 1;
        s.input += r.input_tokens as u128;
        s.output += r.output_tokens as u128;
        match s.by_cmd.iter_mut().find(|c| c.cmd == r.cmd) {
            Some(c) => {
                c.count += 1;
                c.input += r.input_tokens as u128;
                c.output += r.output_tokens as u128;
            }
            None => s.by_cmd.push(CmdAgg {
                cmd: r.cmd.clone(),
                count: 1,
                input: r.input_tokens as u128,
                output: r.output_tokens as u128,
            }),
        }
    }
    s.by_cmd
        .sort_by_key(|c| std::cmp::Reverse(saved(c.input, c.output)));
    s
}

fn saved(input: u128, output: u128) -> i128 {
    input as i128 - output as i128
}

fn pct(input: u128, output: u128) -> f64 {
    if input == 0 {
        0.0
    } else {
        round1(saved(input, output) as f64 / input as f64 * 100.0)
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Thousands-separated integer (e.g. 1,783).
fn fmt_int(n: i128) -> String {
    let neg = n < 0;
    let digits = n.unsigned_abs().to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3 + 1);
    if neg {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i != 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Replace control characters so an untrusted history value (e.g. a planted
/// `cmd`) can't emit ANSI/OSC escapes into the user's terminal when printed.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn render(s: &Summary) -> String {
    let mut out = String::new();
    out.push_str("mdlens token savings\n");
    out.push_str("────────────────────\n");
    out.push_str(&format!("calls       {}\n", fmt_int(s.count as i128)));
    out.push_str(&format!(
        "baseline    {} tokens\n",
        fmt_int(s.input as i128)
    ));
    out.push_str(&format!(
        "returned    {} tokens\n",
        fmt_int(s.output as i128)
    ));
    out.push_str(&format!(
        "saved       {} tokens ({:.1}%)\n",
        fmt_int(saved(s.input, s.output)),
        pct(s.input, s.output)
    ));
    if !s.by_cmd.is_empty() {
        out.push_str("\nby command\n");
        for c in &s.by_cmd {
            out.push_str(&format!(
                "  {:<8} {:>6} calls   saved {} ({:.1}%)\n",
                sanitize(&c.cmd),
                fmt_int(c.count as i128),
                fmt_int(saved(c.input, c.output)),
                pct(c.input, c.output)
            ));
        }
    }
    if s.count == 0 {
        out.push_str("\nNo usage recorded yet — run `mdlens scout`/`read`, then check back.\n");
    }
    out
}

fn render_json(s: &Summary) -> String {
    let by_cmd: Vec<serde_json::Value> = s
        .by_cmd
        .iter()
        .map(|c| {
            serde_json::json!({
                "command": c.cmd,
                "count": c.count,
                "baseline_tokens": c.input.to_string(),
                "returned_tokens": c.output.to_string(),
                "saved_tokens": saved(c.input, c.output).to_string(),
                "savings_pct": pct(c.input, c.output),
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": 1,
        "calls": s.count,
        "baseline_tokens": s.input.to_string(),
        "returned_tokens": s.output.to_string(),
        "saved_tokens": saved(s.input, s.output).to_string(),
        "savings_pct": pct(s.input, s.output),
        "by_command": by_cmd,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Read + parse records from a specific file, tolerating malformed lines.
fn load_from(path: &Path) -> Vec<Record> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<Record>(line).ok())
        .collect()
}

fn load() -> Vec<Record> {
    match history_path() {
        Some(p) => load_from(&p),
        None => Vec::new(),
    }
}

/// Entry point for the `gain` command.
pub fn run_gain(json: bool, reset: bool, yes: bool) -> Result<()> {
    if reset {
        let summary = aggregate(&load());
        if summary.count == 0 {
            println!("mdlens gain: no history to reset");
            return Ok(());
        }
        let discarded = format!(
            "{} calls / {} tokens saved",
            fmt_int(summary.count as i128),
            fmt_int(saved(summary.input, summary.output))
        );
        if !yes {
            println!("This will discard {discarded}.");
            println!("Re-run to confirm:  mdlens gain --reset --yes");
            return Ok(());
        }
        if let Some(path) = history_path() {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        println!("mdlens gain: reset ({discarded} discarded)");
        return Ok(());
    }
    let summary = aggregate(&load());
    if json {
        println!("{}", render_json(&summary));
    } else {
        print!("{}", render(&summary));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(cmd: &str, input: usize, output: usize) -> Record {
        Record {
            ts: 0,
            cmd: cmd.to_string(),
            input_tokens: input,
            output_tokens: output,
        }
    }

    #[test]
    fn aggregate_totals_and_savings() {
        let recs = vec![
            rec("scout", 1000, 200),
            rec("read", 800, 300),
            rec("scout", 500, 100),
        ];
        let s = aggregate(&recs);
        assert_eq!(s.count, 3);
        assert_eq!(s.input, 2300);
        assert_eq!(s.output, 600);
        assert_eq!(saved(s.input, s.output), 1700);
    }

    #[test]
    fn aggregate_groups_by_command_sorted_by_saved() {
        let recs = vec![
            rec("read", 100, 90),
            rec("scout", 2000, 100),
            rec("scout", 1000, 50),
        ];
        let s = aggregate(&recs);
        assert_eq!(s.by_cmd[0].cmd, "scout");
        assert_eq!(s.by_cmd[0].count, 2);
        assert_eq!(saved(s.by_cmd[0].input, s.by_cmd[0].output), 2850);
        assert_eq!(s.by_cmd[1].cmd, "read");
    }

    #[test]
    fn pct_is_safe_on_empty() {
        let s = aggregate(&[]);
        assert_eq!(s.count, 0);
        assert_eq!(pct(s.input, s.output), 0.0);
        assert!(render(&s).contains("No usage recorded yet"));
    }

    #[test]
    fn fmt_int_groups_and_signs() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(1783), "1,783");
        assert_eq!(fmt_int(-1700), "-1,700");
        assert_eq!(fmt_int(1234567), "1,234,567");
    }

    #[test]
    fn negative_savings_render_honestly() {
        let s = aggregate(&[rec("read", 100, 250)]);
        assert_eq!(saved(s.input, s.output), -150);
        assert!(render(&s).contains("-150"));
    }

    #[test]
    fn pct_rounds_to_one_decimal() {
        // 200/290 saved = 90/290 ≈ 31.03... → 31.0
        assert_eq!(pct(290, 200), 31.0);
    }

    #[test]
    fn sanitize_strips_control_chars() {
        let s = sanitize("scout\x1b[31m\x07");
        assert!(!s.contains('\x1b'));
        assert!(!s.contains('\x07'));
        assert!(s.starts_with("scout"));
    }

    #[test]
    fn append_load_roundtrip_tolerates_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.jsonl");
        for r in [rec("scout", 1000, 200), rec("read", 500, 100)] {
            let mut line = serde_json::to_string(&r).unwrap();
            line.push('\n');
            append_record(&path, &line).unwrap();
        }
        append_record(&path, "this is not json\n").unwrap();
        let recs = load_from(&path);
        assert_eq!(recs.len(), 2); // garbage line dropped, good lines kept
        let s = aggregate(&recs);
        assert_eq!(s.input, 1500);
        assert_eq!(s.output, 300);
    }

    #[test]
    fn load_tolerates_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.jsonl");
        // A line written by a hypothetical older/newer version missing `ts`.
        append_record(
            &path,
            "{\"cmd\":\"scout\",\"input_tokens\":10,\"output_tokens\":2}\n",
        )
        .unwrap();
        let recs = load_from(&path);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].ts, 0);
        assert_eq!(recs[0].input_tokens, 10);
    }

    #[cfg(unix)]
    #[test]
    fn append_refuses_symlinked_history() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.txt");
        fs::write(&real, "secret\n").unwrap();
        let link = dir.path().join("h.jsonl");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(append_record(&link, "x\n").is_err());
        assert_eq!(fs::read_to_string(&real).unwrap(), "secret\n");
    }
}
