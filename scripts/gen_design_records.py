#!/usr/bin/env python3
"""Write docs/reference/design-records.md, the register of design records.

The records themselves stay in docs/adr/ and are not published on the site. This page lists each
one with its status and the decision in a line, grouped by subject, and links to the file on GitHub.
It fails when a record has no category, so a new record cannot be missing from the register.

    python scripts/gen_design_records.py [--check]
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import docgen  # noqa: E402

ADR_DIR = docgen.DOCS / "adr"
OUT = docgen.DOCS / "reference" / "design-records.md"
BLOB = "https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/"

CATEGORIES = {
    "Trust and governance": [1, 7, 9, 10, 12, 30, 55, 65],
    "Storage and data": [2, 3, 4, 5, 17, 60],
    "Search and recall": [13, 48, 49, 52, 53, 54, 59, 72],
    "Writing and review": [11, 20, 21, 50, 56],
    "Sources and sync": [47, 57, 61, 62, 63],
    "Integrations and agents": [14, 22, 25, 28, 29, 31, 37, 38, 39, 40, 67, 76, 77],
    "Operations and security": [15, 16, 23, 26, 33, 34, 41, 42, 43, 44, 45, 46, 58, 71, 79, 84],
    "Architecture and platform": [6, 8, 18, 19, 24, 66, 70],
}
BLURB = {
    "Trust and governance": "The rules that make a write held, reviewed by someone else and signed.",
    "Storage and data": "Where memory lives, how it is encrypted, drained to the lake and erased.",
    "Search and recall": "How recall finds, ranks and filters memories.",
    "Writing and review": "What a write may carry and how review judges it.",
    "Sources and sync": "Where a memory came from and how a workarea follows a repository or a chat.",
    "Integrations and agents": "The API, the MCP tools, the plugin and the hooks agents use.",
    "Operations and security": "Deployment, identity, observability and the web pages.",
    "Architecture and platform": "The stack, the workarea tree and how decisions are recorded.",
}


# Records the page leaves out although they are accepted: ADR-0061 ships in a release after v0.6.0,
# ADR-0064, ADR-0070 and ADR-0076 are accepted designs that are decided and not built, and ADR-0066 is about how this repository tracks work, which readers of the site do not need.
HELD_BACK = {61, 64, 66, 70, 76}


def record(path: pathlib.Path) -> tuple[int, str, str, str]:
    text = path.read_text()
    num = int(re.search(r"ADR-(\d+)", path.name).group(1))
    title = re.search(r"^# ADR-\d+\s*[—-]\s*(.+)$", text, re.M).group(1).strip()
    st = re.search(r"^\*\*Status:\*\*\s*(.+)$", text, re.M)
    status = st.group(1).split("·")[0].strip() if st else "unknown"
    return num, title, status, path.name


def render() -> tuple[str, list[str]]:
    every = {r[0]: r for r in (record(p) for p in sorted(ADR_DIR.glob("ADR-*.md")))}
    recs = {n: r for n, r in every.items() if r[2].startswith("accepted") and n not in HELD_BACK}
    placed = {n for ns in CATEGORIES.values() for n in ns}
    problems = [f"ADR-{n:04d} has no category in scripts/gen_design_records.py" for n in recs if n not in placed]
    problems += [f"category list names ADR-{n:04d}, which does not exist" for n in placed if n not in every]
    out = [
        docgen.front_matter("every accepted design record of the current release and the decision it records"),
        "# Design records\n",
        f"Each decision about how OML works is a record in the repository, with the alternatives it rejected. "
        f"The records are not part of this site. This page lists the {len(recs)} accepted records that describe the current release, by subject, and each row links to the record on GitHub.\n",
        f"*{docgen.BANNER.format(script='scripts/gen_design_records.py')}*\n",
    ]
    for cat, nums in CATEGORIES.items():
        out.append(f"## {cat}\n")
        out.append(BLURB[cat] + "\n")
        out.append("| Decision |\n|---|")
        for n in sorted(x for x in nums if x in recs):
            _, title, status, fname = recs[n]
            out.append(f"| [{docgen.strip_handles(title).replace('|', '\\|')}]({BLOB}{fname}) |")
        out.append("")
    return "\n".join(out), problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="fail if the register differs from the records")
    args = ap.parse_args()
    text, problems = render()
    for p in problems:
        print(p, file=sys.stderr)
    if problems:
        return 1
    return docgen.finish(OUT, text, args.check)


if __name__ == "__main__":
    sys.exit(main())
