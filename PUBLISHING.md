# Publishing

`gdhealth` releases are source-first and tag-driven. The version source of truth is the committed repo state:

- `Cargo.toml`
- aligned version-bearing docs/example files updated by `just bump`
- a matching Git tag `v<version>`

GitHub Actions only builds and publishes artifacts from the pushed tag. It does not edit manifests, create tags, or publish to crates.io: run `cargo publish` once the release workflow succeeds.

## Release

From a clean `main` checkout:

```sh
just release 0.2.0
just release 0.2.0-beta.1
```

`just release` will:

1. verify the git worktree is clean
2. validate the version string
3. update all version-bearing files
4. run `cargo test`
5. create commit `release: v<version>`
6. create tag `v<version>`
7. push `main` and the tag

## Failed release recovery

If the GitHub release workflow fails but the tag already points to the correct release commit, prefer fixing the workflow and re-running the job.

If the fix requires a new commit and the original release did not publish successfully, move the same tag to the new commit instead of inventing a patch version:

```sh
just retag-release 0.2.0
```

Equivalent raw git commands:

```sh
git tag -f v0.2.0
git push origin -f v0.2.0
```
