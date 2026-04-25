use assert_cmd::Command;
use predicates::prelude::*;

const FIXTURES: &str = "tests/fixtures";

#[test]
fn search_plain_text() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search").arg(FIXTURES).arg("Results");
    cmd.assert().success();
}

#[test]
fn search_json() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(FIXTURES)
        .arg("Results")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["query"], "Results");
    assert!(json["results"].is_array());
}

#[test]
fn search_case_sensitive() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(format!("{}/duplicate_headings.md", FIXTURES))
        .arg("Results")
        .arg("--case-sensitive");
    cmd.assert().success();
}

#[test]
fn search_regex() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(FIXTURES)
        .arg("Results|Analysis")
        .arg("--regex");
    cmd.assert().success();
}

#[test]
fn search_single_file() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("Overview");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Overview"));
}

#[test]
fn search_no_matches() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("xyznonexistent");
    cmd.assert().success();
}

#[test]
fn search_max_results() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(FIXTURES)
        .arg("Content")
        .arg("--max-results")
        .arg("2");
    cmd.assert().success();
}
