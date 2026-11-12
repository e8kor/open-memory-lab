"""Shared scan and registry code for docs/NUMBERS.md (reserve_number.py, check_numbers.py).

Two number spaces are kept: ADR files (docs/adr/ADR-nnnn-*.md) and migrations
(migrations/nnnn_*.sql). Open work is tracked as GitHub issues, which GitHub numbers.
Standard library only.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
from pathlib import Path

KINDS = ("adr", "migration")
REGISTRY = "docs/NUMBERS.md"
ROW = re.compile(r"^\|\s*(adr|migration)\s*\|\s*(\d+)\s*\|([^|]*)\|([^|]*)\|([^|]*)\|([^|]*)\|\s*$")
ADR_FILE = re.compile(r"^docs/adr/ADR-(\d+)-(.*)\.md$")
MIG_FILE = re.compile(r"^migrations/(\d+)_(.*)\.sql$")


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
    return out


def remote_branches(root: Path, remote: str = "origin", main: str = "main") -> list[str]:
    refs = git(root, "for-each-ref", "--format=%(refname:short)", f"refs/remotes/{remote}/").splitlines()
    skip = {f"{remote}/HEAD", f"{remote}/{main}", remote}
    return [r for r in refs if r not in skip]


def fetch_heads(root: Path, remote: str = "origin", timeout: int = 120) -> str | None:
    """Refresh every head of `remote` in one pass, pruning the ones deleted there. Returns None
    on success and the reason on failure (no network, no such remote, a timeout), so a caller can
    refuse rather than work from refs it could not refresh."""
    try:
        r = subprocess.run(["git", "fetch", "--prune", "--no-tags", remote, f"+refs/heads/*:refs/remotes/{remote}/*"],
                           cwd=root, capture_output=True, text=True, timeout=timeout,
                           env={**os.environ, "GIT_TERMINAL_PROMPT": "0"})
    except subprocess.TimeoutExpired:
        return f"git fetch {remote} timed out after {timeout}s"
    except OSError as e:
        return f"git fetch {remote}: {e}"
    if r.returncode != 0:
        return f"git fetch {remote} failed: {(r.stderr.strip().splitlines() or ['no output'])[-1]}"
    return None


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
    return {"adr": f"ADR-{n:04d}", "migration": f"migration {n:04d}"}[kind]


def render_rows(rows: list[dict]) -> str:
    order = {k: i for i, k in enumerate(KINDS)}
    lines = ["| kind | number | branch | PR | title | state |", "|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (order[r["kind"]], r["number"], r["branch"])):
        num = f"{r['number']:04d}"
        lines.append(f"| {r['kind']} | {num} | `{r['branch']}` | {r['pr']} | {plain(r['title'])} | {r['state']} |")
    return "\n".join(lines) + "\n"


def reservations_on(root: Path, ref: str, name: str) -> list[dict]:
    """The reservations a branch's own registry holds for itself."""
    return [r for r in registry_at(root, ref) if r["state"] == "reserved" and r["branch"] == name]


def live_branches(root: Path, remote: str = "origin", main: str = "main") -> dict[str, str]:
    """Branch name -> remote ref for every remote branch not yet merged into main. A reservation
    counts only while its branch is in this map: deleting the branch releases the number."""
    return {b.split("/", 1)[1]: b for b in remote_branches(root, remote, main)
            if not merged_into(root, b, f"{remote}/{main}")}


def scan_rows(root: Path, remote: str = "origin", main: str = "main",
              prs: dict[str, int] | None = None) -> list[dict]:
    """Rows for everything on main and on every unmerged remote branch.

    A reservation in a branch's NUMBERS.md holds its number on any unmerged remote branch, with or
    without a pull request (the flow is reserve, push, then open the pull request). A number the
    branch's files introduce holds it only while the branch has an open pull request, as before. A
    reservation whose number is already used on main holds nothing."""
    prs = open_prs(root) if prs is None else prs
    on_main = numbers_on(root, f"{remote}/{main}")
    rows = [{"kind": k, "number": n, "branch": main, "pr": "", "title": t, "state": "used"}
            for (k, n), t in on_main.items()]
    for name, b in live_branches(root, remote, main).items():
        pr = f"#{prs[name]}" if name in prs else ""
        if not prs or name in prs:  # no PR list at all (gh unavailable) counts every branch
            for (k, n), t in numbers_added(root, f"{remote}/{main}", b).items():
                if on_main.get((k, n)) == t:
                    continue  # already merged: the branch only needs a rebase
                rows.append({"kind": k, "number": n, "branch": name, "pr": pr, "title": t, "state": "open"})
        rows += [r for r in reservations_on(root, b, name) if (r["kind"], r["number"]) not in on_main]
    return stacked(rows)


def drop_stale(local: list[dict], live: dict[str, str], keep: set[str], on_main: dict) -> list[dict]:
    """Reserved rows in the checked-out registry whose branch is gone, merged, or whose number main
    now uses; `keep` names the branches of this clone, pushed or not."""
    return [r for r in local if r["state"] != "reserved"
            or (r["branch"] in live or r["branch"] in keep) and (r["kind"], r["number"]) not in on_main]


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
