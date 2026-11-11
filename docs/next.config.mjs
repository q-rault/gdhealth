import fs from 'node:fs';
import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

// The site lives at https://q-rault.github.io/gdhealth/. Each docs version is
// its own static build under that root: the stable release at the root, an
// older major under /v0, an unreleased major under /next (see
// scripts/docs-versions.mjs). Set DOCS_SITE_ROOT='' for a domain root.
const siteRoot = process.env.DOCS_SITE_ROOT ?? '/gdhealth';
const versionPath = process.env.DOCS_VERSION_PATH ?? '';
const basePath = versionPath ? `${siteRoot}/${versionPath}` : siteRoot;

function cargoVersion() {
  const toml = fs.readFileSync(new URL('../Cargo.toml', import.meta.url), 'utf8');
  const match = toml.match(/^\[(?:workspace\.)?package\][^[]*?^version\s*=\s*"([^"]+)"/m);
  return match ? match[1] : '0.0.0';
}

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  output: 'export',
  basePath,
  images: { unoptimized: true },
  env: {
    NEXT_PUBLIC_BASE_PATH: basePath,
    NEXT_PUBLIC_DOCS_SITE_ROOT: siteRoot,
    NEXT_PUBLIC_DOCS_VERSION_PATH: versionPath,
    NEXT_PUBLIC_DOCS_VERSION: cargoVersion(),
    // stable, next, archived, or dev for a local build.
    NEXT_PUBLIC_DOCS_CHANNEL: process.env.DOCS_CHANNEL ?? 'dev',
  },
};

export default withMDX(config);
