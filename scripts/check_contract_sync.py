#!/usr/bin/env python3
"""Contract-sync check for the OML design spec.

The API contract is declared on three surfaces that must agree by construction:

  1. docs/api/openapi.yaml       — the ErrorCode enum and the paths section
  2. docs/api/errors.md          — the per-family code tables and the status-code index
  3. docs/api/http-api.md        — the endpoint surface (tables, bullets, headings)

This script extracts each surface and fails with a diff on any disagreement:

  * an error code present in one of {enum, family tables, status index} and not the others
  * a code whose HTTP status differs between its family table row and the status index
  * a path+method present in openapi.yaml and never documented on http-api.md,
    or documented on http-api.md and absent from openapi.yaml

It checks AGREEMENT only — no count or code name is hard-coded, so the closed
taxonomy may grow without touching this script.

Parsing rules (kept deliberately dumb and documented):

  * Markdown pipe tables are parsed row-wise; "\\|" inside a cell is an escaped pipe.
  * A family table is any table whose header starts | Code | HTTP |.
  * The status index is the table whose header starts | HTTP | Codes |.
  * An error code is a backticked token shaped family.name (lowercase + underscores).
  * An endpoint is a backticked span "METHOD /path" anywhere in http-api.md.
    Query strings are stripped. A span whose path contains ":" (e.g. the reserved
    POST /v1/memories:batch, FR-12) is a future reservation, not current contract,
    and is skipped.
  * http-api.md groups endpoints as `GET /a/{id}` · `/tree` · `/edges`: a bare
    "/suffix" span joined by the "·" separator continues the previous span —
    appended when the previous path ends in a {placeholder} segment
    (`GET /v2/exports/{export_id}` · `/bundle`), otherwise replacing the final
    segment (`POST /v2/memories/{id}/reassign` · `/demote`).
  * Path placeholders are normalized to {} before comparison, because http-api.md
    writes {id} where openapi.yaml writes {workarea_id}.

Dependencies: stdlib + PyYAML.
Exit status: 0 when all surfaces agree, 1 with a readable diff otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover
    sys.exit("check_contract_sync.py requires PyYAML: pip install pyyaml")

ROOT = Path(__file__).resolve().parent.parent
ERRORS_MD = ROOT / "docs" / "api" / "errors.md"
HTTP_API_MD = ROOT / "docs" / "api" / "http-api.md"
OPENAPI_YAML = ROOT / "docs" / "api" / "openapi.yaml"

CODE_RE = re.compile(r"`([a-z_]+\.[a-z_]+)`")
SPAN_RE = re.compile(r"`([^`]+)`")
OP_SPAN_RE = re.compile(r"^(GET|POST|PUT|DELETE|PATCH)\s+(/\S+)$")
SUFFIX_SPAN_RE = re.compile(r"^/[A-Za-z0-9_{}\-]+$")
PLACEHOLDER_RE = re.compile(r"\{[^}]*\}")
SEPARATOR_CELL_RE = re.compile(r"^:?-{3,}:?$")
OPENAPI_METHODS = {"get", "put", "post", "delete", "patch", "head", "options", "trace"}


def normalize_path(path: str) -> str:
    """{workarea_id} and {id} are the same hole: normalize every placeholder to {}."""
    return PLACEHOLDER_RE.sub("{}", path)


def parse_pipe_tables(text: str) -> list[list[list[str]]]:
    """Return every markdown pipe table as a list of rows of stripped cells."""
    tables: list[list[list[str]]] = []
    current: list[list[str]] = []
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("|"):
            cells = [c.strip() for c in re.split(r"(?<!\\)\|", stripped)]
            if cells and cells[0] == "":
                cells = cells[1:]
            if cells and cells[-1] == "":
                cells = cells[:-1]
            if cells and all(SEPARATOR_CELL_RE.fullmatch(c) for c in cells):
                continue  # the |---|---| separator row
            current.append(cells)
        else:
            if current:
                tables.append(current)
                current = []
    if current:
        tables.append(current)
    return tables


def errors_md_family_codes(text: str) -> tuple[dict[str, int], list[str]]:
    """code -> HTTP status from every family table (header | Code | HTTP | ...)."""
    codes: dict[str, int] = {}
    problems: list[str] = []
    for table in parse_pipe_tables(text):
        header = table[0]
        if len(header) < 2 or header[0] != "Code" or header[1] != "HTTP":
            continue
        for row in table[1:]:
            if len(row) < 2:
                continue
            code_match = CODE_RE.search(row[0])
            if not code_match:
                problems.append(f"family table row with unparseable code cell: {row[0]!r}")
                continue
            code = code_match.group(1)
            if not re.fullmatch(r"\d{3}", row[1]):
                problems.append(f"family table row for `{code}` has non-numeric HTTP cell: {row[1]!r}")
                continue
            status = int(row[1])
            if code in codes and codes[code] != status:
                problems.append(
                    f"`{code}` appears in two family tables with different statuses: "
                    f"{codes[code]} and {status}"
                )
            codes[code] = status
    return codes, problems


def errors_md_status_index(text: str) -> tuple[dict[str, set[int]], list[str]]:
    """code -> set of HTTP statuses from the status-code index (header | HTTP | Codes |)."""
    index: dict[str, set[int]] = {}
    problems: list[str] = []
    found = False
    for table in parse_pipe_tables(text):
        header = table[0]
        if len(header) < 2 or header[0] != "HTTP" or header[1] != "Codes":
            continue
        found = True
        for row in table[1:]:
            if len(row) < 2:
                continue
            if not re.fullmatch(r"\d{3}", row[0]):
                problems.append(f"status index row with non-numeric HTTP cell: {row[0]!r}")
                continue
            status = int(row[0])
            row_codes = CODE_RE.findall(row[1])
            if not row_codes:
                problems.append(f"status index row {status} lists no parseable codes: {row[1]!r}")
            for code in row_codes:
                index.setdefault(code, set()).add(status)
    if not found:
        problems.append("no status-code index table (header | HTTP | Codes |) found in errors.md")
    return index, problems


def openapi_error_codes(spec: dict) -> list[str]:
    return spec["components"]["schemas"]["ErrorCode"]["enum"]


def openapi_ops(spec: dict) -> set[tuple[str, str]]:
    ops: set[tuple[str, str]] = set()
    for path, item in spec["paths"].items():
        if not isinstance(item, dict):
            continue
        for method in item:
            if method.lower() in OPENAPI_METHODS:
                ops.add((method.upper(), normalize_path(path)))
    return ops


def http_api_md_ops(text: str) -> set[tuple[str, str]]:
    """Every documented METHOD+path on http-api.md, grouped spans expanded."""
    ops: set[tuple[str, str]] = set()
    for line in text.splitlines():
        last: tuple[str, str] | None = None  # (METHOD, raw path) of the previous span
        prev_end: int | None = None
        for match in SPAN_RE.finditer(line):
            span = match.group(1).strip()
            separator = line[prev_end : match.start()] if prev_end is not None else None
            prev_end = match.end()

            op = OP_SPAN_RE.match(span)
            if op:
                method, raw = op.group(1), op.group(2).split("?", 1)[0]
                if ":" in raw:
                    last = None  # reserved future endpoint (e.g. /v1/memories:batch)
                    continue
                ops.add((method, normalize_path(raw)))
                last = (method, raw)
                continue

            if (
                last is not None
                and separator is not None
                and separator.strip() == "·"
                and SUFFIX_SPAN_RE.fullmatch(span)
            ):
                method, prev_path = last
                segments = prev_path.rstrip("/").split("/")
                if segments[-1].startswith("{"):
                    new_path = prev_path + span  # append after a placeholder
                else:
                    new_path = "/".join(segments[:-1]) + span  # sibling action verb
                ops.add((method, normalize_path(new_path)))
                last = (method, new_path)
                continue

            last = None  # any other span breaks a grouped run
    return ops


def format_ops(ops: set[tuple[str, str]]) -> list[str]:
    return [f"{method} {path}" for method, path in sorted(ops, key=lambda t: (t[1], t[0]))]


def main() -> int:
    failures: list[tuple[str, list[str]]] = []

    errors_text = ERRORS_MD.read_text(encoding="utf-8")
    http_text = HTTP_API_MD.read_text(encoding="utf-8")
    spec = yaml.safe_load(OPENAPI_YAML.read_text(encoding="utf-8"))

    family_codes, family_problems = errors_md_family_codes(errors_text)
    index_codes, index_problems = errors_md_status_index(errors_text)
    enum_codes = openapi_error_codes(spec)
    spec_ops = openapi_ops(spec)
    doc_ops = http_api_md_ops(http_text)

    if family_problems:
        failures.append(("errors.md family tables: parse problems", family_problems))
    if index_problems:
        failures.append(("errors.md status index: parse problems", index_problems))

    # Sanity floor: an empty extraction means the parser broke or a file moved,
    # and a silent pass on an empty set would defeat the whole check.
    for label, size in [
        ("errors.md family-table codes", len(family_codes)),
        ("errors.md status-index codes", len(index_codes)),
        ("openapi.yaml ErrorCode enum", len(enum_codes)),
        ("openapi.yaml path+method ops", len(spec_ops)),
        ("http-api.md documented ops", len(doc_ops)),
    ]:
        if size < 10:
            failures.append(
                (f"implausibly small extraction: {label}",
                 [f"only {size} items extracted — the parser or the file layout broke"])
            )

    enum_dupes = sorted({c for c in enum_codes if enum_codes.count(c) > 1})
    if enum_dupes:
        failures.append(("openapi.yaml ErrorCode enum: duplicate entries", enum_dupes))

    enum_set, family_set, index_set = set(enum_codes), set(family_codes), set(index_codes)
    for a_name, a, b_name, b in [
        ("openapi.yaml ErrorCode enum", enum_set, "errors.md family tables", family_set),
        ("openapi.yaml ErrorCode enum", enum_set, "errors.md status index", index_set),
        ("errors.md family tables", family_set, "errors.md status index", index_set),
    ]:
        only_a = sorted(a - b)
        only_b = sorted(b - a)
        if only_a or only_b:
            lines = [f"only in {a_name}: {c}" for c in only_a]
            lines += [f"only in {b_name}: {c}" for c in only_b]
            failures.append((f"error-code mismatch: {a_name} vs {b_name}", lines))

    status_lines = []
    for code in sorted(family_set & index_set):
        family_status = {family_codes[code]}
        if index_codes[code] != family_status:
            status_lines.append(
                f"{code}: family table says {sorted(family_status)}, "
                f"status index says {sorted(index_codes[code])}"
            )
    if status_lines:
        failures.append(("HTTP status disagreement between family tables and status index", status_lines))

    only_spec = spec_ops - doc_ops
    only_docs = doc_ops - spec_ops
    if only_spec:
        failures.append(
            ("paths in openapi.yaml never documented on http-api.md", format_ops(only_spec))
        )
    if only_docs:
        failures.append(
            ("paths documented on http-api.md but absent from openapi.yaml", format_ops(only_docs))
        )

    if failures:
        print("contract-sync check FAILED\n")
        for title, lines in failures:
            print(f"== {title} ==")
            for line in lines:
                print(f"  {line}")
            print()
        return 1

    print("contract-sync check passed")
    print(f"  error codes agreeing on all three surfaces: {len(enum_set)}")
    print(f"  path+method operations agreeing: {len(spec_ops)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
