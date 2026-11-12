#!/usr/bin/env python3
"""Fail when this branch introduces an ADR or migration number that docs/NUMBERS.md
assigns to another branch, does not list, or that another remote branch also introduces.

    python scripts/check_numbers.py [--base origin/main] [--branch NAME] [--no-fetch]

It also fails when another unmerged remote branch, with or without a pull request, has reserved
a number this branch uses or reserves. Every remote head is fetched first; when that fails the
check fails with the reason rather than pass against refs it could not refresh (--no-fetch uses
the refs already here).
"""
from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import oml_numbers as N  # noqa: E402


def check(root: Path, base: str, branch: str, remote: str = "origin", prs: dict[str, int] | None = None) -> list[str]:
    """`prs` maps the head of each open pull request to its number; None asks GitHub. When it is
    known, only open pull requests hold numbers (as scan_rows reads it), and a branch fully merged
    into `base` holds none."""
    prs = N.open_prs(root) if prs is None else prs
    reg = root / N.REGISTRY
    rows = N.parse_registry(reg.read_text()) if reg.exists() else []
    mine = N.numbers_added(root, base, "HEAD")
    others = {}
    for b in N.remote_branches(root, remote, base.split("/")[-1]):
        name = b.split("/", 1)[1]
        if name != branch and not (prs and name not in prs) and not N.merged_into(root, b, base):
            for key, title in N.numbers_added(root, base, b).items():
                if mine.get(key) != title:  # a branch stacked on this one repeats its numbers
                    others.setdefault(key, []).append(name)
    problems = []
    on_main = N.numbers_on(root, base)
    held = {(r["kind"], r["number"]) for r in rows if r["branch"] == branch and r["state"] == "reserved"}
    reserved_elsewhere: dict[tuple[str, int], list[str]] = {}
    for name, b in N.live_branches(root, remote, base.split("/")[-1]).items():
        if name != branch:
            for r in N.reservations_on(root, b, name):
                if (r["kind"], r["number"]) not in on_main:
                    reserved_elsewhere.setdefault((r["kind"], r["number"]), []).append(name)
    for key in sorted((mine.keys() | held) & reserved_elsewhere.keys()):
        problems.append(f"{N.fmt(*key)} is also reserved on {', '.join(reserved_elsewhere[key])}: "
                        f"one of the two renumbers")
    for (kind, n) in sorted(mine):
        owners = [r["branch"] for r in rows if r["kind"] == kind and r["number"] == n]
        label = N.fmt(kind, n)
        if not owners:
            problems.append(f"{label} is not reserved: run scripts/reserve_number.py {kind} and commit docs/NUMBERS.md")
        elif branch not in owners:
            problems.append(f"{label} is assigned to {', '.join(owners)}, not to {branch}")
        elif len(set(owners)) > 1:
            problems.append(f"{label} is listed for {', '.join(sorted(set(owners)))}: the later reservation renumbers")
        if (kind, n) in others:
            problems.append(f"{label} is also introduced on {', '.join(others[(kind, n)])}")
    return problems


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--branch")
    ap.add_argument("--root", default=".")
    ap.add_argument("--remote", default="origin")
    ap.add_argument("--no-fetch", action="store_true")
    a = ap.parse_args(argv)
    root = Path(a.root).resolve()
    if not a.no_fetch:
        err = N.fetch_heads(root, a.remote)
        if err:
            print(f"numbers: FAIL: cannot read the remote branches: {err} "
                  f"(--no-fetch checks against the refs already here)")
            return 1
    branch = a.branch or os.environ.get("GITHUB_HEAD_REF") or N.git(root, "rev-parse", "--abbrev-ref", "HEAD").strip()
    problems = check(root, a.base, branch, a.remote)
    for p in problems:
        print(f"numbers: {p}")
    print(f"numbers: {'FAIL' if problems else 'ok'} ({branch})")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
