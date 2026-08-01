use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

// All commit timestamps in these fixtures are fixed relative offsets from a
// hardcoded epoch, never `SystemTime::now()`. Since gdhealth's git signals
// anchor "now" to the repo's own last-commit time (not wall clock), fixed
// offsets make every hotspot/entropy/co-change/bus-factor value 100%
// reproducible regardless of when the test actually runs.
const BASE_TIME: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

#[test]
fn health_reports_git_signals_on_a_real_repo() {
    let root = unique_temp_dir("gdhealth-git-signals");
    init_git_repo(&root);
    write_config(&root);
    write_file(&root, "a.gd", "extends RefCounted\n");
    write_file(&root, "b.gd", "extends RefCounted\n");
    commit_at(&root, "initial commit", BASE_TIME - 10 * DAY, "alice", "alice@example.com");

    write_file(&root, "a.gd", "extends RefCounted\n\nfunc ready() -> void:\n    pass\n");
    commit_at(&root, "fix: null check on ready", BASE_TIME - 2 * DAY, "alice", "alice@example.com");

    let output = run_gdhealth(&root, &["health", "--root", "."]);
    assert_success(&output);

    let health: Value = serde_json::from_str(&read_bundle_file(&root, "health.json"))
        .expect("parse health.json");
    assert_eq!(health["git_available"].as_bool(), Some(true));
    assert_eq!(health["repo_bus_factor"].as_u64(), Some(1));

    let top_churn = health["top_churn_files"].as_array().expect("top_churn_files array");
    assert!(!top_churn.is_empty(), "expected at least one file with git history");
    let a_gd = top_churn
        .iter()
        .find(|entry| entry["file"] == "a.gd")
        .expect("a.gd should appear in top churn files");
    assert_eq!(a_gd["commit_count_total"].as_u64(), Some(2));
    assert_eq!(a_gd["prior_defect_count"].as_u64(), Some(1));

    let summary = fs::read_to_string(root.join("reports/code-health/summary.txt"))
        .expect("read summary.txt");
    assert!(summary.contains("git signals:"), "summary should mention git signals");

    fs::remove_dir_all(&root).ok();
}

#[test]
fn not_a_git_repo_degrades_gracefully() {
    let root = unique_temp_dir("gdhealth-git-no-repo");
    write_config(&root);
    write_file(&root, "a.gd", "extends RefCounted\n");
    // Deliberately no `git init` — this directory is a valid gdhealth
    // project but not a git repository.

    let output = run_gdhealth(&root, &["health", "--root", "."]);
    assert_success(&output);

    let health: Value = serde_json::from_str(&read_bundle_file(&root, "health.json"))
        .expect("parse health.json");
    assert_eq!(health["git_available"].as_bool(), Some(false));
    assert!(health["git_unavailable_reason"].is_string());

    fs::remove_dir_all(&root).ok();
}

#[test]
fn no_git_flag_disables_git_signals_even_inside_a_repo() {
    let root = unique_temp_dir("gdhealth-git-no-git-flag");
    init_git_repo(&root);
    write_config(&root);
    write_file(&root, "a.gd", "extends RefCounted\n");
    commit_at(&root, "initial commit", BASE_TIME, "alice", "alice@example.com");

    let output = run_gdhealth(&root, &["health", "--root", ".", "--no-git"]);
    assert_success(&output);

    let health: Value = serde_json::from_str(&read_bundle_file(&root, "health.json"))
        .expect("parse health.json");
    assert_eq!(health["git_available"].as_bool(), Some(false));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn git_outputs_are_deterministic_across_runs() {
    let root = unique_temp_dir("gdhealth-git-determinism");
    init_git_repo(&root);
    write_config(&root);
    write_file(&root, "a.gd", "extends RefCounted\n");
    write_file(&root, "b.gd", "extends RefCounted\n");
    commit_at(&root, "initial commit", BASE_TIME - 20 * DAY, "alice", "alice@example.com");
    write_file(&root, "b.gd", "extends RefCounted\n\nfunc ready() -> void:\n    pass\n");
    commit_at(&root, "fix: edge case", BASE_TIME - 5 * DAY, "bob", "bob@example.com");

    let first = run_and_read(&root, &["health", "--root", ".", "--format", "json", "--output", "run1.json"], "run1.json");
    let second = run_and_read(&root, &["health", "--root", ".", "--format", "json", "--output", "run2.json"], "run2.json");
    assert_eq!(first, second, "git-derived health output must be byte-identical across runs");

    fs::remove_dir_all(&root).ok();
}

// ── helpers ───────────────────────────────────────────────────────────────

fn init_git_repo(root: &Path) {
    fs::create_dir_all(root).expect("create fixture root");
    run(root, &["init", "-q", "-b", "main"]);
    run(root, &["config", "core.autocrlf", "false"]);
}

fn commit_at(root: &Path, message: &str, unix_time: i64, author_name: &str, author_email: &str) {
    run(root, &["add", "-A"]);
    let date = format!("{unix_time} +0000");
    let output = Command::new("git")
        .current_dir(root)
        .args(["-c", "commit.gpgsign=false", "commit", "-q", "-m", message])
        .env("GIT_AUTHOR_NAME", author_name)
        .env("GIT_AUTHOR_EMAIL", author_email)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_NAME", author_name)
        .env("GIT_COMMITTER_EMAIL", author_email)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .expect("git commit");
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_config(root: &Path) {
    fs::create_dir_all(root).expect("create fixture root");
    fs::write(
        root.join("gdhealth.toml"),
        "[discovery]\ninclude = [\"**/*.gd\"]\nexclude = [\"addons/**\", \".godot/**\", \"reports/**\", \".git/**\"]\n",
    )
    .expect("write config");
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write fixture file");
}

fn read_bundle_file(root: &Path, file_name: &str) -> String {
    fs::read_to_string(root.join("reports").join("code-health").join(file_name))
        .unwrap_or_else(|_| panic!("read {file_name}"))
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

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()))
}
