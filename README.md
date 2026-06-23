# gdhealth

`gdhealth` is a code-health CLI for Godot/GDScript repositories. It extracts dependency graphs, computes complexity metrics, evaluates architecture policies, and bundles actionable reports — without requiring Godot to run.

## Why use it

- Surface dependency cycles and architecture violations before they compound
- Identify the highest-complexity files and functions as refactor targets
- Enforce size, complexity, and dependency rules in CI with exit-code semantics
- Produce stable, machine-readable JSON artifacts for trend tracking and tooling

## Install

Requires Rust 1.85+.

```sh
cargo install --path tools/gdhealth
```

Or build without installing:

```sh
cargo build --release --manifest-path tools/gdhealth/Cargo.toml
```

## Quickstart

**1. Create a `gdhealth.toml` at your project root:**

```toml
[discovery]
include = ["**/*.gd"]
exclude = ["addons/**", ".godot/**", "reports/**"]

[groups]
gameplay = ["*.gd", "!server/**", "!net/**"]
net      = ["net/**"]
server   = ["server/**"]

[thresholds]
file_lines_warn     = 500
file_lines_critical = 1000
cyclomatic_warn     = 10
cyclomatic_critical = 15

[policy]
allow_cycles = []

[[policy.forbidden_dependencies]]
from     = "server"
to       = "gameplay"
severity = "critical"
```

**2. Run the health command:**

```sh
gdhealth health --root .
```

**3. Inspect the bundle written to `reports/code-health/`:**

```
reports/code-health/
├── summary.txt    ← human-readable summary
├── health.json    ← machine-readable aggregate
├── graph.json     ← full dependency graph
├── graph.dot      ← GraphViz format
├── metrics.json   ← file and function metrics
└── policy.json    ← policy findings
```

```sh
cat reports/code-health/summary.txt
```

## Commands

| Command            | What it does                                              |
|--------------------|-----------------------------------------------------------|
| `gdhealth graph`   | Extract and render the file dependency graph              |
| `gdhealth metrics` | Compute file and function-level complexity metrics        |
| `gdhealth policy`  | Evaluate architecture and threshold policies              |
| `gdhealth health`  | Bundle graph + metrics + policy + summary (recommended)   |

Each command accepts `--root <path>`, `--format <format>`, `--output <file>`, and `--fail-on <severity>`.

## Output formats

| Format         | `graph` | `metrics` | `policy` | `health` |
|----------------|:-------:|:---------:|:--------:|:--------:|
| `text`         | ✓       | ✓         | ✓        | ✓        |
| `json`         | ✓       | ✓         | ✓        | ✓        |
| `dot`          | ✓       |           |          |          |
| `emerge-json`  | ✓       |           |          |          |
| `emerge-html`  | ✓       |           |          |          |

The `emerge-html` format writes a self-contained browser viewer to `reports/code-health/browser/`.

## CI usage

Use `--fail-on` to control exit code behavior:

```sh
# Exit 1 if any critical findings are present
gdhealth health --root . --fail-on critical

# Exit 1 if any warning or critical findings are present
gdhealth policy --root . --format json --fail-on warning
```

**Exit codes:**

| Code | Meaning                                          |
|------|--------------------------------------------------|
| `0`  | Success, no findings at or above threshold       |
| `1`  | Config error, I/O error, unsupported format, or findings at/above `--fail-on` level |

**Example GitHub Actions step:**

```yaml
- name: Run gdhealth
  run: gdhealth health --root . --fail-on critical
```

## Configuration

All behavior is driven by `gdhealth.toml`. Place it at your project root or pass `--config <path>`.

See the [full configuration reference](docs/content/docs/configuration.mdx) for all options.

## How it fits into gdsuite

`gdhealth` is part of the [gdsuite](https://github.com/q-rault/gdsuite) platform for Godot development tooling.

`gdsuite health` invokes `gdhealth health` as its backend. It consumes:
- exit code for pass/fail signaling
- `reports/code-health/summary.txt` for human display
- `reports/code-health/health.json` for machine aggregation

Advanced features (browser graph, DOT export, entity scan) are gdhealth-specific and available directly via the CLI.

## Current limits

- The parser is text-based, not a full GDScript semantic parser
- Dynamic `load(variable)` calls are not resolved (only string-literal `load("res://...")` is)
- Only `.gd` dependencies become graph edges (`.tres` resource references are tracked separately via `ExtResource` scanning)
- Duplicate `class_name` declarations surface as parser warnings rather than being disambiguated
- `gdhealth health --output <file>` writes a single file instead of the full bundle
