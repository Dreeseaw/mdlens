use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

const FIXTURES: &str = "tests/fixtures";

#[test]
fn pack_by_ids() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--ids")
        .arg("1.1,1.2")
        .arg("--max-tokens")
        .arg("5000");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Install"));
}

#[test]
fn pack_by_ids_json() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--ids")
        .arg("1.1,1.2")
        .arg("--max-tokens")
        .arg("5000")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["token_budget"], 5000);
    assert!(json["included"].is_array());
}

#[test]
fn pack_with_truncation() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/nested.md", FIXTURES))
        .arg("--ids")
        .arg("1")
        .arg("--max-tokens")
        .arg("15")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["truncated"], true);
    assert_eq!(json["included"][0]["truncated"], true);
}

#[test]
fn pack_with_parents() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/nested.md", FIXTURES))
        .arg("--ids")
        .arg("1.1.1")
        .arg("--max-tokens")
        .arg("5000")
        .arg("--parents");
    cmd.assert().success();
}

#[test]
fn pack_by_search() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(FIXTURES)
        .arg("--search")
        .arg("Results")
        .arg("--max-tokens")
        .arg("5000");
    cmd.assert().success();
}

#[test]
fn pack_by_search_honors_regex() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/duplicate_headings.md", FIXTURES))
        .arg("--search")
        .arg("Results|Analysis")
        .arg("--regex")
        .arg("--max-tokens")
        .arg("5000")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert!(json["included"].as_array().unwrap().len() >= 2);
}

#[test]
fn pack_no_dedupe_keeps_duplicate_ids() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--ids")
        .arg("1.1,1.1")
        .arg("--no-dedupe")
        .arg("--max-tokens")
        .arg("5000")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let included = json["included"].as_array().unwrap();

    assert_eq!(included.len(), 2);
    assert_eq!(included[0]["section_id"], "1.1");
    assert_eq!(included[1]["section_id"], "1.1");
}

#[test]
fn pack_by_path_rejects_ambiguous_match() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "# A\n\nFirst.\n\n# A\n\nSecond.\n").unwrap();

    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(file.path())
        .arg("--paths")
        .arg("A")
        .arg("--max-tokens")
        .arg("5000");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("path matched multiple sections"));
}

#[test]
fn pack_rejects_multiple_selectors() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--ids")
        .arg("1")
        .arg("--search")
        .arg("Overview")
        .arg("--max-tokens")
        .arg("5000");
    cmd.assert().failure();
}

#[test]
fn pack_no_selector() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--max-tokens")
        .arg("5000");
    cmd.assert().failure();
}

#[test]
fn pack_invalid_id() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("pack")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--ids")
        .arg("99")
        .arg("--max-tokens")
        .arg("5000");
    cmd.assert().failure();
}
