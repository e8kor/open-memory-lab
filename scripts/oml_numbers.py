"""Shared scan and registry code for docs/NUMBERS.md (reserve_number.py, check_numbers.py).

Three number spaces are kept: ADR files (docs/adr/ADR-nnnn-*.md), FR rows of
docs/product/roadmap.md, and migrations (migrations/nnnn_*.sql). Standard library only.
"""
from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

KINDS = ("adr", "fr", "migration")
ROADMAP = "docs/product/roadmap.md"
REGISTRY = "docs/NUMBERS.md"
ROW = re.compile(r"^\|\s*(adr|fr|migration)\s*\|\s*(\d+)\s*\|([^|]*)\|([^|]*)\|([^|]*)\|([^|]*)\|\s*$")
ADR_FILE = re.compile(r"^docs/adr/ADR-(\d+)-(.*)\.md$")
MIG_FILE = re.compile(r"^migrations/(\d+)_(.*)\.sql$")
FR_ROW = re.compile(r"^\| FR-(\d+) \|\s*([^|]*)")


def git(root: Path, *args: str, check: bool = True) -> str:
    r = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout if r.returncode == 0 else ""


def _from_path(path: str) -> tuple[str, int, str] | None:
    for kind, rx in (("adr", ADR_FILE), ("migration", MIG_FILE)):
        m = rx.match(path)
        if m:
            return kind, int(m.group(1)), m.group(2).replace("-", " ").replace("_", " ")[:70]
    return None


def numbers_on(root: Path, ref: str) -> dict[tuple[str, int], str]:
    """Every number a ref holds, with a short title."""
    out: dict[tuple[str, int], str] = {}
    for p in git(root, "ls-tree", "-r", "--name-only", ref, "docs/adr", "migrations").splitlines():
        hit = _from_path(p)
        if hit:
            out[(hit[0], hit[1])] = hit[2]
    for line in git(root, "show", f"{ref}:{ROADMAP}", check=False).splitlines():
        m = FR_ROW.match(line)
        if m:
            out[("fr", int(m.group(1)))] = plain(m.group(2).strip())[:70]
    return out


def numbers_added(root: Path, base: str, ref: str) -> dict[tuple[str, int], str]:
    """Numbers a ref introduces beyond its merge base with `base`."""
    mb = git(root, "merge-base", base, ref, check=False).strip()
    if not mb:
        return {}
    out: dict[tuple[str, int], str] = {}
    names = git(root, "diff", "--no-renames", "--diff-filter=A", "--name-only", mb, ref, "--", "docs/adr", "migrations")
    for p in names.splitlines():
        hit = _from_path(p)
        if hit:
            out[(hit[0], hit[1])] = hit[2]
    for line in git(root, "diff", "-U0", mb, ref, "--", ROADMAP).splitlines():
        if line.startswith("+|"):
            m = FR_ROW.match(line[1:])
            if m:
                out[("fr", int(m.group(1)))] = plain(m.group(2).strip())[:70]
    return out


def remote_branches(root: Path, remote: str = "origin", main: str = "main") -> list[str]:
    refs = git(root, "for-each-ref", "--format=%(refname:short)", f"refs/remotes/{remote}/").splitlines()
    skip = {f"{remote}/HEAD", f"{remote}/{main}", remote}
    return [r for r in refs if r not in skip]


def merged_into(root: Path, ref: str, base: str) -> bool:
    """True when every commit of `ref` is already in `base`."""
    return subprocess.run(["git", "merge-base", "--is-ancestor", ref, base], cwd=root, capture_output=True).returncode == 0


def open_prs(root: Path) -> dict[str, int]:
    try:
        r = subprocess.run(["gh", "pr", "list", "--state", "open", "--limit", "200", "--json", "number,headRefName"],
                           cwd=root, capture_output=True, text=True, timeout=60)
        return {p["headRefName"]: p["number"] for p in json.loads(r.stdout)} if r.returncode == 0 else {}
    except (OSError, subprocess.TimeoutExpired, ValueError):
        return {}


def parse_registry(text: str) -> list[dict]:
    rows = []
    for line in text.splitlines():
        m = ROW.match(line)
        if m:
            kind, num, branch, pr, title, state = m.groups()
            rows.append({"kind": kind, "number": int(num), "branch": branch.strip().strip("`"),
                         "pr": pr.strip(), "title": title.strip(), "state": state.strip()})
    return rows


def registry_at(root: Path, ref: str) -> list[dict]:
    return parse_registry(git(root, "show", f"{ref}:{REGISTRY}", check=False))


def plain(title: str) -> str:
    """A title as plain text: a link in it would be resolved by the docs build."""
    return re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", title).replace("|", "/")


def fmt(kind: str, n: int) -> str:
    return {"adr": f"ADR-{n:04d}", "fr": f"FR-{n}", "migration": f"migration {n:04d}"}[kind]


def render_rows(rows: list[dict]) -> str:
    order = {k: i for i, k in enumerate(KINDS)}
    lines = ["| kind | number | branch | PR | title | state |", "|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (order[r["kind"]], r["number"], r["branch"])):
        num = f"{r['number']:04d}" if r["kind"] != "fr" else str(r["number"])
        lines.append(f"| {r['kind']} | {num} | `{r['branch']}` | {r['pr']} | {plain(r['title'])} | {r['state']} |")
    return "\n".join(lines) + "\n"


def scan_rows(root: Path, remote: str = "origin", main: str = "main") -> list[dict]:
    """Rows for everything on main and on every unmerged remote branch."""
    prs = open_prs(root)
    on_main = numbers_on(root, f"{remote}/{main}")
    rows = [{"kind": k, "number": n, "branch": main, "pr": "", "title": t, "state": "used"}
            for (k, n), t in on_main.items()]
    for b in remote_branches(root, remote, main):
        name = b.split("/", 1)[1]
        if prs and name not in prs:
            continue  # only branches with an open pull request hold numbers
        pr = f"#{prs[name]}" if name in prs else ""
        for (k, n), t in numbers_added(root, f"{remote}/{main}", b).items():
            if on_main.get((k, n)) == t:
                continue  # already merged: the branch only needs a rebase
            rows.append({"kind": k, "number": n, "branch": name, "pr": pr, "title": t, "state": "open"})
        for r in registry_at(root, b):
            if r["state"] == "reserved" and r["branch"] == name:
                rows.append(r)
    return stacked(rows)


def stacked(rows: list[dict]) -> list[dict]:
    """A branch stacked on another repeats its base's numbers; keep one row per identical claim,
    preferring the branch with the lowest open PR."""
    best: dict[tuple, dict] = {}
    out = []
    for r in rows:
        if r["state"] != "open":
            out.append(r)
            continue
        key = (r["kind"], r["number"], r["title"])
        cur = best.get(key)
        rank = int(r["pr"][1:]) if r["pr"].startswith("#") else 10**9
        if cur is None or rank < cur[0]:
            best[key] = (rank, r)
    return out + [v[1] for v in best.values()]


def dedupe(rows: list[dict]) -> list[dict]:
    seen, out = set(), []
    for r in rows:
        key = (r["kind"], r["number"], r["branch"])
        if key not in seen:
            seen.add(key)
            out.append(r)
    return out


def next_free(rows: list[dict], kind: str) -> int:
    return max([r["number"] for r in rows if r["kind"] == kind], default=0) + 1
