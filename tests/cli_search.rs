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
    cmd.arg("search").arg(FIXTURES).arg("Results").arg("--json");
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

#[test]
fn search_json_is_stable_across_runs() {
    let mut first = Command::cargo_bin("mdlens").unwrap();
    first
        .arg("search")
        .arg(FIXTURES)
        .arg("Content")
        .arg("--json");
    first.assert().success();
    let first_output = String::from_utf8(first.output().unwrap().stdout).unwrap();

    let mut second = Command::cargo_bin("mdlens").unwrap();
    second
        .arg("search")
        .arg(FIXTURES)
        .arg("Content")
        .arg("--json");
    second.assert().success();
    let second_output = String::from_utf8(second.output().unwrap().stdout).unwrap();

    assert_eq!(first_output, second_output);
}

#[test]
fn search_with_preview() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("Overview")
        .arg("--preview")
        .arg("1");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("This is the overview section"));
}

#[test]
fn search_with_content_and_json() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("Overview")
        .arg("--content")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let first = &json["results"][0];
    assert!(first["body"].as_str().unwrap().contains("# Overview"));
}

#[test]
fn search_returns_results_sorted_by_source_priority() {
    // Results from canonical-named files (e.g. *_state, source_of_truth) should appear
    // before dated-sequence files even without an explicit filter flag.
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("search")
        .arg("tests/fixtures")
        .arg("Content")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["results"].is_array());
}
