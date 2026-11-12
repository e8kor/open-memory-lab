#!/usr/bin/env python3
"""Four repository lints (docs/contributing/review-checklist.md).

    python scripts/lint_practices.py migrations|ports|like|actions [--root DIR] [--no-allowlist]

migrations  every migration not in lint_allow/migrations.txt sets lock_timeout, adds CHECK and
            FOREIGN KEY constraints NOT VALID (an inline CHECK on an added column too) and validates them (in that or a later
            migration, and only in a migration marked `-- no-transaction`), and on a partitioned table creates indexes ON ONLY the parent and
            ATTACHes the per-partition indexes.
ports       no localhost:5432, 127.0.0.1:5432 or PGPORT:-5432 default in tests or scripts.
like        no LIKE or ILIKE operator in the SQL of crates/adapters/oml-pg/src; row security
            runs a non-leakproof operator before its policy (ADR-0052).
actions     every `uses:` under .github/ is `owner/repo@<40 hex> # v<tag>`, and one action is one
            commit in every workflow (docs/contributing/ci-and-toolchain.md "Pinned actions").

An escape is a reasoned line comment: `-- lint:allow <rule> <why>` in SQL,
`// lint:allow-like <why>` in Rust, `lint:allow-5432 <why>` anywhere on the line.
Standard library only.
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def allowed(name: str, root: Path, use: bool) -> set[str]:
    f = root / "scripts" / "lint_allow" / name
    if not use or not f.exists():
        return set()
    return {l.split("#")[0].strip() for l in f.read_text().splitlines() if l.split("#")[0].strip()}


def strip_sql(text: str) -> str:
    return re.sub(r"--[^\n]*", "", text)


# ---- migrations -------------------------------------------------------------------------

def partitioned_tables(sqls: list[str]) -> set[str]:
    out = set()
    for s in sqls:
        for m in re.finditer(r"CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)\s*\((?:[^;]*?)\)\s*PARTITION\s+BY", s, re.I | re.S):
            out.add(m.group(1).lower())
    return out


def lint_migrations(root: Path, use_allow: bool = True) -> list[str]:
    mdir = root / "migrations"
    files = sorted(mdir.glob("*.sql"))
    texts = {f.name: f.read_text() for f in files}
    bare = {n: strip_sql(t) for n, t in texts.items()}
    part = partitioned_tables(list(bare.values()))
    validated = {m.group(1).lower() for t in bare.values()
                 for m in re.finditer(r"VALIDATE\s+CONSTRAINT\s+(\w+)", t, re.I)}
    skip = allowed("migrations.txt", root, use_allow)
    problems = []
    for f in files:
        if f.name in skip:
            continue
        raw, sql = texts[f.name], bare[f.name]

        def esc(rule: str, raw: str = raw) -> bool:
            return re.search(rf"--\s*lint:allow\s+{rule}\b\s+\S", raw) is not None

        if (re.search(r"VALIDATE\s+CONSTRAINT", sql, re.I) and not esc("validate")
                and not raw.lstrip().startswith("-- no-transaction")):
            problems.append(f"{f.name}: VALIDATE CONSTRAINT runs in the migration transaction, "
                            "under the lock its other statements hold; move it to a "
                            "`-- no-transaction` migration")
        if not re.search(r"SET\s+(?:LOCAL\s+)?lock_timeout", sql, re.I) and not esc("lock_timeout"):
            problems.append(f"{f.name}: no SET [LOCAL] lock_timeout")
        for stmt in sql.split(";"):
            for m in re.finditer(r"ADD\s+CONSTRAINT\s+(\w+)\s+(CHECK|FOREIGN\s+KEY)\b([^;]*?)(?=,\s*ADD\s|$)", stmt, re.I | re.S):
                name, rest = m.group(1).lower(), m.group(3)
                if not re.search(r"NOT\s+VALID\s*$", rest.strip(), re.I) and not esc("constraint"):
                    problems.append(f"{f.name}: constraint {name} is not added NOT VALID")
                elif name not in validated and not esc("constraint"):
                    problems.append(f"{f.name}: constraint {name} is never VALIDATEd")
            # A CHECK written on an added column is validated by a full scan
            # under the table's lock, as an ADD CONSTRAINT ... CHECK is.
            if (re.search(r"ADD\s+COLUMN\s+(?:IF\s+NOT\s+EXISTS\s+)?\w+[^,;]*?\bCHECK\s*\(", stmt, re.I | re.S)
                    and not esc("column-check")):
                problems.append(f"{f.name}: an inline CHECK on an added column is validated under the "
                                "table's lock; add it as a constraint NOT VALID, or escape it with its reason")
            for m in re.finditer(r"CREATE\s+(?:UNIQUE\s+)?INDEX\s+(?:CONCURRENTLY\s+)?(?:IF\s+NOT\s+EXISTS\s+)?(\w+)\s+ON\s+(ONLY\s+)?(\w+)", stmt, re.I):
                iname, only, table = m.group(1).lower(), m.group(2), m.group(3).lower()
                if table in part and not esc("index"):
                    if not only:
                        problems.append(f"{f.name}: index {iname} on partitioned {table} lacks ON ONLY")
                    elif not re.search(rf"ALTER\s+INDEX\s+{iname}\s+ATTACH\s+PARTITION|ATTACH\s+PARTITION", sql, re.I):
                        problems.append(f"{f.name}: index {iname} ON ONLY {table} has no per-partition ATTACH")
    return problems


# ---- ports ------------------------------------------------------------------------------

PORT = re.compile(r"localhost:5432|127\.0\.0\.1:5432|PGPORT:-5432")
SCOPE_DIRS = ("tests", "testing", "scripts", "eval", "bench")
SKIP_DIRS = {"target", ".git", "node_modules", "docs", "results", "fixtures", ".tmp", "tests/fixtures"}
EXT = {".rs", ".py", ".sh", ".ts", ".js", ".yaml", ".yml", ".toml", ".json"}


def lint_ports(root: Path, use_allow: bool = True) -> list[str]:
    skip = allowed("ports.txt", root, use_allow)
    problems = []
    for p in root.rglob("*"):
        rel = p.relative_to(root)
        parts = rel.parts
        if not p.is_file() or p.suffix not in EXT and p.name != "Makefile":
            continue
        if set(parts[:-1]) & SKIP_DIRS or parts[0] == "scripts" and "tests" in parts:
            continue
        if not (set(parts[:-1]) & set(SCOPE_DIRS)):
            continue
        if str(rel) in skip:
            continue
        try:
            lines = p.read_text().splitlines()
        except (UnicodeDecodeError, OSError):
            continue
        for i, line in enumerate(lines, 1):
            if PORT.search(line) and "lint:allow-5432" not in line:
                problems.append(f"{rel}:{i}: defaults to port 5432: {line.strip()[:90]}")
    return problems


# ---- like -------------------------------------------------------------------------------

LIKE = re.compile(r"\b(?:NOT\s+)?I?LIKE\b(?!\s+\{?\w+\}?\s+INCLUDING)", re.I)


def lint_like(root: Path, use_allow: bool = True) -> list[str]:
    skip = allowed("like.txt", root, use_allow)
    problems = []
    for f in sorted((root / "crates" / "adapters" / "oml-pg" / "src").rglob("*.rs")):
        rel = str(f.relative_to(root))
        lines = f.read_text().splitlines()
        for i, line in enumerate(lines):
            code = line.strip()
            if code.startswith("//") or not LIKE.search(code):
                continue
            prev = lines[i - 1] if i else ""
            if "lint:allow-like" in line or "lint:allow-like" in prev or f"{rel}:{i + 1}" in skip:
                continue
            problems.append(f"{rel}:{i + 1}: LIKE operator in candidate SQL: {code[:90]}")
    return problems


# ---- actions ----------------------------------------------------------------------------

PIN = re.compile(r"^(?P<action>[\w.-]+/[\w./-]+)@(?P<sha>[0-9a-f]{40})$")
TAG_COMMENT = re.compile(r"^#\s*v\d\S*(\s|$)")


def uses_values(node, seen: set[int]):
    """Every value of a `uses` key in a composed YAML document, however the key or the value is written: in a flow
    mapping, quoted, with a space before the colon, on the next line, tagged, anchored, aliased or merged in."""
    import yaml

    if id(node) in seen:
        return
    seen.add(id(node))
    if isinstance(node, yaml.MappingNode):
        for key, value in node.value:
            if isinstance(key, yaml.ScalarNode) and key.value == "uses":
                yield value
            yield from uses_values(key, seen)
            yield from uses_values(value, seen)
    elif isinstance(node, yaml.SequenceNode):
        for item in node.value:
            yield from uses_values(item, seen)


def lint_actions(root: Path, use_allow: bool = True) -> list[str]:
    """A tag can be moved to other code; a commit cannot, so a pin names the commit and the comment names the tag a reader can check it against.

    Each workflow is parsed as YAML and every `uses` key is read from the parsed document, so no way of writing the key
    passes unread; a file that does not parse is a problem, never skipped. One action is one repository: a sub-path of
    it (`actions/cache/restore`) at another commit is the same action at two commits."""
    import yaml

    problems = []
    seen: dict[str, dict[str, str]] = {}
    files = sorted((root / ".github").rglob("*.yml")) + sorted((root / ".github").rglob("*.yaml"))
    for f in files:
        rel = f.relative_to(root)
        text = f.read_text()
        lines = text.splitlines()
        try:
            documents = [d for d in yaml.compose_all(text) if d is not None]
        except yaml.YAMLError as e:
            problems.append(f"{rel}: is not YAML, so its actions cannot be read ({str(e).splitlines()[0]})")
            continue
        visited: set[int] = set()
        for document in documents:
            for value in uses_values(document, visited):
                at = f"{rel}:{value.start_mark.line + 1}"
                if not isinstance(value, yaml.ScalarNode):
                    problems.append(f"{at}: `uses` is not a string, so it names no action a pin can be read from")
                    continue
                target = value.value.strip()
                if target.startswith("./"):
                    continue
                pin = PIN.match(target)
                if not pin:
                    problems.append(f"{at}: {target} is not pinned to a 40-hex commit SHA")
                    continue
                end = value.end_mark
                rest = lines[end.line][end.column:] if end.line < len(lines) else ""
                if not TAG_COMMENT.match(rest.lstrip(" \t\"',}]")):
                    problems.append(f"{at}: {target} has no trailing `# v<tag>` comment naming the tag it was resolved from")
                repository = "/".join(pin["action"].lower().split("/")[:2])
                seen.setdefault(repository, {}).setdefault(pin["sha"], at)
    for action, shas in sorted(seen.items()):
        if len(shas) > 1:
            where = ", ".join(f"{sha[:12]} at {at}" for sha, at in sorted(shas.items()))
            problems.append(f".github: {action} is pinned to {len(shas)} different commits ({where}); use one")
    return problems


LINTS = {"migrations": lint_migrations, "ports": lint_ports, "like": lint_like, "actions": lint_actions}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("lint", choices=sorted(LINTS))
    ap.add_argument("--root", default=str(HERE.parent))
    ap.add_argument("--no-allowlist", action="store_true", help="report the allow-listed violations too")
    a = ap.parse_args(argv)
    problems = LINTS[a.lint](Path(a.root).resolve(), not a.no_allowlist)
    for p in problems:
        print(f"{a.lint}: {p}")
    print(f"{a.lint}: {'FAIL' if problems else 'ok'}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
