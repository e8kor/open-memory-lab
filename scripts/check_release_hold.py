#!/usr/bin/env python3
"""Refuse a release while RELEASE-HOLD exists at the repository root.

    python scripts/check_release_hold.py [--root DIR]

The release workflow's version job runs this first, and every other release job needs that
job, so a tag pushed while main holds the file builds, signs and publishes nothing. The file's
first line says why the release is held; the change that ends the reason deletes the file.
Standard library only.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HOLD = "RELEASE-HOLD"


def held(root: Path) -> str | None:
    """The reason the release is held, the hold file's first non-empty line, or None."""
    path = root / HOLD
    if not path.exists():
        return None
    lines = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    return lines[0] if lines else "(the file gives no reason)"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", type=Path, default=ROOT)
    args = ap.parse_args(argv)
    reason = held(args.root)
    if reason is not None:
        print(f"release held: {HOLD} exists at the repository root: {reason}", file=sys.stderr)
        print(f"release held: no release is built while it exists; delete {HOLD} in the change that ends the hold", file=sys.stderr)
        return 1
    print("release hold: none")
    return 0


if __name__ == "__main__":
    sys.exit(main())
