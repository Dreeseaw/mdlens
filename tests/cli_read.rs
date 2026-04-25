use assert_cmd::Command;
use predicates::prelude::*;

const FIXTURES: &str = "tests/fixtures";

#[test]
fn read_by_id() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--id")
        .arg("1.1");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Install"));
}

#[test]
fn read_by_id_json() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--id")
        .arg("1.1")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["selector"]["type"], "id");
    assert_eq!(json["selector"]["value"], "1.1");
    assert!(!json["content"].as_str().unwrap().is_empty());
}

#[test]
fn read_by_path() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/nested.md", FIXTURES))
        .arg("--heading-path")
        .arg("A>B>C");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Content of C"));
}

#[test]
fn read_by_lines() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--lines")
        .arg("1:3");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("# Overview"));
}

#[test]
fn read_with_parents() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/nested.md", FIXTURES))
        .arg("--id")
        .arg("1.1.1")
        .arg("--parents");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Should contain parent headings
    assert!(stdout.contains("# A"));
    assert!(stdout.contains("## B"));
}

#[test]
fn read_invalid_id() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--id")
        .arg("99");
    cmd.assert().failure();
}

#[test]
fn read_invalid_line_range() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--lines")
        .arg("10:5");
    cmd.assert().failure();
}

#[test]
fn read_no_selector() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read").arg(format!("{}/simple.md", FIXTURES));
    cmd.assert().failure();
}

#[test]
fn read_max_tokens() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg("--id")
        .arg("1")
        .arg("--max-tokens")
        .arg("1");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("truncated"));
}

#[test]
fn read_code_blocks() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("read")
        .arg(format!("{}/code_blocks.md", FIXTURES))
        .arg("--id")
        .arg("1");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Real Heading"))
        .stdout(predicate::str::contains("```"));
}
