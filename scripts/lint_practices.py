#!/usr/bin/env python3
"""Five repository lints (docs/contributing/review-checklist.md).

    python scripts/lint_practices.py migrations|ports|like|actions|voice [--root DIR] [--no-allowlist]

migrations  every migration not in lint_allow/migrations.txt sets lock_timeout, adds CHECK and
            FOREIGN KEY constraints NOT VALID (an inline CHECK on an added column too) and validates them (in that or a later
            migration, and only in a migration marked `-- no-transaction`), and on a partitioned table creates indexes ON ONLY the parent and
            ATTACHes the per-partition indexes.
ports       no localhost:5432, 127.0.0.1:5432 or PGPORT:-5432 default in tests or scripts.
like        no LIKE or ILIKE operator in the SQL of crates/adapters/oml-pg/src; row security
            runs a non-leakproof operator before its policy (ADR-0052).
actions     every `uses:` under .github/ is `owner/repo@<40 hex> # v<tag>`, and one action is one
            commit in every workflow (docs/contributing/ci-and-toolchain.md "Pinned actions").
voice       no second- or first-person word (you, your, yours, yourself, yourselves, we, our, ours,
            ourselves, us, me, my, mine, the pronoun I, let's) in the prose of docs/**/*.md,
            changes/*.md and the descriptions in docs/api/mcp-tools.json: code (fenced, indented,
            inline), HTML comments, link targets, URLs, quotations and blockquotes are not prose,
            and a file in lint_allow/voice.txt is skipped (design/README.md, the neutral voice).

An escape is a reasoned line comment: `-- lint:allow <rule> <why>` in SQL,
`// lint:allow-like <why>` in Rust, `lint:allow-5432 <why>` anywhere on the line,
`<!-- lint:allow-voice <why> -->` on a Markdown line, the reason not empty.
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


def read_uses(root: Path) -> tuple[list[tuple[str, str, str]], list[str]]:
    """Every `uses` value under .github/ as (where, target, what follows it on its line), and a problem for each
    file that is not YAML and each `uses` that is not a string. scripts/check_action_updates.py reads the pins
    through this, so the weekly check sees every use the lint sees."""
    import yaml

    uses, problems = [], []
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
                end = value.end_mark
                rest = lines[end.line][end.column:] if end.line < len(lines) else ""
                uses.append((at, value.value.strip(), rest.lstrip(" \t\"',}]")))
    return uses, problems


def repository(action: str) -> str:
    """The repository an action lives in: `actions/cache/restore` is `actions/cache`."""
    return "/".join(action.lower().split("/")[:2])


def lint_actions(root: Path, use_allow: bool = True) -> list[str]:
    """A tag can be moved to other code; a commit cannot, so a pin names the commit and the comment names the tag a reader can check it against.

    Each workflow is parsed as YAML and every `uses` key is read from the parsed document, so no way of writing the key
    passes unread; a file that does not parse is a problem, never skipped. One action is one repository: a sub-path of
    it (`actions/cache/restore`) at another commit is the same action at two commits."""
    uses, problems = read_uses(root)
    seen: dict[str, dict[str, str]] = {}
    for at, target, comment in uses:
        if target.startswith("./"):
            continue
        pin = PIN.match(target)
        if not pin:
            problems.append(f"{at}: {target} is not pinned to a 40-hex commit SHA")
            continue
        if not TAG_COMMENT.match(comment):
            problems.append(f"{at}: {target} has no trailing `# v<tag>` comment naming the tag it was resolved from")
        seen.setdefault(repository(pin["action"]), {}).setdefault(pin["sha"], at)
    for action, shas in sorted(seen.items()):
        if len(shas) > 1:
            where = ", ".join(f"{sha[:12]} at {at}" for sha, at in sorted(shas.items()))
            problems.append(f".github: {action} is pinned to {len(shas)} different commits ({where}); use one")
    return problems


# ---- voice ------------------------------------------------------------------------------

# The neutral voice (design/README.md): no second or first person in reader-facing prose.
VOICE = re.compile(
    r"(?<![-\w'’])(?:let['’]s|you|your|yours|yourself|yourselves|we|our|ours|ourselves|us|me|my|mine)(?![-\w'’])"
    r"|(?<![-\w/.'’])I(?:['’](?:m|ve|ll|d))?(?![-\w/.'’])",
    re.I,
)
# "I" counts only as the capital pronoun: a lower-case i, and an I after a word that numbers
# (Phase I, Type I) or inside a Roman numeral (II, IV), are not it.
NUMBERING = re.compile(r"(?:phase|type|part|tier|class|level|stage|appendix|chapter|volume|annex|article)\s*$", re.I)
ESCAPE = re.compile(r"<!--\s*lint:allow-voice\s+\S[^>]*-->")
FENCE = re.compile(r"^\s*(```|~~~)")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s")
CODE = [
    re.compile(r"`[^`]*`"),                                    # inline code, also over lines
    re.compile(r"<!--.*?-->", re.S),                           # HTML comments, also over lines
]
NOT_PROSE = [
    re.compile(r"\]\([^)]*\)"),                                # link targets
    re.compile(r"<[^>\s]+>"),                                  # autolinks and bare tags
    re.compile(r"https?://\S+"),                               # bare URLs
    # A quotation opens after a non-word and closes before one; a lone mark pairs with nothing.
    re.compile(r'(?<![\w"])"(?=\S)[^"]*?(?<=\S)"(?![\w"])'),
    re.compile(r"(?<![\w'])'(?=\S)[^'\n]*?(?<=\S)'(?![\w'])"),
    re.compile(r"[“‘][^”’]*[”’]"),
]


def blank(match: re.Match) -> str:
    """Blank a span but keep its newlines, so line numbers survive."""
    return re.sub(r"[^\n]", " ", match.group(0))


def fill(match: re.Match) -> str:
    """Code or a comment becomes a run of filler that is no word, keeping its newlines, so a
    quotation that ends in code still closes on a non-space."""
    return re.sub(r"[^\n]", "\u00b7", match.group(0))


# The pronoun I is a capital followed by a lower-case word or a contraction; a label (Option I),
# a unit after a number (3.0 I) or a numeral (Phase I) is not it.
PRONOUN_I = re.compile(r"I(?:['’](?:m|ve|ll|d)\b| +[a-z])")


def voice_words(prose: str) -> list[str]:
    words = []
    for m in VOICE.finditer(prose):
        w = m.group(0)
        if w == "US":
            continue
        if w.lower() == "i" or w.lower().startswith(("i'", "i’")):
            before = prose[: m.start()]
            if (w[0] != "I" or not PRONOUN_I.match(prose, m.start())
                    or NUMBERING.search(before) or re.search(r"\d\s*$", before)):
                continue
        words.append(w)
    return words


def paragraphs(text: str):
    """Yield (first line number, paragraph text) for the prose blocks of a Markdown page.

    Fenced code, 4-space indented code blocks and blockquotes are left out."""
    lines = text.split("\n")
    fenced, block, start, prev_list = False, [], 0, False

    def flush():
        nonlocal block, prev_list
        if block:
            first = block[0]
            is_list = bool(LIST_ITEM.match(first))
            indented_code = (first.startswith(("    ", "\t")) and not is_list and not prev_list
                             and all(l.startswith(("    ", "\t")) for l in block))
            if not indented_code:
                yield start, "\n".join(l if not l.lstrip().startswith(">") else "" for l in block)
            prev_list = is_list or (prev_list and first.startswith((" ", "\t")))
        block = []

    for i, line in enumerate(lines, 1):
        if FENCE.match(line):
            yield from flush()
            fenced = not fenced
            continue
        if fenced:
            continue
        if not line.strip():
            yield from flush()
            continue
        if not block:
            start = i
        block.append(line)
    yield from flush()


def prose_of(text: str) -> str:
    for pat in CODE:
        text = pat.sub(fill, text)
    for pat in NOT_PROSE:
        text = pat.sub(blank, text)
    return text


def markdown_voice(rel: str, text: str) -> list[str]:
    problems = []
    raw = text.split("\n")
    # Multi-line comments are blanked over the whole page first, keeping the line count.
    text = re.sub(r"<!--.*?-->", lambda m: m.group(0) if ESCAPE.fullmatch(m.group(0)) else blank(m), text, flags=re.S)
    for start, para in paragraphs(text):
        for k, line in enumerate(prose_of(para).split("\n")):
            n = start + k
            if ESCAPE.search(raw[n - 1]):
                continue
            words = voice_words(line)
            if words:
                problems.append(f"{rel}:{n}: {', '.join(words)}: {raw[n - 1].strip()[:90]}")
    return problems


def descriptions(node, path=""):
    if isinstance(node, dict):
        for k, v in node.items():
            here = f"{path}.{k}" if path else k
            if k == "description" and isinstance(v, str):
                yield here, v
            else:
                yield from descriptions(v, here)
    elif isinstance(node, list):
        for i, v in enumerate(node):
            name = v.get("name") if isinstance(v, dict) and isinstance(v.get("name"), str) else str(i)
            yield from descriptions(v, f"{path}[{name}]")


def lint_voice(root: Path, use_allow: bool = True) -> list[str]:
    """docs/**/*.md, changes/*.md and the descriptions of docs/api/mcp-tools.json."""
    import json

    skip = allowed("voice.txt", root, use_allow)
    problems = []
    pages = sorted((root / "docs").rglob("*.md")) + sorted((root / "changes").glob("*.md"))
    for f in pages:
        rel = f.relative_to(root).as_posix()
        if any(Path(rel).match(pat) for pat in skip):
            continue
        problems += markdown_voice(rel, f.read_text())
    tools = root / "docs" / "api" / "mcp-tools.json"
    if tools.exists():
        for where, text in descriptions(json.loads(tools.read_text())):
            words = voice_words(prose_of(text))
            if words:
                problems.append(f"docs/api/mcp-tools.json: {where}: {', '.join(words)}")
    return problems


LINTS = {"migrations": lint_migrations, "ports": lint_ports, "like": lint_like, "actions": lint_actions,
         "voice": lint_voice}


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
