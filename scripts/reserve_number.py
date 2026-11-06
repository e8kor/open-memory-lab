#!/usr/bin/env python3
"""Reserve the next free ADR or migration number in docs/NUMBERS.md.

    python scripts/reserve_number.py adr|migration --branch <b> --title <t>
    python scripts/reserve_number.py --rebuild          # rescan main and the open branches

The next number is one above the highest used on main, on any remote branch, or reserved in
the registry here or on any remote branch. It fails if that number turns out to be used on a
remote branch. The registry is written under a file lock and replaced atomically; commit and
push the reservation before writing the file that uses the number.
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
to another branch, or does not list. `python scripts/reserve_number.py --rebuild` rescans main
and the unmerged remote branches and keeps the reservations. A number listed for two branches
is a collision: the branch that reserved later renumbers.

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
        N.git(root, "fetch", "--prune", a.remote, check=False)
    lock = open(reg.with_suffix(".lock"), "w")
    fcntl.flock(lock, fcntl.LOCK_EX)
    try:
        local = N.parse_registry(reg.read_text()) if reg.exists() else []
        scanned = N.scan_rows(root, a.remote)
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
