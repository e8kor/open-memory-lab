#!/usr/bin/env python3
"""Report the pinned GitHub Actions that have a newer release tag.

    python scripts/check_action_updates.py [--root DIR]

Reads every `uses: owner/repo@<sha> # v<tag>` under .github/ (the shape scripts/lint_practices.py
actions enforces) and asks GitHub for the repository's `vN.M.P` tags with `gh api`. It prints one
line per action whose pin trails the newest tag of the same major, and one `newer major` line where
a later major exists. It exits 1 when an action trails its own major; a newer major alone is
information, because a major is a reviewed change (docs/contributing/ci-and-toolchain.md "Pinned
actions"). The weekly dependency pass runs it. Standard library only; needs `gh` signed in.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PIN = re.compile(r"^\s*(?:-\s+)?uses:\s*([\w.-]+/[\w./-]+)@[0-9a-f]{40}\s*#\s*(v\d+\.\d+\.\d+)\s*$")
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")


def pinned(root: Path) -> dict[str, str]:
    """action -> the tag its pin names, over every workflow; the lint holds one pin per action."""
    out: dict[str, str] = {}
    for f in sorted((root / ".github").rglob("*.y*ml")):
        for line in f.read_text().splitlines():
            m = PIN.match(line)
            if m:
                out.setdefault(m.group(1), m.group(2))
    return out


def version(tag: str) -> tuple[int, int, int]:
    m = TAG.match(tag)
    return (int(m.group(1)), int(m.group(2)), int(m.group(3)))


def newer(tag: str, tags: list[str]) -> tuple[str | None, str | None]:
    """(newest tag of tag's major if later than tag, newest tag of a later major), from release tags only."""
    here = version(tag)
    releases = sorted((version(t), t) for t in tags if TAG.match(t))
    same = [t for v, t in releases if v[0] == here[0] and v > here]
    later = [t for v, t in releases if v[0] > here[0]]
    return (same[-1] if same else None, later[-1] if later else None)


def tags_of(repo: str) -> list[str]:
    raw = subprocess.run(
        ["gh", "api", "--paginate", f"repos/{repo}/git/matching-refs/tags/v"],
        check=True, capture_output=True, text=True,
    ).stdout
    # --paginate prints one JSON array per page
    decoder, i, refs = json.JSONDecoder(), 0, []
    while i < len(raw):
        if raw[i].isspace():
            i += 1
            continue
        page, i = decoder.raw_decode(raw, i)
        refs += page
    return [r["ref"].removeprefix("refs/tags/") for r in refs]


def report(pins: dict[str, str], tags: dict[str, list[str]]) -> tuple[list[str], bool]:
    lines, behind = [], False
    for action, tag in sorted(pins.items()):
        same, later = newer(tag, tags[action])
        if same:
            behind = True
            lines.append(f"{action}: pinned {tag}, newer {same}")
        if later:
            lines.append(f"{action}: pinned {tag}, newer major {later} (a reviewed change)")
    return lines, behind


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=str(HERE.parent))
    pins = pinned(Path(ap.parse_args(argv).root).resolve())
    lines, behind = report(pins, {a: tags_of(a) for a in pins})
    print(*lines, f"{len(pins)} pinned actions checked", sep="\n")
    return 1 if behind else 0


if __name__ == "__main__":
    sys.exit(main())
