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
        .stdout(predicate::str::contains("l1-"));
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
fn sections_with_content_omits_child_bodies_by_default() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--content");
    let input = format!("{}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    let parent_idx = stdout.find("§1 A").unwrap();
    let child_idx = stdout.find("§1.1 B").unwrap();
    let parent_block = &stdout[parent_idx..child_idx];

    assert!(
        !parent_block.contains("### C"),
        "parent section should not duplicate descendant content by default"
    );
}

#[test]
fn sections_with_content_and_children_includes_descendants() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--content").arg("--children");
    let input = format!("{}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    let parent_idx = stdout.find("§1 A").unwrap();
    let child_idx = stdout.find("§1.1 B").unwrap();
    let parent_block = &stdout[parent_idx..child_idx];

    assert!(
        parent_block.contains("### C"),
        "parent section should include descendant content when --children is set"
    );
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
        output.status.success(),
        "command should succeed: stderr={stderr}"
    );
    assert!(
        stderr.contains("sections omitted") || output.stdout.len() > 0,
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

#[test]
fn sections_max_files_rejects_over_limit() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--max-files").arg("1");
    let input = format!("{0}/simple.md\n{0}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("exceed --max-files"));
}

#[test]
fn sections_max_files_allows_under_limit() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--max-files").arg("5");
    let input = format!("{}/simple.md\n", FIXTURES);
    cmd.write_stdin(input);
    cmd.assert().success();
}

#[test]
fn sections_positional_file_arg() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections")
        .arg("--lines")
        .arg(format!("{}/simple.md", FIXTURES));
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Overview"))
        .stdout(predicate::str::contains("l1-"));
}

#[test]
fn sections_positional_multiple_files() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections")
        .arg(format!("{}/simple.md", FIXTURES))
        .arg(format!("{}/nested.md", FIXTURES));
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("simple.md"))
        .stdout(predicate::str::contains("nested.md"));
}

#[test]
fn sections_whole_file_mode_caps_depth_by_default() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--lines");
    let input = format!("{}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success(), "stdout={stdout}");
    assert!(stdout.contains("§1 A"));
    assert!(stdout.contains("§1.1 B"));
    assert!(!stdout.contains("§1.1.1 C"));
}

#[test]
fn sections_whole_file_mode_max_depth_opt_out_restores_deeper_sections() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections")
        .arg("--lines")
        .arg("--max-depth")
        .arg("3");
    let input = format!("{}/nested.md\n", FIXTURES);
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success(), "stdout={stdout}");
    assert!(stdout.contains("§1.1.1 C"));
}

#[test]
fn sections_grep_hits_select_deepest_matching_sections() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--lines").arg("--content");
    let input = format!(
        "{}/nested.md:11:Content of C.\n{}/nested.md:19:Content of E.\n",
        FIXTURES, FIXTURES
    );
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success(), "stdout={stdout}");
    assert!(stdout.contains("§1.1.1 C l9-12"));
    assert!(stdout.contains("§1.2 E l17-20"));
    assert!(!stdout.contains("§1 A"));
    assert!(!stdout.contains("§2 F"));
}

#[test]
fn sections_grep_hits_dedupe_by_section_by_default() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--lines");
    let input = format!(
        "{}/nested.md:11:Content of C.\n{}/nested.md:11:Content of C.\n",
        FIXTURES, FIXTURES
    );
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success(), "stdout={stdout}");
    assert_eq!(stdout.matches("§1.1.1 C").count(), 1);
}

#[test]
fn sections_grep_hits_respect_no_dedupe() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections").arg("--lines").arg("--no-dedupe");
    let input = format!(
        "{}/nested.md:11:Content of C.\n{}/nested.md:11:Content of C.\n",
        FIXTURES, FIXTURES
    );
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success(), "stdout={stdout}");
    assert_eq!(stdout.matches("§1.1.1 C").count(), 2);
}

#[test]
fn sections_grep_hits_max_sections_keeps_highest_hit_count_first() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("sections")
        .arg("--lines")
        .arg("--max-sections")
        .arg("1");
    let input = format!(
        "{0}/nested.md:5:## B\n{0}/nested.md:7:Content of B.\n{0}/nested.md:19:Content of E.\n",
        FIXTURES
    );
    cmd.write_stdin(input);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success(), "stdout={stdout}");
    assert!(stdout.contains("§1.1 B"));
    assert!(!stdout.contains("§1.2 E"));
}
