# Publishing

A release is one run of **Cut release**, from the Actions tab. There is no local release command.

Cut release commits the new version and pushes a `v<version>` tag, which starts **Publish release** (`.github/workflows/release.yml`). Publish release checks that the tag matches the version in `Cargo.toml`, builds the archives, publishes to crates.io (trusted publishing, no token), then creates the GitHub release with the archives. Tags with a prerelease suffix (`v1.3.0-rc.1`) become GitHub prereleases and are not sent to gdsuite.

## Releasing

**Actions → Cut release → Run workflow**, on `main`:

| Input | Values |
| --- | --- |
| bump | `patch`, `minor` or `major` |
| pre | `none` for a stable release, or `alpha`, `beta`, `rc` |
| version | an exact version instead, such as `1.3.0-rc.1` (overrides the two above) |
| dry run | pick the version and run the tests, push nothing |

It updates `Cargo.toml`, `Cargo.lock`, the docs package files and the `tool_version` in the example outputs and docs pages, runs `cargo test --locked`, commits `release: v<version>` to `main`, pushes the tag and starts Publish release and the docs deploy (`docs.yml`). The run summary shows the version it picked.

How the version is picked, from the current one:

| Current | bump | pre | Next |
| --- | --- | --- | --- |
| 1.2.3 | patch | none | 1.2.4 |
| 1.2.3 | minor | none | 1.3.0 |
| 1.2.3 | major | none | 2.0.0 |
| 1.2.3 | minor | beta | 1.3.0-beta.1 |
| 1.3.0-beta.1 | minor | beta | 1.3.0-beta.2 |
| 1.3.0-beta.2 | minor | rc | 1.3.0-rc.1 |
| 1.3.0-rc.1 | minor | none | 1.3.0 |
| 1.3.0-rc.1 | major | none | 2.0.0 |

From a prerelease, a bump that already leads to its version stays on it, so keep the same `bump` through the betas, the release candidates and the release. Going back (from `rc` to `beta`) is refused. To see a pick without releasing, run Cut release with **dry run**, or `python scripts/release.py next-version minor rc`.

## When a release fails

- **Publish release failed on a transient error** (runner or network): open the run and use **Re-run failed jobs**.
- **It needs a fix, and crates.io does not have the version yet**: delete the tag (Code → Tags, or `git push origin --delete v<version>`) and the GitHub release if one was created, merge the fix to `main`, then run Cut release with **version** set to the same version. It tags `main` as it is, since the version is already there.
- **crates.io already has the version**: it cannot be published again. Release the fix as the next patch.

## Automation set up once

### Telling gdsuite: `GDSUITE_TRIGGER_TOKEN`

After a stable release, Publish release starts gdsuite's pin-tools workflow, which opens or updates gdsuite's PR pinning this release. That needs the `GDSUITE_TRIGGER_TOKEN` secret here; [gdsuite's RELEASING.md](https://github.com/q-rault/gdsuite/blob/main/RELEASING.md) says how to create it. Without it, the release still succeeds and warns that gdsuite was not told.

### Following gdsyntax: `DEPENDENCY_PR_TOKEN`

**Update gdsyntax** (`.github/workflows/update-gdsyntax.yml`) keeps one PR open, on the `auto/update-gdsyntax` branch, that moves the `gdsyntax = "..."` requirement in `Cargo.toml` and `Cargo.lock` to gdsyntax's latest stable release. gdsyntax's Publish release starts it in gdhealth, gdtest and gdtidy after each stable release; it can also be run from the Actions tab. If CI fails on the PR, the new gdsyntax needs changes: push them to that branch. Merging the PR does not release this tool: run Cut release for that.

A PR opened with a workflow's default token gets no CI, so the workflow uses a token of yours. One token serves the three tools:

1. Account settings (not the repository's) → Developer settings → Personal access tokens → **Fine-grained tokens** → Generate new token.
2. Name it `gdsyntax update PRs`, owner `q-rault`, with an expiration (note the date: the PRs stop getting CI once it expires).
3. Repository access: **Only select repositories** → `q-rault/gdhealth`, `q-rault/gdtest`, `q-rault/gdtidy`.
4. Repository permissions: **Contents: Read and write** and **Pull requests: Read and write**.
5. Store it as the `DEPENDENCY_PR_TOKEN` secret in each of the three (`gh secret set` asks for the value; paste the token):

   ```sh
   for repo in gdhealth gdtest gdtidy; do
     gh secret set DEPENDENCY_PR_TOKEN -R q-rault/$repo
   done
   ```

Without it, the PR is still opened but CI does not run on it (and the repository must allow GitHub Actions to create pull requests: Settings → Actions → General).

The docs site is published from release and prerelease tags; see [docs/README.md](docs/README.md#published-versions).
