#!/usr/bin/env python3
"""The names the SDK packages are built, signed and published under, read from their manifests.

    python scripts/sdk_names.py --version X.Y.Z    # prints sdk_wheel=, sdk_sdist=, sdk_tgz= lines

The distribution name lives in one place per package: `[project] name` in sdks/python/pyproject.toml
and `name` in sdks/typescript/package.json. Every file name release.yml, scripts/build_sdk_packages.py
and scripts/check_release.py use comes from here, so choosing the registry name changes the
manifests and nothing else (plus the one `importlib.metadata` call in the Python client, which a
test holds to the manifest). release.yml's version job writes the lines to its outputs.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parents[1]


def python_name(root: Path = DEFAULT_ROOT) -> str:
    return tomllib.loads((root / "sdks/python/pyproject.toml").read_text())["project"]["name"]


def npm_name(root: Path = DEFAULT_ROOT) -> str:
    return json.loads((root / "sdks/typescript/package.json").read_text())["name"]


def python_stem(name: str) -> str:
    """The name as a wheel or sdist file spells it: PEP 503 normalised, `-` as `_`."""
    return re.sub(r"[-_.]+", "_", name).lower()


def wheel_filename(root: Path, version: str) -> str:
    return f"{python_stem(python_name(root))}-{version}-py3-none-any.whl"


def sdist_filename(root: Path, version: str) -> str:
    return f"{python_stem(python_name(root))}-{version}.tar.gz"


def tgz_filename(root: Path, version: str) -> str:
    """What `npm pack` names the tarball: a scope's `@` dropped and its `/` made `-`."""
    return f"{npm_name(root).lstrip('@').replace('/', '-')}-{version}.tgz"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--version", required=True)
    args = parser.parse_args(argv)
    print(f"sdk_wheel={wheel_filename(args.root, args.version)}")
    print(f"sdk_sdist={sdist_filename(args.root, args.version)}")
    print(f"sdk_tgz={tgz_filename(args.root, args.version)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
