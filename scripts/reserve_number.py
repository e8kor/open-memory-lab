#!/usr/bin/env python3
"""Reserve the next free ADR or migration number in docs/NUMBERS.md.

    python scripts/reserve_number.py adr|migration --branch <b> --title <t>
    python scripts/reserve_number.py --rebuild          # rescan main and the open branches

The next number is one above the highest used on main, reserved in the registry on any
unmerged remote branch (with or without a pull request), or introduced by a branch with an
open pull request. A reservation counts while its branch exists on the remote (or, until it is
pushed, in this clone) and is not merged into main (every commit already in main; a squash-merged branch holds an
unused number until it is deleted). Deleting the branch releases it. It fails if that number
turns out to be used on a remote branch, and when another branch's registry cannot be read.

Every remote head is fetched first, in one pass. When the fetch fails (no network, no such
remote) nothing is reserved and the exit code is 1, because a number that could not be
checked against the remote branches could collide; --no-fetch reserves against the refs
already here and says so. The registry is written under a file lock and replaced atomically;
commit and push the reservation before writing the file that uses the number.
"""
from __future__ import annotations

import argparse
import fcntl
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import oml_numbers as N  # noqa: E402

HEADER = """<!--
id: engineering/numbers
tier: engineering
status: process register
answers: which ADR and migration numbers are used or reserved, and by which branch
-->

# Number registry

Every ADR and migration number is used on main or reserved by one branch. Reserve a
number before writing the file or row that uses it:

```sh
python scripts/reserve_number.py adr --branch <branch> --title "<the decision, one sentence>"
git add docs/NUMBERS.md && git commit -m "chore: reserve ADR-nnnn" && git push
```

`scripts/check_numbers.py` fails a pull request that introduces a number the registry assigns
to another branch, or does not list. A reservation pushed on any unmerged remote branch holds its
number, whether or not the branch has a pull request yet; delete a dead branch to release its
numbers. `python scripts/reserve_number.py --rebuild` rescans main and the unmerged remote
branches and keeps the reservations. A number listed for two branches is a collision: the
branch that reserved later renumbers.

"""


def write_registry(path: Path, rows: list[dict]) -> None:
    body = HEADER + N.render_rows(rows)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".numbers-")
    with os.fdopen(fd, "w") as f:
        f.write(body)
    os.replace(tmp, path)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kind", nargs="?", choices=N.KINDS)
    ap.add_argument("--branch")
    ap.add_argument("--title")
    ap.add_argument("--rebuild", action="store_true")
    ap.add_argument("--root", default=".")
    ap.add_argument("--remote", default="origin")
    ap.add_argument("--no-fetch", action="store_true")
    a = ap.parse_args(argv)
    root = Path(a.root).resolve()
    reg = root / N.REGISTRY
    if not a.no_fetch:
        err = N.fetch_heads(root, a.remote)
        if err:
            print(f"cannot read the remote branches: {err}\nnothing was reserved: a number that could not be "
                  f"checked against every remote branch could collide. Reconnect and retry, or pass --no-fetch "
                  f"to use the refs already here (a branch pushed since the last fetch will not be seen).",
                  file=sys.stderr)
            return 1
    else:
        print("numbers: --no-fetch, using the remote refs already here", file=sys.stderr)
    lock = open(reg.with_suffix(".lock"), "w")
    fcntl.flock(lock, fcntl.LOCK_EX)
    try:
        local = N.parse_registry(reg.read_text()) if reg.exists() else []
        try:
            scanned = N.scan_rows(root, a.remote)
        except N.RegistryUnreadable as e:
            print(f"{e}\nnothing was reserved: its reservations are unknown, so a number could collide. "
                  f"Fix or delete {e.branch}.", file=sys.stderr)
            return 1
        main_numbers = N.numbers_on(root, f"{a.remote}/main")
        # a branch of this clone that is not pushed yet still holds the numbers it reserved
        keep = set(N.git(root, "for-each-ref", "--format=%(refname:short)", "refs/heads/").split()) | {a.branch or ""}
        local = N.drop_stale(local, N.live_branches(root, a.remote), keep, main_numbers)
        if a.rebuild:
            keep = [r for r in local if r["state"] == "reserved"
                    and not any(s["kind"] == r["kind"] and s["number"] == r["number"] for s in scanned)]
            write_registry(reg, N.dedupe(scanned + keep))
            print(f"rebuilt {N.REGISTRY}")
            return 0
        if not (a.kind and a.branch and a.title):
            ap.error("kind, --branch and --title are required")
        rows = N.dedupe(scanned + local)
        n = N.next_free(rows, a.kind)
        if any(s["kind"] == a.kind and s["number"] == n and s["state"] != "reserved" for s in scanned):
            print(f"{N.fmt(a.kind, n)} is used on a remote branch; rescan and retry", file=sys.stderr)
            return 1
        rows.append({"kind": a.kind, "number": n, "branch": a.branch, "pr": "", "title": a.title, "state": "reserved"})
        write_registry(reg, rows)
    finally:
        fcntl.flock(lock, fcntl.LOCK_UN)
        lock.close()
        try:
            reg.with_suffix(".lock").unlink()
        except OSError:
            pass
    print(f"reserved {N.fmt(a.kind, n)} for {a.branch}")
    print(f"Commit {N.REGISTRY} and push it before writing the file that uses the number.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
