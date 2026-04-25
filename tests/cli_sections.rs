use assert_cmd::Command;
use predicates::prelude::*;

const FIXTURES: &str = "tests/fixtures";

#[test]
fn sections_basic() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections");
    let input = format!("{}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Overview"))
        .stdout(predicate::str::contains("Install"))
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn sections_basic_with_lines() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--lines");
    let input = format!("{}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("lines 1-"));
}

#[test]
fn sections_json() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--json");
    let input = format!("{}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert!(json["files"].is_array());
    assert!(!json["files"][0]["sections"].as_array().unwrap().is_empty());
}

#[test]
fn sections_json_with_heading_paths() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--json").arg("--heading-paths");
    let input = format!("{}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let sections = json["files"][0]["sections"].as_array().unwrap();
    // At least one section should have heading_path
    let has_path = sections.iter().any(|s| s.get("heading_path").is_some());
    assert!(has_path, "Expected heading_path in JSON output");
}

#[test]
fn sections_json_with_lines() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--json").arg("--lines");
    let input = format!("{}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let sections = json["files"][0]["sections"].as_array().unwrap();
    assert!(sections[0].get("line_start").is_some());
    assert!(sections[0].get("line_end").is_some());
}

#[test]
fn sections_with_content() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--content");
    let input = format!("{}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("# Overview"));
}

#[test]
fn sections_max_tokens() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--max-tokens").arg("50");
    let input = format!("{}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("sections omitted"),
        "Expected warning about omitted sections, got stderr: {}",
        stderr
    );
}

#[test]
fn sections_empty_stdin() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections");
    cmd.write_stdin("");
    cmd.assert().success().stdout(predicate::str::is_empty());
}

#[test]
fn sections_missing_file() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections");
    cmd.write_stdin("nonexistent_file.md\n");
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Warning"));
    assert!(stderr.contains("nonexistent_file.md"));
}

#[test]
fn sections_dedup() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--json");
    let input = format!("{0}/simple.md\n{0}/simple.md\n{0}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Should only have one file entry despite 3 identical paths
    assert_eq!(json["files"].as_array().unwrap().len(), 1);
}

#[test]
fn sections_no_dedupe() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--json").arg("--no-dedupe");
    let input = format!("{0}/simple.md\n{0}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Should have two file entries when dedup is disabled
    assert_eq!(json["files"].as_array().unwrap().len(), 2);
}

#[test]
fn sections_multiple_files() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections");
    let input = format!("{}/simple.md\n{}/nested.md\n", FIXTURES, FIXTURES);
    cmd.write_stdin(input);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("simple.md"))
        .stdout(predicate::str::contains("nested.md"));
}

#[test]
fn sections_no_headings_file() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections");
    let input = format!("{}/no_headings.md\n", FIXTURES);
    cmd.write_stdin(input);
    // File with no headings should produce no sections (preamble skipped)
    cmd.assert().success();
}
