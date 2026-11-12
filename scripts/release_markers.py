#!/usr/bin/env python3
"""The machine-readable markers a release's notes carry for an installer that reads them.

    python scripts/release_markers.py append --tag vX.Y.Z NOTES.md
    python scripts/release_markers.py check  --tag vX.Y.Z NOTES.md
    python scripts/release_markers.py list   --tag vX.Y.Z

A release whose scanner rule set (`RULESET` in crates/oml-scan/src/lib.rs) differs from the
previous stable tag's carries `<!-- oml-deployer: post-step admin-rescan-record-held -->` in its
notes, since a changed rule set leaves held versions scanned under the old one until an admin runs
`oml admin rescan --record-held` (docs/operations/upgrades.md). v0.7.2 changed the rule set to
`oml-scan/1.2` and its notes had no such line. The release workflow appends the markers to the notes
it writes, then checks them; `check` names the line to add when one is missing. A release with no
previous stable tag carries no marker. Standard library only; it reads the tags with git, so the
checkout needs its full history.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCANNER_SOURCE = "crates/oml-scan/src/lib.rs"
ADMIN_RESCAN_MARKER = "<!-- oml-deployer: post-step admin-rescan-record-held -->"
RULESET = re.compile(r'^pub const RULESET: &str = "([^"]+)";', re.M)
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$")
STABLE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")


def ruleset_of(source: str) -> str | None:
    """The rule-set version a scanner source declares, or None when it declares none."""
    found = RULESET.search(source)
    return found.group(1) if found else None


def core(tag: str, pattern: re.Pattern = TAG) -> tuple[int, int, int] | None:
    found = pattern.match(tag)
    return tuple(int(n) for n in found.groups()) if found else None  # type: ignore[return-value]


def previous_tag(tag: str, tags: list[str]) -> str | None:
    """The greatest stable tag whose version is below `tag`'s, or None."""
    here = core(tag)
    if here is None:
        raise ValueError(f"{tag!r} is no release tag (vX.Y.Z)")
    below = [(core(t, STABLE), t) for t in tags]
    below = [(c, t) for c, t in below if c is not None and c < here]
    return max(below)[1] if below else None


def git(root: Path, *args: str) -> str:
    done = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    if done.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {done.stderr.strip()}")
    return done.stdout


def ruleset_at(root: Path, rev: str) -> str | None:
    """The scanner's rule set at `rev`; None when the file or the constant is not there.

    Raises RuntimeError, with git's message, when git itself fails (an unknown revision, no
    repository, an unreadable object): that is not the same as the rule set being undeclared."""
    git(root, "rev-parse", "--verify", f"{rev}^{{commit}}")
    if not git(root, "ls-tree", "--name-only", rev, "--", SCANNER_SOURCE).strip():
        return None
    return ruleset_of(git(root, "show", f"{rev}:{SCANNER_SOURCE}"))


def required(root: Path, tag: str) -> list[tuple[str, str]]:
    """Each marker the notes of `tag` must carry, with the sentence that says why.

    Raises RuntimeError when the tag's own scanner source declares no rule set, since no
    comparison can then be made."""
    now = ruleset_at(root, tag)
    if now is None:
        raise RuntimeError(f"{SCANNER_SOURCE} at {tag} declares no `pub const RULESET`")
    tags = [t for t in git(root, "tag", "--list", "v*").split() if core(t, STABLE) is not None]
    before = previous_tag(tag, tags)
    if before is None:
        return []
    then = ruleset_at(root, before)
    if then == now:
        return []
    return [(
        ADMIN_RESCAN_MARKER,
        f"the scanner's rule set is {now} at {tag} and {then or 'not declared'} at {before}",
    )]


def missing(notes: str, wanted: list[tuple[str, str]]) -> list[str]:
    lines = {line.strip() for line in notes.splitlines()}
    return [f"{marker}  ({why})" for marker, why in wanted if marker not in lines]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["append", "check", "list"])
    ap.add_argument("notes", nargs="?", type=Path, help="the release notes file (append, check)")
    ap.add_argument("--tag", required=True)
    ap.add_argument("--root", type=Path, default=ROOT)
    args = ap.parse_args(argv)
    if args.action != "list" and args.notes is None:
        ap.error(f"{args.action} needs the release notes file")
    try:
        wanted = required(args.root, args.tag)
    except (RuntimeError, ValueError) as e:
        print(f"release markers: {e}", file=sys.stderr)
        return 1
    if args.action == "list":
        for marker, _ in wanted:
            print(marker)
        return 0
    notes = args.notes.read_text(encoding="utf-8")
    if args.action == "append":
        add = [marker for marker, _ in wanted if marker not in {ln.strip() for ln in notes.splitlines()}]
        if add:
            args.notes.write_text(notes.rstrip("\n") + "\n\n" + "\n".join(add) + "\n", encoding="utf-8")
        print(f"release markers: {len(add)} appended, {len(wanted) - len(add)} already present")
        return 0
    lacking = missing(notes, wanted)
    for line in lacking:
        print(
            f"release markers: the release notes lack the line {line}; add it to the release notes source"
            " (the notes steps of release.yml's publish job; `python scripts/release_markers.py append` writes it)",
            file=sys.stderr,
        )
    if not lacking:
        print(f"release markers: {len(wanted)} required, all present")
    return 1 if lacking else 0


if __name__ == "__main__":
    sys.exit(main())
