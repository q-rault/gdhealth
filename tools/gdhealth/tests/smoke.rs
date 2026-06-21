use std::process::Command;

#[test]
fn graph_help_succeeds() {
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--", "graph", "--help"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run cargo");

    assert!(output.status.success());
}
