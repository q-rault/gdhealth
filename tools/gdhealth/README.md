# gdhealth CLI

`gdhealth` is a code-health CLI for Godot/GDScript repositories. See the [project README](../../README.md) for full documentation.

## Build

From `tools/gdhealth`:

```sh
cargo install --path . --force
```

Or from the repository root:

```sh
cargo install --path tools/gdhealth
```

## Run tests

```sh
cargo test -p gdhealth
```

## Quick reference

```sh
gdhealth health --root .                        # full bundle → reports/code-health/
gdhealth health --root . --fail-on critical     # exit 1 on critical findings
gdhealth graph  --root . --format dot           # GraphViz dependency graph
gdhealth graph  --root . --format emerge-html   # interactive browser viewer
gdhealth metrics --root . --format json         # file and function metrics
gdhealth policy  --root . --format json         # policy findings only
```
