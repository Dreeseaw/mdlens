use assert_cmd::Command;
use predicates::prelude::*;

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
        .arg("3")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["truncated"], true);
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
