#!/usr/bin/env node
// Decides which docs versions the deploy builds. Prints `builds=<json>` and
// `versions=<json>` for $GITHUB_OUTPUT. Run from the repository root with
// tags fetched.
//
// Every published version comes from a tag:
//
// - `/`        the latest release tag (vX.Y.Z), the stable docs.
// - `/next`    the latest prerelease tag (vX.Y.Z-beta.1, -rc.1...), only
//              while it is newer than the latest release: push a prerelease
//              tag to publish a preview, release to retire it.
// - `/v<N>`    the last release tag of major N, for each N listed in
//              `archived_majors` in docs/versions.config.json on main. Remove
//              a major from the list to unpublish it.
//
// Until a repository makes its first release with these docs, its latest
// release tag cannot be built as a static site; the stable docs are then
// built from main instead, and the next release ends that.

import { execFileSync } from 'node:child_process';

const mainRef = process.env.DOCS_MAIN_REF || 'origin/main';
const git = (...args) =>
  execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();

function parse(text) {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(text);
  if (!match) return null;
  return { major: +match[1], minor: +match[2], patch: +match[3], pre: match[4] ?? null };
}

function compareIdentifiers(a, b) {
  const left = a.split('.');
  const right = b.split('.');
  for (let i = 0; i < Math.max(left.length, right.length); i += 1) {
    if (left[i] === undefined) return -1;
    if (right[i] === undefined) return 1;
    const numeric = /^\d+$/.test(left[i]) && /^\d+$/.test(right[i]);
    const order = numeric ? +left[i] - +right[i] : left[i].localeCompare(right[i]);
    if (order !== 0) return order;
  }
  return 0;
}

function compare(a, b) {
  if (a.major !== b.major) return a.major - b.major;
  if (a.minor !== b.minor) return a.minor - b.minor;
  if (a.patch !== b.patch) return a.patch - b.patch;
  if (a.pre === b.pre) return 0;
  if (a.pre === null) return 1;
  if (b.pre === null) return -1;
  return compareIdentifiers(a.pre, b.pre);
}

function cargoVersion(ref) {
  const toml = git('show', `${ref}:Cargo.toml`);
  const match = toml.match(/^\[(?:workspace\.)?package\][^[]*?^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error(`no package version in Cargo.toml at ${ref}`);
  return match[1];
}

function buildable(ref) {
  try {
    return git('show', `${ref}:docs/next.config.mjs`).includes("output: 'export'");
  } catch {
    return false;
  }
}

function readConfig() {
  try {
    return JSON.parse(git('show', `${mainRef}:docs/versions.config.json`));
  } catch {
    return {};
  }
}

const tags = git('tag', '--list', 'v*')
  .split('\n')
  .filter(Boolean)
  .map((tag) => ({ tag, version: parse(tag) }))
  .filter((entry) => entry.version)
  .sort((a, b) => compare(a.version, b.version));
const releases = tags.filter((entry) => entry.version.pre === null);
const latest = releases.at(-1);

const builds = [];
function add(ref, path, channel) {
  builds.push({
    ref: git('rev-parse', `${ref}^{commit}`),
    path,
    channel,
    label: cargoVersion(ref),
    artifact: `docs-${path || 'root'}`,
    // Built from main rather than a tag (see the header).
    main: ref === mainRef,
  });
}

// Stable.
if (latest && buildable(latest.tag)) {
  add(latest.tag, '', 'stable');
} else {
  console.error(
    latest
      ? `${latest.tag} predates the static docs: building the stable docs from main until the next release`
      : 'no release yet: building the stable docs from main',
  );
  add(mainRef, '', 'stable');
}

// Preview.
const preview = tags.at(-1);
if (preview && preview.version.pre !== null && (!latest || compare(preview.version, latest.version) > 0)) {
  if (buildable(preview.tag)) add(preview.tag, 'next', 'next');
  else console.error(`skipping ${preview.tag}: its docs do not build as a static site`);
}

// Archived majors.
const archived = [...new Set(readConfig().archived_majors ?? [])].sort((a, b) => b - a);
for (const major of archived) {
  const last = releases.filter((entry) => entry.version.major === major).at(-1);
  if (!last) {
    console.error(`archived_majors: no release of major ${major}, skipping it`);
  } else if (major >= latest.version.major) {
    // `last` exists, so `latest` does too.
    console.error(`archived_majors: ${major} is the stable major, skipping it`);
  } else if (!buildable(last.tag)) {
    console.error(`archived_majors: ${last.tag} predates the static docs, skipping it`);
  } else {
    add(last.tag, `v${major}`, 'archived');
  }
}

const versions = builds.map(({ label, channel, path }) => ({ label, channel, path }));
console.log(`builds=${JSON.stringify(builds)}`);
console.log(`versions=${JSON.stringify({ versions })}`);
