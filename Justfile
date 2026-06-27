set dotenv-load := true
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

test: test-rust

test-rust:
    cargo test --quiet

build:
    cargo build --release --quiet

check-clean-git:
    python scripts/release.py check-clean-git

check-release-version NEW_VER:
    python scripts/release.py check-release-version {{NEW_VER}}

check-version-sync:
    python scripts/release.py check-version-sync

bump NEW_VER:
    python scripts/release.py bump {{NEW_VER}}

release NEW_VER:
    just check-clean-git
    just check-release-version "{{NEW_VER}}"
    just bump "{{NEW_VER}}"
    just check-version-sync
    cargo test --quiet
    git add Cargo.toml Cargo.lock docs/package.json docs/package-lock.json tests docs README.md .github/workflows/release.yml .gitignore Justfile PUBLISHING.md scripts/release.py
    git commit -m "release: v{{NEW_VER}}"
    git tag "v{{NEW_VER}}"
    git push origin main
    git push origin "v{{NEW_VER}}"

retag-release NEW_VER:
    just check-release-version "{{NEW_VER}}"
    git tag -f "v{{NEW_VER}}"
    git push origin -f "v{{NEW_VER}}"
