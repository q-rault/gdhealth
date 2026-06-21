# gdhealth CLI

`gdhealth` is the implementation package for the repository PoC.

## Current Status

This crate currently provides the Step 1 bootstrap from the implementation plan:

- CLI entrypoint with `graph`, `metrics`, `policy`, and `health` subcommands
- config loading from `gdhealth.toml`
- `.gd` file discovery with include/exclude filtering
- text, JSON, and DOT output plumbing

The current graph and reporting logic is intentionally minimal. Dependency extraction, metrics enrichment, policy evaluation, and aggregate health logic will be expanded in later steps without changing the command surface.

## Example Commands

```bash
cargo run --manifest-path tools/gdhealth/Cargo.toml -- graph --root . --format text
cargo run --manifest-path tools/gdhealth/Cargo.toml -- graph --root . --format json --output reports/code-health/graph.json
cargo run --manifest-path tools/gdhealth/Cargo.toml -- metrics --root . --format text
cargo run --manifest-path tools/gdhealth/Cargo.toml -- health --root . --config gdhealth.toml --format json --output reports/code-health/health.json
```

## Supported Today

- discovery of `.gd` files under a repo root
- deterministic file ordering
- optional exclusion of `test/**`
- config defaults when `gdhealth.toml` is absent

## Planned Next

- tolerant dependency extraction from `extends`, `preload`, and `load`
- function and file metrics based on GDScript structure
- policy checks for forbidden dependencies and thresholds
- richer summary output under `reports/code-health/`

## Known Limitations

- no semantic analysis
- no real dependency edges yet
- no function-level parsing yet
- no policy findings yet
