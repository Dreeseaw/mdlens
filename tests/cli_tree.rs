use assert_cmd::Command;
use predicates::prelude::*;

const FIXTURES: &str = "tests/fixtures";

#[test]
fn tree_simple_file() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree").arg(format!("{}/simple.md", FIXTURES));
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Overview"))
        .stdout(predicate::str::contains("Install"))
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn tree_simple_json() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree").arg(format!("{}/simple.md", FIXTURES)).arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["line_count"], 14);
    assert!(json["sections"].is_array());
}

#[test]
fn tree_nested() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree").arg(format!("{}/nested.md", FIXTURES));
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("A"))
        .stdout(predicate::str::contains("B"))
        .stdout(predicate::str::contains("C"))
        .stdout(predicate::str::contains("D"))
        .stdout(predicate::str::contains("E"))
        .stdout(predicate::str::contains("F"));
}

#[test]
fn tree_no_headings() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree")
        .arg(format!("{}/no_headings.md", FIXTURES))
        .arg("--include-preamble");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("<preamble>"));
}

#[test]
fn tree_code_blocks() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree").arg(format!("{}/code_blocks.md", FIXTURES));
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Real Heading"))
        .stdout(predicate::str::contains("Real Child"))
        .stdout(predicate::str::contains("Fake Heading").not());
}

#[test]
fn tree_duplicate_headings() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree")
        .arg(format!("{}/duplicate_headings.md", FIXTURES))
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Should have Report as root with Results appearing twice
    let sections = &json["sections"];
    assert!(sections.is_array());
    let report = &sections[0];
    assert_eq!(report["title"], "Report");
    assert_eq!(report["children"].as_array().unwrap().len(), 3); // Results, Results, Analysis
}

#[test]
fn tree_directory() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree").arg(FIXTURES);
    cmd.assert().success();
}

#[test]
fn tree_max_depth() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("tree")
        .arg(format!("{}/nested.md", FIXTURES))
        .arg("--max-depth")
        .arg("1");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    // At depth 1, should not show C, D (which are level 3)
    assert!(stdout.contains("A"));
    assert!(stdout.contains("F"));
}
