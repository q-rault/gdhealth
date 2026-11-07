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
    else:
        raise SystemExit(f"unknown command: {command}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
