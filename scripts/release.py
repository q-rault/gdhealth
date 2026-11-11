import pathlib
import re
import subprocess
import sys


SEMVER_RE = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?")
ROOT = pathlib.Path(__file__).resolve().parent.parent


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf8")


def replace(path: str, pattern: str, repl: str, *, flags: int = re.MULTILINE, count: int = 1) -> None:
    file_path = ROOT / path
    # Read and write without newline translation: the repo is LF on every platform.
    original = file_path.read_bytes().decode("utf8").replace(chr(13), "")
    text, changed = re.subn(pattern, repl, original, count=count, flags=flags)
    if changed < 1:
        raise SystemExit(f"ERROR: pattern not found in {path}")
    file_path.write_bytes(text.encode("utf8"))
    print(f"  {path}")


def check_clean_git() -> None:
    result = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    if result.stdout.strip():
        sys.stderr.write("ERROR: working tree must be clean before release\n")
        sys.stderr.write(result.stdout)
        raise SystemExit(1)


def check_release_version(version: str) -> None:
    if not SEMVER_RE.fullmatch(version):
        raise SystemExit(
            f"ERROR: invalid release version '{version}'. Expected semver like 0.2.0 or 0.2.0-beta.1"
        )


def cargo_version() -> str:
    match = re.search(r'^version\s*=\s*"([^"]+)"', read("Cargo.toml"), re.MULTILINE)
    if match is None:
        raise SystemExit("ERROR: failed to read version from Cargo.toml")
    return match.group(1)


def check_version_sync() -> None:
    expected = cargo_version()
    checks = [
        ("docs/package.json", r'"version"\s*:\s*"([^"]+)"'),
        ("docs/package-lock.json", r'"version"\s*:\s*"([^"]+)"'),
        (
            "docs/package-lock.json#root",
            r'""\s*:\s*\{\s*"name"\s*:\s*"gdhealth-docs",\s*"version"\s*:\s*"([^"]+)"',
        ),
        ("tests/docs/example_output/health-snippet.json", r'"tool_version"\s*:\s*"([^"]+)"'),
        ("tests/docs/example_output/godot-open-rpg-health.json", r'"tool_version"\s*:\s*"([^"]+)"'),
        ("docs/content/docs/showcase.mdx", r'"tool_version":\s*"([^"]+)"'),
        ("docs/content/docs/schema-reference.mdx", r'"tool_version":\s*"([^"]+)"'),
    ]

    mismatches = []
    for path, pattern in checks:
        match = re.search(pattern, read(path.split("#", 1)[0]), re.MULTILINE | re.DOTALL)
        if match is None or match.group(1) != expected:
            mismatches.append(path)

    if mismatches:
        sys.stderr.write(f"ERROR: version mismatch against Cargo.toml {expected}\n")
        for path in mismatches:
            sys.stderr.write(f"  {path}\n")
        raise SystemExit(1)


def bump(version: str) -> None:
    check_release_version(version)
    print(f"bumping to {version}")
    replace("Cargo.toml", r'^(version\s*=\s*)"[^"]+"', f'\\g<1>"{version}"')
    replace("docs/package.json", r'("version"\s*:\s*)"[^"]+"', f'\\g<1>"{version}"')
    replace("docs/package-lock.json", r'("version"\s*:\s*)"[^"]+"', f'\\g<1>"{version}"')
    replace(
        "docs/package-lock.json",
        r'(""\s*:\s*\{\s*"name"\s*:\s*"gdhealth-docs",\s*"version"\s*:\s*)"[^"]+"',
        f'\\g<1>"{version}"',
        flags=re.MULTILINE | re.DOTALL,
    )
    replace("tests/docs/example_output/health-snippet.json", r'("tool_version"\s*:\s*)"[^"]+"', f'\\g<1>"{version}"')
    replace("tests/docs/example_output/godot-open-rpg-health.json", r'("tool_version"\s*:\s*)"[^"]+"', f'\\g<1>"{version}"')
    replace("docs/content/docs/showcase.mdx", r'("tool_version":\s*)"[^"]+"', f'\\g<1>"{version}"', count=2)
    replace("docs/content/docs/schema-reference.mdx", r'("tool_version":\s*)"[^"]+"', f'\\g<1>"{version}"')
    print("done - run `just check-version-sync` before releasing")


PRE_KINDS = ("alpha", "beta", "rc")
PARTS_RE = re.compile(r"([0-9]+)\.([0-9]+)\.([0-9]+)(?:-(alpha|beta|rc)\.([0-9]+))?")


def next_version(current: str, level: str, pre: str) -> str:
    """The version after `current` for a `level` bump (patch, minor, major),
    as a prerelease when `pre` is alpha, beta or rc, else a release.

    From a release, the level bump comes first: 0.3.2 + minor + beta -> 0.4.0-beta.1.
    From a prerelease, a level whose bump already leads to its version stays on it:
    0.4.0-beta.1 + minor + beta -> 0.4.0-beta.2, + rc -> 0.4.0-rc.1, + none -> 0.4.0.
    A level it does not cover starts a new version: 0.4.0-rc.1 + major -> 1.0.0.
    """
    if level not in ("patch", "minor", "major"):
        raise SystemExit(f"ERROR: unknown bump '{level}', expected patch, minor or major")
    if pre not in ("none", *PRE_KINDS):
        raise SystemExit(f"ERROR: unknown prerelease '{pre}', expected none, alpha, beta or rc")
    match = PARTS_RE.fullmatch(current)
    if match is None:
        raise SystemExit(f"ERROR: cannot bump '{current}': expected X.Y.Z or X.Y.Z-(alpha|beta|rc).N")
    major, minor, patch = (int(part) for part in match.group(1, 2, 3))
    cur_pre, cur_num = match.group(4), int(match.group(5) or 0)

    # A prerelease of X.Y.Z already counts as a bump to X.Y.Z when the level
    # does not reach further: any level for X.0.0, minor or patch for X.Y.0.
    stays = cur_pre is not None and (
        level == "patch"
        or (level == "minor" and patch == 0)
        or (level == "major" and minor == 0 and patch == 0)
    )
    if not stays:
        if level == "major":
            major, minor, patch = major + 1, 0, 0
        elif level == "minor":
            minor, patch = minor + 1, 0
        else:
            patch += 1

    core = f"{major}.{minor}.{patch}"
    if pre == "none":
        return core
    if stays:
        if PRE_KINDS.index(pre) < PRE_KINDS.index(cur_pre):
            raise SystemExit(f"ERROR: {current} is already past {pre}: pick {cur_pre} or later")
        if pre == cur_pre:
            return f"{core}-{pre}.{cur_num + 1}"
    return f"{core}-{pre}.1"


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        raise SystemExit("usage: python scripts/release.py <command> [version]")
    command = argv[1]
    if command == "check-clean-git":
        check_clean_git()
    elif command == "check-release-version":
        check_release_version(argv[2])
    elif command == "check-version-sync":
        check_version_sync()
    elif command == "bump":
        bump(argv[2])
    elif command == "next-version":
        # next-version <patch|minor|major> [none|alpha|beta|rc]
        print(next_version(cargo_version(), argv[2], argv[3] if len(argv) > 3 else "none"))
    else:
        raise SystemExit(f"unknown command: {command}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
