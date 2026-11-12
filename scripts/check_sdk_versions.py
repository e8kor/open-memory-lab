#!/usr/bin/env python3
"""The SDKs' versions are the server's version (docs/contributing/releasing.md, "Publishing the SDKs").

    python scripts/check_sdk_versions.py [--root DIR] [--tag X.Y.Z]

Reads the workspace version from Cargo.toml and every place the Python and TypeScript SDKs carry
their own, and fails naming each place that disagrees: pyproject.toml, the oml-sdk entry of
uv.lock, package.json, both entries of package-lock.json, the User-Agent the TypeScript wrapper
sends, and the Rust SDK's `version.workspace = true`. With --tag, the tag must be that version too.
Run by release.yml's version job, so a tag whose SDKs were not bumped publishes nothing, and by
scripts/tests/test_sdk_versions_equal_the_server_version.py on every pull request.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parents[1]


def read_versions(root: Path) -> dict[str, str]:
    """Each place that carries a version, by the file and field it is read from."""
    found: dict[str, str] = {}
    cargo = tomllib.loads((root / "Cargo.toml").read_text())
    found["Cargo.toml [workspace.package] version"] = cargo["workspace"]["package"]["version"]

    py = root / "sdks/python"
    found["sdks/python/pyproject.toml [project] version"] = tomllib.loads(
        (py / "pyproject.toml").read_text()
    )["project"]["version"]
    lock = tomllib.loads((py / "uv.lock").read_text())
    entry = [p for p in lock["package"] if p["name"] == "oml-sdk"]
    found["sdks/python/uv.lock oml-sdk version"] = entry[0]["version"] if entry else "(no oml-sdk entry)"

    ts = root / "sdks/typescript"
    found["sdks/typescript/package.json version"] = json.loads((ts / "package.json").read_text())["version"]
    plock = json.loads((ts / "package-lock.json").read_text())
    found["sdks/typescript/package-lock.json version"] = plock["version"]
    found['sdks/typescript/package-lock.json packages[""] version'] = plock["packages"][""]["version"]
    agent = re.search(r'USER_AGENT = "oml-sdk-typescript/([^"]*)"', (ts / "src/client.ts").read_text())
    found["sdks/typescript/src/client.ts USER_AGENT"] = agent.group(1) if agent else "(no USER_AGENT)"

    rust = tomllib.loads((root / "sdks/rust/Cargo.toml").read_text())["package"]["version"]
    found["sdks/rust/Cargo.toml version"] = (
        "workspace" if rust == {"workspace": True} else f"{rust!r} (not version.workspace = true)"
    )
    return found


def disagreements(root: Path, tag: str | None = None) -> list[str]:
    found = read_versions(root)
    want = found["Cargo.toml [workspace.package] version"]
    problems = []
    for place, got in found.items():
        if place.startswith("sdks/rust"):
            if got != "workspace":
                problems.append(f"{place}: {got}")
        elif got != want:
            problems.append(f"{place} is {got}, the workspace is {want}")
    if tag is not None and tag != want:
        problems.append(f"tag {tag} is not the workspace version {want}")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--tag", help="the release tag without its v")
    args = parser.parse_args(argv)
    problems = disagreements(args.root, args.tag)
    for problem in problems:
        print(f"sdk version: {problem}", file=sys.stderr)
    if problems:
        print(
            "bump every place in one change (docs/contributing/releasing.md, 'Publishing the SDKs')",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
