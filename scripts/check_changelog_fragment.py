#!/usr/bin/env python3
"""Fail when a change under crates/ or docs/adr/ adds no changes/*.md fragment.

    python scripts/check_changelog_fragment.py [--base origin/main]

The label `no-changelog` on the pull request skips the check; CI passes the labels in
PR_LABELS (comma separated).
"""
from __future__ import annotations

import argparse
import os
import subprocess
import sys

NEEDS = ("crates/", "docs/adr/")


def changed(root: str, base: str) -> list[tuple[str, str]]:
    mb = subprocess.run(["git", "merge-base", base, "HEAD"], cwd=root, capture_output=True, text=True).stdout.strip()
    out = subprocess.run(["git", "diff", "--no-renames", "--name-status", mb, "HEAD"], cwd=root,
                         capture_output=True, text=True, check=True).stdout
    return [tuple(l.split("\t", 1)) for l in out.splitlines() if "\t" in l]


def verdict(files: list[tuple[str, str]], labels: list[str]) -> str | None:
    if "no-changelog" in labels:
        return None
    if not any(p.startswith(NEEDS) for _, p in files):
        return None
    if any(s == "A" and p.startswith("changes/") and p.endswith(".md") and p != "changes/README.md" for s, p in files):
        return None
    return "this change touches crates/ or docs/adr/ and adds no changes/<pr-or-branch>.md (label no-changelog skips)"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--root", default=".")
    a = ap.parse_args(argv)
    labels = [l.strip() for l in os.environ.get("PR_LABELS", "").split(",")]
    msg = verdict(changed(a.root, a.base), labels)
    print(f"changelog: {msg or 'ok'}")
    return 1 if msg else 0


if __name__ == "__main__":
    sys.exit(main())
