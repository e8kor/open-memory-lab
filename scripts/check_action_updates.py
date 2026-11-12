#!/usr/bin/env python3
"""Report the pinned GitHub Actions that have a newer release tag, or that are not the commit they name.

    python scripts/check_action_updates.py [--root DIR]

Reads every pinned `uses` under .github/ as scripts/lint_practices.py actions reads it, so a use in
any YAML form is checked, and asks GitHub about each action's repository with `gh api`. For each pin
it checks that the commit is the one the tag in its comment points at, and prints a line when it is
not, or when that tag does not exist. It prints one line per action whose pin trails the newest
`vN.M.P` tag of the same major, and one `newer major` line where a later major exists. A comment may
name a major or a minor alone (`# v4`): the pin is then checked against that tag and against the
newest release of the major. It exits 1 when a pin is not its tag's commit or trails its own major;
a newer major alone is information, because a major is a reviewed change
(docs/contributing/ci-and-toolchain.md "Pinned actions"). The weekly dependency pass runs it and
prints its output. Needs PyYAML, as the lint does, and `gh` signed in.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import lint_practices as L  # noqa: E402

COMMENT_TAG = re.compile(r"^#\s*(v\d+(?:\.\d+){0,2})(?:\s|$)")
TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
PARTIAL = re.compile(r"^v(\d+)(?:\.(\d+))?(?:\.(\d+))?$")


def pinned(root: Path) -> dict[str, tuple[str, str]]:
    """repository -> (the commit it is pinned to, the tag its comment names), over every workflow; the
    lint holds one commit per repository."""
    out: dict[str, tuple[str, str]] = {}
    for _, target, comment in L.read_uses(root)[0]:
        pin, tag = L.PIN.match(target), COMMENT_TAG.match(comment)
        if pin and tag:
            out.setdefault(L.repository(pin["action"]), (pin["sha"], tag.group(1)))
    return out


def version(tag: str) -> tuple[int, int, int]:
    """A tag's version; a part a comment leaves out (`v4`, `v4.3`) counts as zero."""
    m = PARTIAL.match(tag)
    return (int(m.group(1)), int(m.group(2) or 0), int(m.group(3) or 0))


def newer(tag: str, tags: list[str]) -> tuple[str | None, str | None]:
    """(newest tag of tag's major if later than tag, newest tag of a later major), from release tags only."""
    here = version(tag)
    releases = sorted((version(t), t) for t in tags if TAG.match(t))
    same = [t for v, t in releases if v[0] == here[0] and v > here]
    later = [t for v, t in releases if v[0] > here[0]]
    return (same[-1] if same else None, later[-1] if later else None)


def gh_json(path: str) -> list:
    raw = subprocess.run(["gh", "api", "--paginate", path], check=True, capture_output=True, text=True).stdout
    # --paginate prints one JSON value per page
    decoder, i, pages = json.JSONDecoder(), 0, []
    while i < len(raw):
        if raw[i].isspace():
            i += 1
            continue
        page, i = decoder.raw_decode(raw, i)
        pages.append(page)
    return pages


def refs_of(repo: str) -> dict[str, dict]:
    """tag -> the object its ref points at (`sha`, and `type` commit or tag), for every `v` tag of repo."""
    return {
        r["ref"].removeprefix("refs/tags/"): r["object"]
        for page in gh_json(f"repos/{repo}/git/matching-refs/tags/v")
        for r in page
    }


def commit_of(repo: str, obj: dict | None) -> str | None:
    """The commit a tag's object is, following an annotated tag to what it tags."""
    for _ in range(5):
        if obj is None or obj["type"] == "commit":
            return obj and obj["sha"]
        obj = gh_json(f"repos/{repo}/git/tags/{obj['sha']}")[0]["object"]
    return None


def report(
    pins: dict[str, tuple[str, str]], tags: dict[str, list[str]], commit: Callable[[str, str], str | None]
) -> tuple[list[str], bool]:
    """`commit(repo, tag)` is the commit that tag points at, or None when the repository has no such tag."""
    lines, behind = [], False
    for repo, (sha, tag) in sorted(pins.items()):
        at = commit(repo, tag)
        if at is None:
            behind = True
            lines.append(f"{repo}: pinned {sha[:12]} names {tag}, which is no tag of {repo}")
        elif at != sha:
            behind = True
            lines.append(f"{repo}: pinned {sha[:12]} is not the commit of {tag} ({at[:12]})")
        same, later = newer(tag, tags[repo])
        # A comment that names a major alone may sit on the newest release already.
        if same and commit(repo, same) != sha:
            behind = True
            lines.append(f"{repo}: pinned {tag}, newer {same}")
        if later:
            lines.append(f"{repo}: pinned {tag}, newer major {later} (a reviewed change)")
    return lines, behind


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=str(HERE.parent))
    pins = pinned(Path(ap.parse_args(argv).root).resolve())
    refs = {repo: refs_of(repo) for repo in pins}
    known: dict[tuple[str, str], str | None] = {}

    def commit(repo: str, tag: str) -> str | None:
        if (repo, tag) not in known:
            known[(repo, tag)] = commit_of(repo, refs[repo].get(tag))
        return known[(repo, tag)]

    lines, behind = report(pins, {repo: list(found) for repo, found in refs.items()}, commit)
    print(*lines, f"{len(pins)} pinned actions checked", sep="\n")
    return 1 if behind else 0


if __name__ == "__main__":
    sys.exit(main())
