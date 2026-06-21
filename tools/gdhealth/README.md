# gdhealth CLI

`gdhealth` is the Rust implementation package for the repository PoC. It analyzes Godot GDScript repositories and exposes `graph`, `metrics`, `policy`, and `health` commands over the same discovery pipeline.

## Current Status

The crate now covers Steps 1 through 4 of the implementation plan:

- config loading from `gdhealth.toml`
- `.gd` discovery with include/exclude filtering
- deterministic graph extraction for `extends`, `class_name`, `preload(...)`, and string-literal `load(...)`
- cycle detection and parser warnings
- function and file metrics, including hotspot scoring
- policy evaluation for forbidden dependencies, size thresholds, complexity thresholds, and cycles
- `--fail-on <info|warning|critical>` enforcement for `policy` and `health`
- default `health` report bundling under `<root>/reports/code-health/`

## Example Commands

```bash
cargo run --manifest-path tools/gdhealth/Cargo.toml -- graph --root . --format text
cargo run --manifest-path tools/gdhealth/Cargo.toml -- graph --root . --format json --output reports/code-health/graph.json
cargo run --manifest-path tools/gdhealth/Cargo.toml -- metrics --root . --format text
cargo run --manifest-path tools/gdhealth/Cargo.toml -- policy --root . --format text --fail-on critical
cargo run --manifest-path tools/gdhealth/Cargo.toml -- health --root . --config gdhealth.toml --format json --output reports/code-health/health.json
cargo run --manifest-path tools/gdhealth/Cargo.toml -- health --root .
```

## Supported Today

- deterministic `.gd` discovery under a repo root
- dependency extraction for:
  - `extends "res://..."`
  - `extends SomeClassName`
  - `class_name`
  - `preload("...")`
  - string-literal `load("...")`
- repo-owned `.gd` path resolution from `res://` and relative paths
- file-level and function-level metrics
- config-driven policy findings with severity
- non-zero exit behavior through `--fail-on`
- default `health` bundle output:
  - `graph.json`
  - `graph.dot`
  - `metrics.json`
  - `policy.json`
  - `health.json`
  - `summary.txt`

## Known Limitations

- no semantic parser; extraction is tolerant and text-based
- dynamic `load(variable)` calls are not resolved
- only `.gd` dependencies become graph edges
- duplicate `class_name` declarations surface as warnings rather than being disambiguated
- `health --output <file>` switches back to single-file output instead of bundle generation
