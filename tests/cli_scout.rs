use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn scout_help_exposes_agent_workflow() {
    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("--help");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Agent quickstart"));
    assert!(stdout.contains("mdlens scout <dir>"));
    assert!(stdout.contains("Answering from scout"));
}

#[test]
fn scout_prioritizes_rule_risk_evidence_for_why_questions() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("experiment_report.md"),
        "# Experiment Report\n\n## Metrics\n\n| field | value |\n|---|---|\n| score | 0.91 |\n| baseline | 0.72 |\n\n## Decision\n\nUse adaptive batching for export.\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("operator_policy.md"),
        "# Operator Policy\n\n## Export Rule\n\n- Rule: use adaptive batching when queue depth changes rapidly.\n- Known risk: fixed batches create stale queue assignments.\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("scout")
        .arg(dir.path())
        .arg("Why should export use adaptive batching rather than fixed batches?");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rule_pos = stdout.find("Known risk").expect("expected risk evidence");
    let metric_pos = stdout.find("| score |").unwrap_or(usize::MAX);
    assert!(
        rule_pos < metric_pos,
        "rule/risk evidence should appear before metrics for why questions:\n{stdout}"
    );
}

#[test]
fn scout_json_returns_structured_metadata_and_rendered_pack() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("policy.md"),
        "# Policy\n\n## Current Rule\n\nUse --mode current because stale mode drops row labels.\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("mdlens").unwrap();
    cmd.arg("scout")
        .arg(dir.path())
        .arg("Which mode is current?")
        .arg("--json");
    cmd.assert().success();

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["question"], "Which mode is current?");
    assert!(json["queries"].is_array());
    assert!(json["candidates"].is_array());
    assert!(json["rendered_text"].as_str().unwrap().contains("[evidence]"));
}
