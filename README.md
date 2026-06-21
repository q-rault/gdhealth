# gdhealth

`gdhealth` is a Rust-based code-health CLI for Godot/GDScript repositories.

This repository starts from a PoC plan focused on one goal: produce deterministic, actionable code-health reports for a GDScript codebase without requiring Godot to run.

## PoC Objective

The first implementation target is a single Rust package that can:

- discover repo-owned `.gd` files
- extract lightweight dependency information
- compute file and function hotspot metrics
- evaluate a small set of architecture and threshold policies
- emit machine-readable reports plus a readable text summary

Success for the PoC means the tool can surface obvious structural and complexity hotspots in a target repository and generate stable outputs suitable for refactor planning.

## Planned Repository Layout

The implementation will follow this structure:

```text
.
|-- README.md
|-- gdhealth.toml
|-- reports/
|   `-- code-health/
`-- tools/
    `-- gdhealth/
        |-- Cargo.toml
        |-- README.md
        |-- src/
        `-- tests/
```

The tool itself stays in a single Cargo package under `tools/gdhealth/`.

## Planned CLI Surface

The PoC CLI will expose four subcommands:

- `gdhealth graph`
- `gdhealth metrics`
- `gdhealth policy`
- `gdhealth health`

Shared options will include:

- `--root <path>`
- `--config <path>`
- `--format <json|text|dot>`
- `--output <path>`
- `--include-tests`
- `--fail-on <severity>`

## Scope

In scope:

- `.gd` discovery with include/exclude filtering
- tolerant extraction of selected dependency constructs
- file/function/class metrics
- hotspot ranking
- config-driven policy checks
- JSON output
- DOT graph output
- text summary output

Out of scope:

- full GDScript semantic analysis
- dead-code detection with strong guarantees
- auto-fixes
- formatting
- type inference
- editor integration
- multi-crate decomposition

## Supported Analysis Strategy

The PoC intentionally favors tolerant extraction over full parsing.

Initial dependency extraction targets:

- `extends "res://..."`
- `extends SomeClassName`
- `class_name Foo`
- `const X = preload("res://...")`
- `preload("res://...")`
- `load("res://...")`

Initial metric extraction targets:

- `func name(...)`
- indentation-based block tracking
- control-flow markers such as `if`, `elif`, `else`, `for`, `while`, and `match`

## Expected Outputs

Generated reports will live under `reports/code-health/`:

- `graph.json`
- `graph.dot`
- `metrics.json`
- `policy.json`
- `health.json`
- `summary.txt`

Optional later output:

- `health.html`
- `graph --format emerge-html`

## Implementation Priorities

1. bootstrap a usable CLI before refining parser sophistication
2. keep outputs deterministic and trustworthy
3. keep repo-specific assumptions in `gdhealth.toml`
4. preserve stable command names and JSON schema shape from the start
5. avoid overengineering the package structure

## Initial Roadmap

1. Bootstrap the Rust crate in `tools/gdhealth` with CLI, config loading, and discovery.
2. Implement dependency graph extraction and export to JSON and DOT.
3. Implement metric extraction and hotspot scoring.
4. Add config-driven policy evaluation.
5. Build aggregate health reporting and output generation.
6. Harden with deterministic ordering, fixtures, and integration tests.

## Non-Goals

This repository is not intended to become:

- a full semantic linter
- an auto-fix tool
- a complete dead-code detector
- a Godot editor plugin

## Status

Current state:

- plan imported from `poc-plan-single-crate-rust-code-health-tool-for-godot-gdscript.md`
- git repository initialized
- repository README established

Next implementation target:

- bootstrap `tools/gdhealth` and make `gdhealth graph --root . --format text` run end to end
