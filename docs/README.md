# gdhealth docs

The site at https://q-rault.github.io/gdhealth/, built with [Fumadocs](https://fumadocs.dev). Pages are in `content/docs/`, the sidebar order in `content/docs/meta.json`.

```sh
npm ci
npm run dev      # http://localhost:3000/gdhealth
npm run build    # static site in out/
```

## Published versions

Every published version is built from a tag by `.github/workflows/docs.yml`:

| Path | Built from | Published |
| --- | --- | --- |
| `/` | the latest release tag, `vX.Y.Z` | always |
| `/next` | the latest prerelease tag, such as `v2.0.0-beta.1` or `v2.0.0-rc.1` | while it is newer than the latest release |
| `/v<major>` | the last release tag of that major | for each major listed in `versions.config.json` |

- **Stable docs** change when you release. Docs merged to `main` go live with the next release tag.
- **A preview** starts when you push a prerelease tag; each new prerelease tag updates it, and the release removes it.
- **Keeping an older major**: after releasing `v1.0.0`, add `0` to `archived_majors` in `versions.config.json` to keep the 0.x docs at `/v0`. To fix them, release a `v0.*` patch from a branch. Remove a major from the list to unpublish it; the version switcher, which reads `versions.json` at the site root, drops it everywhere at the next deploy.

The switcher appears once two versions are published. `scripts/docs-versions.mjs` decides what to build; tags made before the docs became a static site can't be built, so until the first release that has them, the stable docs are built from `main`.
