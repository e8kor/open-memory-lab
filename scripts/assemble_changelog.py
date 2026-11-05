#!/usr/bin/env python3
"""Build a release's section of docs/changelog.md from changes/*.md and delete the fragments.

    python scripts/assemble_changelog.py 0.5.9-design [--dry-run]

The section goes above the newest existing release section. Fragments are joined in file-name
order, so a release PR never conflicts on the changelog: every change added its own file.
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def assemble(changelog: Path, changes: Path, version: str, delete: bool = True) -> str:
    frags = sorted(p for p in changes.glob("*.md") if p.name.lower() != "readme.md")
    if not frags:
        raise SystemExit(f"no fragments in {changes}")
    text = changelog.read_text()
    if re.search(rf"^## {re.escape(version)}\s*$", text, re.M):
        raise SystemExit(f"{changelog} already has a section {version}")
    m = re.search(r"^## \d", text, re.M)
    if not m:
        raise SystemExit(f"{changelog} has no release section to insert above")
    body = "\n\n".join(p.read_text().strip() for p in frags)
    out = text[: m.start()] + f"## {version}\n\n{body}\n\n" + text[m.start():]
    changelog.write_text(out)
    if delete:
        for p in frags:
            p.unlink()
    return body


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("version")
    ap.add_argument("--changelog", default="docs/changelog.md")
    ap.add_argument("--changes", default="changes")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args(argv)
    cl, ch = Path(a.changelog), Path(a.changes)
    if a.dry_run:
        frags = sorted(p for p in ch.glob("*.md") if p.name.lower() != "readme.md")
        print("\n\n".join(p.read_text().strip() for p in frags))
        return 0
    assemble(cl, ch, a.version)
    print(f"{a.version}: assembled into {cl}; fragments deleted")
    return 0


if __name__ == "__main__":
    sys.exit(main())
