#!/usr/bin/env python3
"""Stop the publish jobs while the registry names are undecided (sdks/REGISTRY-NAMES.toml).

    python scripts/check_sdk_registry_names.py [--root DIR]

Exits 0 only when the file says `decided = true` and its `python` and `npm` keys are exactly the
distribution names in the two manifests. release.yml's publish-sdk-pypi and publish-sdk-npm run
it as their first step, so setting the repository variable PUBLISH_SDKS publishes nothing under a
name the operator has not chosen.
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

import sdk_names

MARKER = "sdks/REGISTRY-NAMES.toml"


def refusals(root: Path) -> list[str]:
    path = root / MARKER
    if not path.is_file():
        return [f"{MARKER} does not exist, so no registry name is decided"]
    marker = tomllib.loads(path.read_text())
    if marker.get("decided") is not True:
        return [f"{MARKER}: decided is not true; the names on PyPI and npm are the operator's to choose"]
    problems = []
    for key, manifest in (("python", sdk_names.python_name(root)), ("npm", sdk_names.npm_name(root))):
        if marker.get(key) != manifest:
            problems.append(
                f"{MARKER}: {key} is {marker.get(key)!r}, the manifest names the package {manifest!r}"
            )
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", type=Path, default=sdk_names.DEFAULT_ROOT)
    args = parser.parse_args(argv)
    problems = refusals(args.root)
    for problem in problems:
        print(f"sdk registry names: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
