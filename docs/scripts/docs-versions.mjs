#!/usr/bin/env node
// Decides which docs versions the deploy builds, from the repo's release tags
// and main's Cargo.toml version. Prints `builds=<json>` and `versions=<json>`
// for $GITHUB_OUTPUT. Run from the repository root with tags fetched.
//
// - main's major equals the latest released major (or nothing is released):
//   main is built at the site root as the stable docs.
// - main's major is higher (a new major is being prepared): main goes to
//   /next, and the root is built from the latest released major.
// - every older released major gets /v<major>.
//
// A released major is built from the branch v<major>.x when it exists, so
// docs fixes for it can land there, else from its latest release tag.
// Refs whose docs predate static export are skipped.

import { execFileSync } from 'node:child_process';

const mainRef = process.env.DOCS_MAIN_REF || 'origin/main';
const git = (...args) => execFileSync('git', args, { encoding: 'utf8' }).trim();

function parse(text) {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(text);
  if (!match) return null;
  return { major: +match[1], minor: +match[2], patch: +match[3], pre: match[4] ?? null };
}

function compare(a, b) {
  if (a.major !== b.major) return a.major - b.major;
  if (a.minor !== b.minor) return a.minor - b.minor;
  if (a.patch !== b.patch) return a.patch - b.patch;
  if (a.pre === b.pre) return 0;
  if (a.pre === null) return 1;
  if (b.pre === null) return -1;
  return a.pre < b.pre ? -1 : 1;
}

function cargoVersion(ref) {
  const toml = git('show', `${ref}:Cargo.toml`);
  const match = toml.match(/^\[(?:workspace\.)?package\][^[]*?^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error(`no package version in Cargo.toml at ${ref}`);
  return match[1];
}

function supportsStaticExport(ref) {
  try {
    return git('show', `${ref}:docs/next.config.mjs`).includes("output: 'export'");
  } catch {
    return false;
  }
}

const releases = git('tag', '--list', 'v*')
  .split('\n')
  .filter(Boolean)
  .map((tag) => ({ tag, version: parse(tag) }))
  .filter((entry) => entry.version && entry.version.pre === null)
  .sort((a, b) => compare(a.version, b.version));

const maintenanceBranches = new Set(
  git('branch', '--remotes', '--list', 'origin/v*.x')
    .split('\n')
    .map((line) => line.trim().replace(/^origin\//, ''))
    .filter(Boolean),
);

function refForMajor(major) {
  const branch = `v${major}.x`;
  if (maintenanceBranches.has(branch)) return `origin/${branch}`;
  return releases.filter((entry) => entry.version.major === major).at(-1).tag;
}

const main = parse(cargoVersion(mainRef));
const releasedMajors = [...new Set(releases.map((entry) => entry.version.major))].sort((a, b) => b - a);
const stableMajor = releasedMajors[0] ?? main.major;

const candidates = [];
if (main.major > stableMajor) {
  candidates.push({ ref: refForMajor(stableMajor), path: '', channel: 'stable' });
  candidates.push({ ref: mainRef, path: 'next', channel: 'next' });
} else {
  candidates.push({ ref: mainRef, path: '', channel: 'stable' });
}
for (const major of releasedMajors.filter((major) => major < stableMajor)) {
  candidates.push({ ref: refForMajor(major), path: `v${major}`, channel: 'archived' });
}

const builds = [];
for (const candidate of candidates) {
  if (!supportsStaticExport(candidate.ref)) {
    console.error(`skipping ${candidate.ref}: its docs do not build as a static export`);
    continue;
  }
  builds.push({
    ...candidate,
    ref: git('rev-parse', `${candidate.ref}^{commit}`),
    label: cargoVersion(candidate.ref),
    artifact: `docs-${candidate.path || 'root'}`,
  });
}

const versions = builds.map(({ label, channel, path }) => ({ label, channel, path }));
console.log(`builds=${JSON.stringify(builds)}`);
console.log(`versions=${JSON.stringify({ versions })}`);
