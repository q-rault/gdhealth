use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

// ── help flag tests ──────────────────────────────────────────────────────────

#[test]
fn graph_help_succeeds() {
    let output = run_cargo_help(&["graph", "--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--root"), "help should mention --root");
    assert!(text.contains("--format"), "help should mention --format");
}

#[test]
fn metrics_help_succeeds() {
    let output = run_cargo_help(&["metrics", "--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--root"), "help should mention --root");
    assert!(text.contains("--format"), "help should mention --format");
}

#[test]
fn policy_help_succeeds() {
    let output = run_cargo_help(&["policy", "--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--fail-on"), "help should mention --fail-on");
}

#[test]
fn health_help_succeeds() {
    let output = run_cargo_help(&["health", "--help"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--fail-on"), "help should mention --fail-on");
    assert!(
        text.contains("reports/code-health"),
        "help should mention bundle output path"
    );
}

// ── bundle output test ───────────────────────────────────────────────────────

#[test]
fn health_writes_default_report_bundle() {
    let fixture_root = unique_temp_dir("gdhealth-health-bundle");
    write_repo(
        &fixture_root,
        &bundle_config("warning", "[]"),
        &[
            (
                "main.gd",
                r#"extends RefCounted

const HUD = preload("res://levels/main_level/hud/hud_panel.gd")

func update_hud(player_id: int, hp: int, mana: int, shield: int) -> void:
    if hp > 0:
        if mana > 0:
            print(player_id, hp, mana, shield)
"#,
            ),
            (
                "levels/main_level/hud/hud_panel.gd",
                r#"extends RefCounted

func redraw() -> void:
    print("hud")
"#,
            ),
            (
                "server/state_sync.gd",
                r#"extends RefCounted

func tick() -> void:
    print("server")
"#,
            ),
        ],
    );

    let output = run_gdhealth(&fixture_root, &["health", "--root", "."]);
    assert_success(&output);

    let report_dir = fixture_root.join("reports").join("code-health");
    for file_name in [
        "graph.json",
        "graph.dot",
        "metrics.json",
        "policy.json",
        "health.json",
        "summary.txt",
    ] {
        assert!(
            report_dir.join(file_name).is_file(),
            "missing report file {file_name}"
        );
    }

    let summary = fs::read_to_string(report_dir.join("summary.txt")).expect("read summary");
    assert!(summary.contains("top files:"));
    assert!(summary.contains("top functions:"));
    assert!(summary.contains("policy findings:"));
    assert!(summary.contains("cross-group dependencies:"));
    assert!(summary.contains("main.gd depends on levels/main_level/hud/hud_panel.gd"));

    // Verify health.json has all expected top-level fields
    let health_json: Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("health.json")).expect("read health.json"),
    )
    .expect("parse health.json");
    assert_eq!(
        health_json["tool_name"].as_str(),
        Some("gdhealth"),
        "health.json must have tool_name"
    );
    assert_eq!(
        health_json["schema_version"].as_u64(),
        Some(1),
        "health.json must have schema_version: 1"
    );
    assert!(health_json["summary"].is_object(), "health.json must have summary");
    assert!(health_json["top_files"].is_array(), "health.json must have top_files");
    assert!(health_json["top_functions"].is_array(), "health.json must have top_functions");
    assert!(health_json["cycles"].is_array(), "health.json must have cycles");
    assert!(
        health_json["policy_findings"].is_array(),
        "health.json must have policy_findings"
    );
    assert!(
        health_json["cross_group_dependencies"].is_array(),
        "health.json must have cross_group_dependencies"
    );

    // Verify graph.json schema
    let graph_json: Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("graph.json")).expect("read graph.json"),
    )
    .expect("parse graph.json");
    assert_eq!(graph_json["schema_version"].as_u64(), Some(1));
    assert!(graph_json["files"].is_array());
    assert!(graph_json["edges"].is_array());
    assert!(graph_json["cycles"].is_array());
    assert!(graph_json["parser_warnings"].is_array());

    // Verify metrics.json schema
    let metrics_json: Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("metrics.json")).expect("read metrics.json"),
    )
    .expect("parse metrics.json");
    assert_eq!(metrics_json["schema_version"].as_u64(), Some(1));
    assert!(metrics_json["files"].is_array());
    assert!(metrics_json["functions"].is_array());

    // Verify policy.json schema
    let policy_json: Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("policy.json")).expect("read policy.json"),
    )
    .expect("parse policy.json");
    assert_eq!(policy_json["schema_version"].as_u64(), Some(1));
    assert!(policy_json["findings"].is_array());

    fs::remove_dir_all(&fixture_root).ok();
}

// ── policy fail-on test ──────────────────────────────────────────────────────

#[test]
fn policy_fails_on_critical_forbidden_dependency() {
    let fixture_root = unique_temp_dir("gdhealth-fail-on-critical");
    write_repo(
        &fixture_root,
        &bundle_config("critical", "[]"),
        &[
            (
                "main.gd",
                r#"extends RefCounted
const HUD = preload("res://levels/main_level/hud/hud_panel.gd")
"#,
            ),
            (
                "levels/main_level/hud/hud_panel.gd",
                "extends RefCounted\n",
            ),
        ],
    );

    let output = run_gdhealth(
        &fixture_root,
        &[
            "policy",
            "--root",
            ".",
            "--format",
            "json",
            "--fail-on",
            "critical",
        ],
    );
    assert!(
        !output.status.success(),
        "expected failure, stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(&fixture_root).ok();
}

// ── cycle allowlist test ─────────────────────────────────────────────────────

#[test]
fn cycle_allowlist_suppresses_only_intended_findings() {
    let fixture_root = unique_temp_dir("gdhealth-cycle-allowlist");
    write_repo(
        &fixture_root,
        &bundle_config("warning", "[\"net/**\"]"),
        &[
            (
                "net/allowed_cycle.gd",
                r#"extends RefCounted
const SELF = preload("res://net/allowed_cycle.gd")
"#,
            ),
            (
                "server/disallowed_cycle.gd",
                r#"extends RefCounted
const SELF = preload("res://server/disallowed_cycle.gd")
"#,
            ),
        ],
    );

    let report_path = fixture_root.join("policy.json");
    let output = run_gdhealth(
        &fixture_root,
        &[
            "policy",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "policy.json",
        ],
    );
    assert_success(&output);

    let findings = read_findings(&report_path);
    assert!(
        findings.iter().all(|finding| {
            finding["message"]
                .as_str()
                .is_none_or(|message| !message.contains("net/allowed_cycle.gd"))
        }),
        "allowlisted cycle should be suppressed: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["rule_id"] == "cycle_detected"
                && finding["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("server/disallowed_cycle.gd"))
        }),
        "non-allowlisted cycle should remain: {findings:#?}"
    );

    fs::remove_dir_all(&fixture_root).ok();
}

// ── determinism test ─────────────────────────────────────────────────────────

#[test]
fn outputs_are_deterministic_across_runs() {
    let fixture_root = unique_temp_dir("gdhealth-deterministic");
    write_repo(
        &fixture_root,
        &bundle_config("warning", "[]"),
        &[
            (
                "main.gd",
                r#"extends RefCounted
const HUD = preload("res://levels/main_level/hud/hud_panel.gd")
const NET = preload("res://net/transport.gd")

func alpha() -> void:
    if true:
        print("a")
"#,
            ),
            (
                "net/transport.gd",
                r#"extends RefCounted
const STATE = preload("res://shared/state.gd")
"#,
            ),
            ("shared/state.gd", "extends RefCounted\n"),
            (
                "levels/main_level/hud/hud_panel.gd",
                "extends RefCounted\n",
            ),
        ],
    );

    let graph_a = run_and_read(
        &fixture_root,
        &[
            "graph",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "graph-a.json",
        ],
        "graph-a.json",
    );
    let graph_b = run_and_read(
        &fixture_root,
        &[
            "graph",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "graph-b.json",
        ],
        "graph-b.json",
    );
    assert_eq!(graph_a, graph_b);

    let metrics_a = run_and_read(
        &fixture_root,
        &[
            "metrics",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "metrics-a.json",
        ],
        "metrics-a.json",
    );
    let metrics_b = run_and_read(
        &fixture_root,
        &[
            "metrics",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "metrics-b.json",
        ],
        "metrics-b.json",
    );
    assert_eq!(metrics_a, metrics_b);

    let policy_a = run_and_read(
        &fixture_root,
        &[
            "policy",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "policy-a.json",
        ],
        "policy-a.json",
    );
    let policy_b = run_and_read(
        &fixture_root,
        &[
            "policy",
            "--root",
            ".",
            "--format",
            "json",
            "--output",
            "policy-b.json",
        ],
        "policy-b.json",
    );
    assert_eq!(policy_a, policy_b);

    let emerge_a = run_and_read(
        &fixture_root,
        &[
            "graph",
            "--root",
            ".",
            "--format",
            "emerge-json",
            "--output",
            "emerge-a.json",
        ],
        "emerge-a.json",
    );
    let emerge_b = run_and_read(
        &fixture_root,
        &[
            "graph",
            "--root",
            ".",
            "--format",
            "emerge-json",
            "--output",
            "emerge-b.json",
        ],
        "emerge-b.json",
    );
    assert_eq!(emerge_a, emerge_b);

    fs::remove_dir_all(&fixture_root).ok();
}

// ── browser output test ──────────────────────────────────────────────────────

#[test]
fn graph_emerge_html_writes_browser_bundle() {
    let fixture_root = unique_temp_dir("gdhealth-emerge-html");
    write_repo(
        &fixture_root,
        &bundle_config("warning", "[]"),
        &[
            (
                "items/item_catalog.gd",
                r#"extends RefCounted
const ITEM = preload("res://items/amp_tome.tres")
"#,
            ),
            (
                "items/amp_tome.tres",
                r#"
[gd_resource type="Resource" load_steps=2 format=3]
[ext_resource type="Script" path="res://items/item_profile.gd" id="1"]
[resource]
script = ExtResource("1")
"#,
            ),
            ("items/item_profile.gd", "extends Resource\n"),
        ],
    );

    let output = run_gdhealth(
        &fixture_root,
        &["graph", "--root", ".", "--format", "emerge-html"],
    );
    assert_success(&output);

    let bundle_dir = fixture_root
        .join("reports")
        .join("code-health")
        .join("browser");
    let html = fs::read_to_string(bundle_dir.join("emerge.html")).expect("read html bundle");
    let json =
        fs::read_to_string(bundle_dir.join("graph-emerge.json")).expect("read browser payload");

    assert!(html.contains("gdhealth"));
    assert!(html.contains("metric_fan_in_dependency_graph"));
    assert!(json.contains("\"cluster_metrics_map\""));
    assert!(json.contains("\"items/item_profile.gd\""));

    fs::remove_dir_all(&fixture_root).ok();
}

// ── config validation test ───────────────────────────────────────────────────

#[test]
fn config_validation_rejects_bad_inputs_through_cli() {
    let fixture_root = unique_temp_dir("gdhealth-invalid-config");
    write_repo(
        &fixture_root,
        r#"[discovery]
include = ["["

[thresholds]
cyclomatic_warn = 20
cyclomatic_critical = 10
"#,
        &[("main.gd", "extends RefCounted\n")],
    );

    let output = run_gdhealth(&fixture_root, &["graph", "--root", "."]);
    assert!(
        !output.status.success(),
        "expected invalid config failure, stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(&fixture_root).ok();
}

// ── unsupported format test ──────────────────────────────────────────────────

#[test]
fn unsupported_format_combination_fails_with_message() {
    let fixture_root = unique_temp_dir("gdhealth-unsupported-format");
    write_repo(
        &fixture_root,
        &bundle_config("warning", "[]"),
        &[("main.gd", "extends RefCounted\n")],
    );

    let output = run_gdhealth(
        &fixture_root,
        &["metrics", "--root", ".", "--format", "dot"],
    );
    assert!(
        !output.status.success(),
        "metrics --format dot should fail, stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported") || stderr.contains("does not support"),
        "error message should mention unsupported format, got: {stderr}"
    );

    let output2 = run_gdhealth(
        &fixture_root,
        &["policy", "--root", ".", "--format", "emerge-json"],
    );
    assert!(
        !output2.status.success(),
        "policy --format emerge-json should fail"
    );

    let output3 = run_gdhealth(
        &fixture_root,
        &["health", "--root", ".", "--output", "out.json", "--format", "dot"],
    );
    assert!(
        !output3.status.success(),
        "health --format dot should fail"
    );

    fs::remove_dir_all(&fixture_root).ok();
}

// ── empty project test ───────────────────────────────────────────────────────

#[test]
fn empty_project_exits_zero() {
    let fixture_root = unique_temp_dir("gdhealth-empty-project");
    // Config only, no .gd files
    write_repo(&fixture_root, &bundle_config("warning", "[]"), &[]);

    let output = run_gdhealth(&fixture_root, &["health", "--root", "."]);
    assert_success(&output);

    // Bundle should still be written with empty/minimal content
    let report_dir = fixture_root.join("reports").join("code-health");
    assert!(
        report_dir.join("health.json").is_file(),
        "health.json should exist even for empty project"
    );

    let health_json: Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("health.json")).expect("read health.json"),
    )
    .expect("parse health.json");
    assert_eq!(
        health_json["summary"]["file_count"].as_u64(),
        Some(0),
        "empty project should report 0 files"
    );

    fs::remove_dir_all(&fixture_root).ok();
}

// ── parser warnings test ─────────────────────────────────────────────────────

#[test]
fn parser_warnings_do_not_fail_without_fail_on() {
    let fixture_root = unique_temp_dir("gdhealth-parser-warnings");
    // A file that references a class that doesn't exist → parser warning for unresolved reference
    write_repo(
        &fixture_root,
        &bundle_config("warning", "[]"),
        &[(
            "main.gd",
            r#"extends RefCounted
extends NonExistentClass
"#,
        )],
    );

    let output = run_gdhealth(&fixture_root, &["health", "--root", "."]);
    // Parser warnings alone should not cause failure — only --fail-on triggers exit 1
    assert_success(&output);

    fs::remove_dir_all(&fixture_root).ok();
}

// ── helper: run cargo for help commands (slow but consistent with ci) ─────────

fn run_cargo_help(args: &[&str]) -> Output {
    let mut all_args = vec!["run", "--quiet", "--"];
    all_args.extend_from_slice(args);
    Command::new("cargo")
        .args(all_args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run cargo")
}

// ── shared helpers ────────────────────────────────────────────────────────────

fn bundle_config(forbidden_severity: &str, allow_cycles: &str) -> String {
    format!(
        r#"[project]
root = "."

[discovery]
include = ["**/*.gd"]
exclude = ["addons/**", ".godot/**", "reports/**"]

[groups]
root_gameplay = ["*.gd", "!test/**", "!server/**", "!client/**", "!net/**", "!levels/main_level/hud/**"]
net = ["net/**"]
ai = ["ai/**", "server/ai/**"]
hud = ["levels/main_level/hud/**", "levels/main_level/hud.gd"]
server = ["server/**"]
test = ["test/**"]

[thresholds]
file_lines_warn = 20
file_lines_critical = 40
function_lines_warn = 5
function_lines_critical = 10
cyclomatic_warn = 2
cyclomatic_critical = 4
nesting_warn = 2
nesting_critical = 4
parameters_warn = 3

[policy]
allow_cycles = {allow_cycles}

[[policy.forbidden_dependencies]]
from = "root_gameplay"
to = "hud"
severity = "{forbidden_severity}"
"#
    )
}

fn write_repo(root: &Path, config: &str, files: &[(&str, &str)]) {
    fs::create_dir_all(root).expect("create fixture root");
    fs::write(root.join("gdhealth.toml"), config).expect("write config");

    for (relative_path, content) in files {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(path, content).expect("write fixture file");
    }
}

fn run_gdhealth(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gdhealth"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("failed to run gdhealth")
}

fn run_and_read(root: &Path, args: &[&str], output_file: &str) -> String {
    let output = run_gdhealth(root, args);
    assert_success(&output);
    fs::read_to_string(root.join(output_file)).expect("read generated file")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_findings(path: &Path) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json");
    value["findings"]
        .as_array()
        .expect("findings array")
        .clone()
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()))
}
