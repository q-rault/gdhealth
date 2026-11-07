# gdhealth

`gdhealth` is a code-health CLI for Godot/GDScript repositories. It extracts dependency graphs, computes complexity metrics, evaluates architecture policies, and bundles actionable reports — without requiring Godot to run.

## Why use it

New to code-health terminology? Read the [Understanding Code Health guide](docs/content/docs/understanding-code-health.mdx).

- Surface dependency cycles and architecture violations before they compound
- Identify the highest-complexity files and functions as refactor targets
- Enforce size, complexity, and dependency rules in CI with exit-code semantics
- Produce stable, machine-readable JSON artifacts for trend tracking and tooling
- See which files actually churn and carry bug history, via git (ownership, co-change, hotspots)
- Score a commit or PR range for defect risk from diff shape alone
- Flag likely copy-pasted code and get concrete "cut this edge" / "split this file" suggestions

## Install

```sh
cargo install gdhealth
```

Requires Rust 1.96+. Prebuilt binaries are attached to each [GitHub release](https://github.com/q-rault/gdhealth/releases).

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

| Command                     | What it does                                                       |
|------------------------------|---------------------------------------------------------------------|
| `gdhealth graph`             | Extract and render the file dependency graph                        |
| `gdhealth metrics`           | Compute file/function complexity metrics, plus duplicate-code hints |
| `gdhealth policy`            | Evaluate architecture and threshold policies                        |
| `gdhealth health`            | Bundle graph + metrics + policy + git signals + summary (recommended) |
| `gdhealth risk <revspec>`    | Score a commit or `base..head` range for defect risk (diff shape only) |
| `gdhealth validate-defects`  | Self-check: do the worst-hotspot files actually carry bug history?  |

Each command accepts `--root <path>`, `--format <format>`, `--output <file>`, and `--fail-on <severity>`
(`--fail-on` has no effect on `risk`/`validate-defects`, which are diagnostic-only). `health`, `risk`,
and `validate-defects` additionally accept `--no-git` to skip git-derived signals.

For tooling integration, `gdhealth` also exposes stable probe commands:

```sh
gdhealth --version
gdhealth --schema-version
gdhealth schema-version
```

## Output formats

| Format         | `graph` | `metrics` | `policy` | `health` |
|----------------|:-------:|:---------:|:--------:|:--------:|
| `text`         | ✓       | ✓         | ✓        | ✓        |
| `json`         | ✓       | ✓         | ✓        | ✓        |
| `dot`          | ✓       |           |          |          |
| `emerge-json`  | ✓       |           |          |          |
| `emerge-html`  | ✓       |           |          |          |

The `emerge-html` format writes a self-contained browser viewer to `reports/code-health/browser/`.

## Git-derived signals

`gdhealth health` shells out to the system `git` binary (no `git2`/libgit2 dependency) to compute,
per file: commit frequency and recency, a temporal churn-hotspot score, ownership via `git blame`,
co-change partners, change entropy, and prior-defect count (mined from fix-commit-shaped messages).
This is always on when the project is a git repository; pass `--no-git` or set `[git] enabled = false`
in `gdhealth.toml` to skip it.

`gdhealth risk <revspec>` scores a single commit or a `base..head` range from diff shape alone (size,
diffusion across files/directories, author experience), compared percentile-wise with this repository's
recent commits. It is a transparent heuristic to focus review, not a statistically validated defect predictor.

`gdhealth validate-defects` checks whether the existing structural hotspot score actually correlates
with files that had real bugs (via the git prior-defect signal above), and prints "insufficient data"
rather than a misleading number on small or young repositories.

## Current limits (new features)

See [Understanding Code Health](docs/content/docs/understanding-code-health.mdx) for the practical limits of text-based static analysis and how to interpret scores.

The git intelligence layer, `risk`, `validate-defects`, duplicate-code detection, and the
`suggested_fix` field on cycle/split-file findings are new and still settling — treat their exact
fields as informational rather than a stable API surface (see [schema reference](docs/content/docs/schema-reference.mdx)).
In particular: `gdhealth risk` is a repository-relative heuristic (see above), and Split File's
community-detection heuristic is intentionally conservative — it currently emits no suggestion on
codebases where the reused Louvain-style optimizer fragments a file into many small communities
rather than 2-6 cohesive ones, since no suggestion is better than an unhelpful 13-way split.

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
