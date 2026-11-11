# Publishing

`gdhealth` releases are tag-driven. Pushing a `v<version>` tag starts **Publish release** (`.github/workflows/release.yml`), which checks that the tag matches the version in `Cargo.toml`, publishes to crates.io (trusted publishing, no token), builds the archives, then creates the GitHub release. Tags with a prerelease suffix (`v0.4.0-rc.1`) become GitHub prereleases and are not sent to gdsuite.

The version source of truth is the committed repo state: `Cargo.toml`, `Cargo.lock` and the version-bearing docs and example files that `scripts/release.py bump` updates.

## Release from GitHub

**Actions → Cut release → Run workflow**, on `main`:

| Input | Values |
| --- | --- |
| bump | `patch`, `minor` or `major` |
| pre | `none` for a stable release, or `alpha`, `beta`, `rc` |
| version | an exact version instead, such as `0.4.0-rc.1` (overrides the two above) |
| dry run | pick the version and run the tests, push nothing |

It bumps the files and `Cargo.lock`, runs the tests, commits `release: v<version>` to `main`, pushes the tag and starts Publish release and `docs.yml`. The run summary shows the version it picked.

How the version is picked, from the current one:

| Current | bump | pre | Next |
| --- | --- | --- | --- |
| 0.3.2 | patch | none | 0.3.3 |
| 0.3.2 | minor | none | 0.4.0 |
| 0.3.2 | major | none | 1.0.0 |
| 0.3.2 | minor | beta | 0.4.0-beta.1 |
| 0.4.0-beta.1 | minor | beta | 0.4.0-beta.2 |
| 0.4.0-beta.2 | minor | rc | 0.4.0-rc.1 |
| 0.4.0-rc.1 | minor | none | 0.4.0 |
| 0.4.0-rc.1 | major | none | 1.0.0 |

From a prerelease, a bump that already leads to its version stays on it, so keep the same `bump` through the betas, the release candidates and the release. Going back (from `rc` to `beta`) is refused. Check a pick without releasing with `python scripts/release.py next-version minor rc`.

## Release from a checkout

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

If Publish release fails but the tag already points to the correct release commit, prefer fixing the workflow and re-running the job.

If the fix requires a new commit and the original release did not publish to crates.io, move the same tag to the new commit instead of inventing a patch version. Pushing the moved tag starts Publish release again:

```sh
just retag-release 0.2.0
```

Equivalent raw git commands:

```sh
git tag -f v0.2.0
git push origin -f v0.2.0
```

Once crates.io has a version, it cannot be published again: release the next patch instead.

The docs site is published from release and prerelease tags; see [docs/README.md](docs/README.md#published-versions).
