'use client';

import { usePathname } from 'next/navigation';
import { useEffect, useState } from 'react';

// Every docs version is a separate static build. The deploy writes
// versions.json at the site root, listing them all, so an old build still
// offers the versions published after it.
type DocsVersion = {
  label: string;
  channel: 'stable' | 'next' | 'archived';
  path: string;
};

const SITE_ROOT = process.env.NEXT_PUBLIC_DOCS_SITE_ROOT ?? '';
const VERSION_PATH = process.env.NEXT_PUBLIC_DOCS_VERSION_PATH ?? '';
const VERSION = process.env.NEXT_PUBLIC_DOCS_VERSION ?? '';
const CHANNEL = process.env.NEXT_PUBLIC_DOCS_CHANNEL ?? 'dev';

function versionUrl(path: string, pathname: string) {
  return `${SITE_ROOT}${path ? `/${path}` : ''}${pathname === '/' ? '/' : pathname}`;
}

function describe(version: DocsVersion) {
  if (version.channel === 'stable') return `v${version.label} (latest)`;
  if (version.channel === 'next') return `v${version.label} (next)`;
  return `v${version.label}`;
}

function useVersions() {
  const [versions, setVersions] = useState<DocsVersion[]>([]);
  useEffect(() => {
    fetch(`${SITE_ROOT}/versions.json`)
      .then((response) => (response.ok ? response.json() : null))
      .then((data) => setVersions(Array.isArray(data?.versions) ? data.versions : []))
      .catch(() => setVersions([]));
  }, []);
  return versions;
}

export function VersionSwitcher() {
  const pathname = usePathname();
  const versions = useVersions();

  if (versions.length < 2) {
    return (
      <div className="px-2 text-xs text-fd-muted-foreground">
        v{VERSION}
        {CHANNEL === 'next' && ' (next)'}
      </div>
    );
  }

  return (
    <label className="flex items-center gap-2 px-2 text-xs text-fd-muted-foreground">
      Version
      <select
        className="flex-1 rounded-md border border-fd-border bg-fd-background px-2 py-1 text-fd-foreground"
        value={VERSION_PATH}
        onChange={(event) => {
          window.location.href = versionUrl(event.target.value, pathname);
        }}
      >
        {versions.map((version) => (
          <option key={version.path} value={version.path}>
            {describe(version)}
          </option>
        ))}
      </select>
    </label>
  );
}

// Shown on every page of a version that is not the stable release.
export function VersionNotice() {
  const pathname = usePathname();
  if (CHANNEL !== 'next' && CHANNEL !== 'archived') return null;

  const text =
    CHANNEL === 'next'
      ? `These docs are for v${VERSION}, which is not released yet.`
      : `These docs are for v${VERSION}, an older major version.`;

  return (
    <div className="mb-6 rounded-lg border border-fd-border bg-fd-card px-4 py-3 text-sm">
      {text}{' '}
      <a className="font-medium underline" href={versionUrl('', pathname)}>
        Go to the latest release
      </a>
      .
    </div>
  );
}
