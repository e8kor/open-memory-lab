#!/usr/bin/env python3
"""Contract-sync check for the OML design spec.

The API contract is declared on three surfaces that must agree by construction:

  1. docs/api/openapi.yaml       — the ErrorCode enum and the served paths, and
     docs/api/openapi-designed.yaml — the designed paths and, under
                                   x-oml-designed-fields, the designed fields of
                                   served operations (ADR-0039 d9)
  2. docs/api/errors.md          — the per-family code tables and the status-code index
  3. docs/api/http-api.md        — generated from openapi.yaml by gen_http_api_reference.py,
                                   whose --check mode keeps it in step; not parsed here

This script extracts each surface and fails with a diff on any disagreement:

  * an error code present in one of {enum, family tables, status index} and not the others
  * a code whose HTTP status differs between its family table row and the status index
  * a path+method present in either OpenAPI document and never documented on
    http-api.md, or documented on http-api.md and absent from both
  * a path+method declared in both OpenAPI documents
  * an x-oml-designed-fields entry that names no served operation, or a side
    other than request and response, or a field the served operation's request
    (body properties and parameters) or 2xx response already declares
  * a surface-table row on http-api.md whose Contract cell says served for an
    operation openapi.yaml does not declare, or designed for one
    openapi-designed.yaml does not declare
  * a key with a null value in either OpenAPI document outside an example,
    default or const: an unquoted ", " in a flow mapping splits a description
    into a truncated one and a garbage key, which the 3.1 validator accepts
  * a `memory.<field>` read in README.md, an SDK README or a script under
    scripts/, other than the host of a URL such as https://memory.example.com,
    that names a field the served Memory schema does not declare:
    whatever names a field moves in the step that renames it (ADR-0039 d14)
  * docs/api/mcp-tools.json's tools, in order, other than the tools/list the
    facade serves (SERVED_TOOLS in crates/transports/oml-transport-http/src/
    mcp.rs, which serves that file whole); a tool docs/api/mcp-tools.designed.json
    and mcp-tools.json both name, or a designed tool the facade does not name as
    designed (DESIGNED_TOOLS); and a designed argument that names no served
    tool or that the served tool's inputSchema already declares (ADR-0039 d2)
  * a retired CLI verb, the designed `oml context add|use|list|show|remove`,
    run on a reader page (README.md, an SDK README, docs/ outside adr/, atlas/
    and changelog.md): `oml login` with one credentials file per identity
    replaces it, and a page that still runs it contradicts the reference
    (ADR-0039 d15)
  * a v0.3.0 spelling from d10's table on a start page: the eight tool names
    (V0_3_0_TOOLS below, removed in v0.5.1),
    `X-OML-Workarea`, `include_own_held`, `OML_CLI_SERVER`, `OML_CLI_KEY`, the
    `--token`, `--server` and `--key` flags, the verbs `oml submit`, `oml recall`,
    `oml supersede` and `oml review verdict`, or the release 0.3.2 or v0.3.2,
    the pattern first checked against probe lines it must catch and pass, on a
    start page (README.md, a plugin or SDK README, a plugin skill,
    kit/README.md, docs/index.md, docs/llms.txt, docs/engineering/sdks.md,
    docs/operations/, docs/start/, docs/guides/, docs/product/use-cases.md,
    docs/product/scenarios.md or a script under scripts/) outside a section headed
    "Upgrading from …": the migration names the old spelling, a start page
    teaches the new one (ADR-0039 d13, d14 step 5)

  * an entry of the hand-written Rust SDK's operation table (OPERATIONS in
    sdks/rust/src/lib.rs, "<operationId> <METHOD> <path>" then query: and
    body: field lists and resp:<the struct the answer decodes into>) whose
    operationId openapi.yaml does not serve, whose method or path differs
    from the operation's, which sends a query parameter or a body field the
    operation does not declare, or whose response struct, or any struct it
    holds at any depth, reads a field the 2xx schema does not declare there,
    or meets a schema that declares no fields (ADR-0070 d9)

It checks AGREEMENT only — no count or code name is hard-coded, so the closed
taxonomy may grow without touching this script.

Parsing rules (kept deliberately dumb and documented):

  * Markdown pipe tables are parsed row-wise; "\\|" inside a cell is an escaped pipe.
  * A family table is any table whose header starts | Code | HTTP |.
  * The status index is the table whose header starts | HTTP | Codes |.
  * An error code is a backticked token shaped family.name (lowercase + underscores).
  * An endpoint is a backticked span "METHOD /path" anywhere in http-api.md.
    Query strings are stripped. A span whose path contains ":" (e.g. the reserved
    POST /v1/memories:batch, #122) is a future reservation, not current contract,
    and is skipped.
  * http-api.md groups endpoints as `GET /a/{id}` · `/tree` · `/edges`: a bare
    "/suffix" span joined by the "·" separator continues the previous span —
    appended when the previous path ends in a {placeholder} segment
    (`GET /v2/exports/{export_id}` · `/bundle`), otherwise replacing the final
    segment (`POST /v2/memories/{id}/reassign` · `/demote`).
  * Path placeholders are normalized to {} before comparison, because http-api.md
    writes {id} where openapi.yaml writes {workarea_id}.
  * A surface table is any table with a Contract column; its cell is served or
    designed, and it marks every endpoint span of its row.
  * A $ref is followed within its own document, or into openapi.yaml when it
    names that file; allOf members contribute their properties.

Dependencies: stdlib + PyYAML.
Exit status: 0 when all surfaces agree, 1 with a readable diff otherwise.
"""

from __future__ import annotations

import json
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
DESIGNED_YAML = ROOT / "docs" / "api" / "openapi-designed.yaml"
DESIGNED_FIELDS = "x-oml-designed-fields"
MCP_TOOLS_JSON = ROOT / "docs" / "api" / "mcp-tools.json"
MCP_DESIGNED_JSON = ROOT / "docs" / "api" / "mcp-tools.designed.json"
MCP_RS = ROOT / "crates" / "transports" / "oml-transport-http" / "src" / "mcp.rs"
RUST_SDK_RS = ROOT / "sdks" / "rust" / "src" / "lib.rs"
MEMORY_READERS = [ROOT / "README.md", *sorted((ROOT / "sdks").glob("*/README.md")),
                  *sorted((ROOT / "scripts").glob("*.sh"))]
# A host such as https://memory.example.com is no field read: a match right
# after `//` is skipped.
MEMORY_READ_RE = re.compile(r"(?<!//)\bmemory\??\.([a-z_]+)\b(?!\.[a-z])")
CONTRACT_MARKS = {"served", "designed"}
RETIRED_VERB_RE = re.compile(r"\boml context (?:add|use|list|show|remove)\b")
RETIRED_VERB_EXEMPT = [ROOT / "docs" / "adr", ROOT / "docs" / "atlas", ROOT / "docs" / "changelog.md"]
# The eight tool names v0.3.0 served, removed in v0.5.1 (ADR-0039 d12), which
# no start page teaches.
V0_3_0_TOOLS = [
    "oml_remember", "oml_learn", "oml_recall", "oml_get",
    "oml_supersede", "oml_my_work", "oml_workarea_info", "oml_events_tail",
]


# d10's v0.3.0 column: the tool names, the header, the recall flag, the
# variables, the flags, the CLI verbs, and the release 0.3.2 the rewrite
# replaced with v0.4.0, spelled with or without a "v" and at a sentence's end.
RETIRED_NAME_RE = re.compile(
    "|".join(rf"\b{re.escape(tool)}\b" for tool in V0_3_0_TOOLS)
    + r"|X-OML-Workarea|\binclude_own_held\b|\bOML_CLI_(?:SERVER|KEY)\b"
    r"|(?<![-\w])--(?:token|server|key)(?![-\w])"
    r"|\boml (?:submit|recall|supersede|review verdict)\b"
    r"|(?<![\w.])v?0\.3\.2(?!\.?\w)"
)
MIGRATION_HEADING_RE = re.compile(r"^(#+) Upgrading from ")

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


def http_api_md_marks(text: str) -> tuple[dict[tuple[str, str], set[str]], list[str]]:
    """(METHOD, path) -> the Contract marks its surface-table rows give it."""
    marks: dict[tuple[str, str], set[str]] = {}
    problems: list[str] = []
    for table in parse_pipe_tables(text):
        header = table[0]
        if "Contract" not in header:
            continue
        column = header.index("Contract")
        for row in table[1:]:
            mark = row[column].strip("`* ") if column < len(row) else ""
            if mark not in CONTRACT_MARKS:
                problems.append(f"surface row with Contract cell {mark!r}, not served or designed: {row[0]}")
                continue
            for op in http_api_md_ops(row[0]):
                marks.setdefault(op, set()).add(mark)
    return marks, problems


def operations_by_id(spec: dict) -> dict[str, dict]:
    ops: dict[str, dict] = {}
    for item in spec["paths"].values():
        if not isinstance(item, dict):
            continue
        for method, op in item.items():
            if method.lower() in OPENAPI_METHODS and isinstance(op, dict) and "operationId" in op:
                ops[op["operationId"]] = op
    return ops


def resolve(node, docs: dict[str, dict], doc: str):
    """Follow $ref until a node without one; returns (node, the document it is in)."""
    seen = set()
    while isinstance(node, dict) and "$ref" in node:
        target, _, pointer = node["$ref"].partition("#")
        doc = target or doc
        if (doc, pointer) in seen:
            break
        seen.add((doc, pointer))
        node = docs[doc]
        for part in pointer.strip("/").split("/"):
            node = node[part]
    return node, doc


def schema_fields(schema, docs: dict[str, dict], doc: str) -> set[str]:
    """The property names a schema declares, through $ref, allOf, oneOf and anyOf."""
    schema, doc = resolve(schema, docs, doc)
    if not isinstance(schema, dict):
        return set()
    fields = set(schema.get("properties", {}))
    for combinator in ("allOf", "oneOf", "anyOf"):
        for member in schema.get(combinator, []):
            fields |= schema_fields(member, docs, doc)
    return fields


def served_fields(op: dict, docs: dict[str, dict]) -> dict[str, set[str]]:
    """The request fields (body properties, parameter names) and 2xx response fields of op."""
    request: set[str] = set()
    for parameter in op.get("parameters", []):
        parameter, _ = resolve(parameter, docs, "openapi.yaml")
        request.add(parameter["name"])
    body, _ = resolve(op.get("requestBody", {}), docs, "openapi.yaml")
    for media in body.get("content", {}).values():
        request |= schema_fields(media.get("schema", {}), docs, "openapi.yaml")
    response: set[str] = set()
    for status, answer in op.get("responses", {}).items():
        if not str(status).startswith("2"):
            continue
        answer, _ = resolve(answer, docs, "openapi.yaml")
        for media in answer.get("content", {}).values():
            response |= schema_fields(media.get("schema", {}), docs, "openapi.yaml")
    return {"request": request, "response": response}


def designed_field_problems(spec: dict, designed: dict) -> list[str]:
    docs = {"openapi.yaml": spec, "openapi-designed.yaml": designed}
    served = operations_by_id(spec)
    entries = designed.get(DESIGNED_FIELDS)
    if not isinstance(entries, dict) or not entries:
        return [f"openapi-designed.yaml declares no {DESIGNED_FIELDS} map"]
    problems: list[str] = []
    for operation_id, sides in entries.items():
        if operation_id not in served:
            problems.append(f"{operation_id}: names no operation openapi.yaml serves")
            continue
        fields = served_fields(served[operation_id], docs)
        for side, declared in (sides or {}).items():
            if side not in fields:
                problems.append(f"{operation_id}.{side}: a side other than request and response")
                continue
            for field in sorted(set(declared or {}) & fields[side]):
                problems.append(f"{operation_id}.{side}.{field}: designed and also served")
    return problems


def memory_read_problems(spec: dict) -> list[str]:
    """Each memory.<field> read on a reader surface that the served Memory schema lacks."""
    docs = {"openapi.yaml": spec}
    fields = schema_fields({"$ref": "#/components/schemas/Memory"}, docs, "openapi.yaml")
    if not fields:
        return ["openapi.yaml: the Memory schema declares no fields"]
    problems: list[str] = []
    for path in MEMORY_READERS:
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for field in MEMORY_READ_RE.findall(line):
                if field not in fields:
                    problems.append(f"{path.relative_to(ROOT)}:{number}: memory.{field}")
    return problems


def retired_verb_problems() -> list[str]:
    """Each line of a reader page that runs a CLI verb ADR-0039 d15 retired."""
    pages = [ROOT / "README.md", *sorted((ROOT / "sdks").glob("*/README.md")),
             *sorted((ROOT / "docs").rglob("*.md"))]
    problems: list[str] = []
    for path in pages:
        if any(path == e or e in path.parents for e in RETIRED_VERB_EXEMPT):
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for verb in RETIRED_VERB_RE.findall(line):
                problems.append(f"{path.relative_to(ROOT)}:{number}: {verb}")
    return problems


def start_pages() -> list[Path]:
    """The pages a new reader starts from, which teach only v0.4.0's names."""
    return [ROOT / "README.md", *sorted((ROOT / "plugins").glob("*/README.md")),
            *sorted((ROOT / "plugins").glob("*/skills/*/SKILL.md")),
            *sorted((ROOT / "sdks").glob("*/README.md")), ROOT / "kit" / "README.md",
            ROOT / "docs" / "index.md", ROOT / "docs" / "llms.txt",
            ROOT / "docs" / "engineering" / "sdks.md",
            *sorted((ROOT / "docs" / "operations").glob("*.md")),
            *sorted((ROOT / "docs" / "start").glob("*.md")),
            *sorted((ROOT / "docs" / "guides").glob("*.md")),
            *[p for p in [ROOT / "docs" / "product" / "use-cases.md",
                         ROOT / "docs" / "product" / "scenarios.md"] if p.exists()],
            *sorted((ROOT / "scripts").glob("*.sh"))]


# One line per v0.3.0 spelling d10 and d13 retire, which RETIRED_NAME_RE must
# catch, and the v0.4.0 spellings and look-alikes it must let through: the
# pattern is checked against these before it reads a page.
RETIRED_NAME_CATCHES = [
    "call oml_remember", "call oml_learn", "call oml_recall", "call oml_get",
    "call oml_supersede", "call oml_my_work", "call oml_workarea_info",
    "call oml_events_tail", "send X-OML-Workarea", "set include_own_held",
    "export OML_CLI_SERVER=x", "export OML_CLI_KEY=x", "oml bootstrap claim ci --token s",
    "oml --server https://x", "oml --key k", "oml submit --claim t", "oml recall --query q",
    "oml supersede a", "oml review verdict --approve", "tag v0.3.2", "ships in 0.3.2.",
    "0.3.2 ships",
]
RETIRED_NAME_PASSES = [
    "call oml_write", "call oml_read", "call oml_workarea", "oml write t", "oml read --query q",
    "oml review approve", "oml --url https://x --api-key k", "--token-stdin", "--key-file k",
    "--server-url x", "OML_URL and OML_API_KEY", "include_unreviewed", "tag v0.3.1",
    "0.3.20", "10.3.2", "0.3.2.1", "oml recalls", "oml submitted",
]


def retired_name_pattern_problems() -> list[str]:
    """Each probe line RETIRED_NAME_RE misreads: a retired spelling it misses or
    a v0.4.0 spelling it fails."""
    missed = [f"misses {line!r}" for line in RETIRED_NAME_CATCHES
              if not RETIRED_NAME_RE.search(line)]
    return missed + [f"fails {line!r}" for line in RETIRED_NAME_PASSES
                     if RETIRED_NAME_RE.search(line)]


def retired_name_problems() -> list[str]:
    """Each line of a start page that names a v0.3.0 spelling ADR-0039 d13 breaks,
    outside a section headed "Upgrading from …", whose migration names it."""
    problems: list[str] = []
    for path in start_pages():
        migration_level = 0
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            heading = re.match(r"^(#+) ", line) if path.suffix == ".md" else None
            if heading:
                level = len(heading.group(1))
                if migration_level and level <= migration_level:
                    migration_level = 0
                if MIGRATION_HEADING_RE.match(line):
                    migration_level = level
            if migration_level:
                continue
            for name in RETIRED_NAME_RE.findall(line):
                problems.append(f"{path.relative_to(ROOT)}:{number}: {name}")
    return problems


NULL_EXEMPT = {"example", "examples", "default", "const"}


def null_keys(node, path: str = "") -> list[str]:
    """Every key whose value is null, outside example, default and const subtrees."""
    found: list[str] = []
    if isinstance(node, dict):
        for key, value in node.items():
            if key in NULL_EXEMPT:
                continue
            here = f"{path}/{key}"
            if value is None:
                found.append(here)
            else:
                found += null_keys(value, here)
    elif isinstance(node, list):
        for index, value in enumerate(node):
            found += null_keys(value, f"{path}/{index}")
    return found


def rust_str_array(source: str, name: str) -> list[str] | None:
    """The string literals of `const NAME: [&str; n] = [...]` in `source`."""
    match = re.search(name + r": \[&str; \d+\] = \[([^\]]*)\]", source)
    return re.findall(r'"([^"]+)"', match.group(1)) if match else None


def mcp_catalog_problems() -> list[str]:
    """mcp-tools.json is the served tools/list and mcp-tools.designed.json the
    designed annex, disjoint from it (ADR-0039 d2)."""
    served = json.loads(MCP_TOOLS_JSON.read_text(encoding="utf-8"))["tools"]
    designed = json.loads(MCP_DESIGNED_JSON.read_text(encoding="utf-8"))
    mcp_rs = MCP_RS.read_text(encoding="utf-8")
    problems = []
    names = [t["name"] for t in served]
    listed = rust_str_array(mcp_rs, "SERVED_TOOLS")
    if listed is None:
        problems.append("SERVED_TOOLS not found in mcp.rs")
    elif names != listed:
        problems.append(f"mcp-tools.json declares {names}, the facade serves {listed}")
    designed_names = [t["name"] for t in designed.get("tools", [])]
    for name in sorted(set(designed_names) & set(names)):
        problems.append(f"{name} is in both mcp-tools.json and mcp-tools.designed.json")
    named_designed = rust_str_array(mcp_rs, "DESIGNED_TOOLS")
    if named_designed is None:
        problems.append("DESIGNED_TOOLS not found in mcp.rs")
    elif sorted(named_designed) != sorted(designed_names):
        problems.append(
            f"mcp-tools.designed.json declares {designed_names}, the facade names {named_designed} as designed"
        )
    schemas = {t["name"]: t["inputSchema"].get("properties", {}) for t in served}
    for tool, arguments in sorted(designed.get("designed_arguments", {}).items()):
        if tool not in schemas:
            problems.append(f"designed_arguments names {tool}, which mcp-tools.json does not serve")
            continue
        for argument in sorted(set(arguments) & set(schemas[tool])):
            problems.append(f"{tool}.{argument} is designed and also in the served inputSchema")
    return problems


RUST_STRUCT = re.compile(r"pub struct (\w+) \{\n(.*?)\n\}", re.S)
RUST_FIELD = re.compile(r"^\s*pub (\w+): (.+),$", re.M)
RUST_WRAPPERS = re.compile(r"^(?:Option|Vec|Box)<(.*)>$")


def rust_structs(source: str) -> dict[str, list[tuple[str, str, bool]]]:
    """Each `pub struct` in `source`: its `pub` fields as (name, inner type, through a Vec)."""
    structs = {}
    for name, body in RUST_STRUCT.findall(source):
        fields = []
        for field, ty in RUST_FIELD.findall(body):
            ty, listed = ty.strip(), False
            while (wrapped := RUST_WRAPPERS.match(ty)):
                listed |= ty.startswith("Vec<")
                ty = wrapped.group(1).strip()
            fields.append((field, ty, listed))
        structs[name] = fields
    return structs


def schema_candidates(schema, docs: dict[str, dict], doc: str) -> list[tuple[dict, str]]:
    """The object schemas `schema` may be, through $ref, allOf, oneOf and anyOf."""
    schema, doc = resolve(schema, docs, doc)
    if not isinstance(schema, dict):
        return []
    found = [(schema, doc)]
    for combinator in ("allOf", "oneOf", "anyOf"):
        for member in schema.get(combinator, []):
            found += schema_candidates(member, docs, doc)
    return found


def shape_problems(
    where: str, struct: str, candidates: list[tuple[dict, str]],
    structs: dict[str, list[tuple[str, str, bool]]], docs: dict[str, dict],
) -> list[str]:
    """Every field `struct` reads is a property its schema declares, at every depth."""
    properties: dict[str, list[tuple[dict, str]]] = {}
    for schema, doc in candidates:
        for name, prop in schema.get("properties", {}).items():
            properties.setdefault(name, []).append((prop, doc))
    if not properties:
        return [f"{where}: the schema {struct} reads declares no fields, so its shape cannot be checked"]
    problems = []
    for field, ty, listed in structs[struct]:
        if field not in properties:
            problems.append(f"{where}: the SDK's {struct}.{field} is a field openapi.yaml does not declare")
            continue
        if ty not in structs:
            continue
        nested: list[tuple[dict, str]] = []
        for prop, doc in properties[field]:
            for candidate, cdoc in schema_candidates(prop, docs, doc):
                if listed and "items" in candidate:
                    nested += schema_candidates(candidate["items"], docs, cdoc)
                elif not listed:
                    nested.append((candidate, cdoc))
        problems += shape_problems(f"{where}.{field}", ty, nested, structs, docs)
    return problems


def rust_sdk_problems(spec: dict, source: str | None = None) -> list[str]:
    """Each operation the Rust SDK calls is served by openapi.yaml under the
    same method and path, sends only query parameters and body fields it
    declares, and reads a response type whose every field, at every depth,
    the 2xx schema declares (ADR-0070 d9). The SDK is hand-written, so this
    is what keeps it on the contract the generated layers follow by
    construction."""
    if source is None:
        source = RUST_SDK_RS.read_text(encoding="utf-8")
    entries = rust_str_array(source, "OPERATIONS")
    if not entries:
        return ["OPERATIONS not found, or empty, in sdks/rust/src/lib.rs"]
    structs = rust_structs(source)
    served: dict[str, tuple[str, str, dict]] = {}
    for path, item in spec["paths"].items():
        if not isinstance(item, dict):
            continue
        for method, op in item.items():
            if method.lower() in OPENAPI_METHODS and isinstance(op, dict) and "operationId" in op:
                served[op["operationId"]] = (method.upper(), path, op)
    docs = {"openapi.yaml": spec}
    problems = []
    for entry in entries:
        words = entry.split()
        if len(words) < 3:
            problems.append(f"{entry!r}: not '<operationId> <METHOD> <path> ...'")
            continue
        op_id, method, path = words[:3]
        if op_id not in served:
            problems.append(f"{op_id}: openapi.yaml serves no operation with this id")
            continue
        want_method, want_path, op = served[op_id]
        if (method, path) != (want_method, want_path):
            problems.append(f"{op_id}: the SDK sends {method} {path}, openapi.yaml declares {want_method} {want_path}")
        query: set[str] = set()
        for parameter in op.get("parameters", []):
            parameter, _ = resolve(parameter, docs, "openapi.yaml")
            if parameter.get("in") == "query":
                query.add(parameter["name"])
        body: set[str] = set()
        request_body, _ = resolve(op.get("requestBody", {}), docs, "openapi.yaml")
        for media in request_body.get("content", {}).values():
            body |= schema_fields(media.get("schema", {}), docs, "openapi.yaml")
        response = None
        for word in words[3:]:
            kind, _, names = word.partition(":")
            if kind == "resp":
                response = names
                continue
            declared = {"query": query, "body": body}.get(kind)
            if declared is None:
                problems.append(f"{op_id}: {word!r} is not query:, body: or resp:")
                continue
            for name in names.split(","):
                if name not in declared:
                    side = {"query": "query parameter", "body": "request field"}[kind]
                    problems.append(f"{op_id}: the SDK sends the {side} {name!r}, which openapi.yaml does not declare")
        if response is None:
            problems.append(f"{op_id}: names no resp: type")
            continue
        if response not in structs:
            problems.append(f"{op_id}: resp:{response} is no pub struct in sdks/rust/src/lib.rs")
            continue
        candidates: list[tuple[dict, str]] = []
        for status, answer in op.get("responses", {}).items():
            if not str(status).startswith("2"):
                continue
            answer, adoc = resolve(answer, docs, "openapi.yaml")
            for media in answer.get("content", {}).values():
                candidates += schema_candidates(media.get("schema", {}), docs, adoc)
        problems += shape_problems(op_id, response, candidates, structs, docs)
    return problems


def format_ops(ops: set[tuple[str, str]]) -> list[str]:
    return [f"{method} {path}" for method, path in sorted(ops, key=lambda t: (t[1], t[0]))]


def main() -> int:
    failures: list[tuple[str, list[str]]] = []

    errors_text = ERRORS_MD.read_text(encoding="utf-8")
    spec = yaml.safe_load(OPENAPI_YAML.read_text(encoding="utf-8"))
    designed = yaml.safe_load(DESIGNED_YAML.read_text(encoding="utf-8"))

    family_codes, family_problems = errors_md_family_codes(errors_text)
    index_codes, index_problems = errors_md_status_index(errors_text)
    enum_codes = openapi_error_codes(spec)
    served_ops = openapi_ops(spec)
    designed_ops = openapi_ops(designed)
    spec_ops = served_ops | designed_ops

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
        ("openapi.yaml path+method ops", len(served_ops)),
        ("openapi-designed.yaml path+method ops", len(designed_ops)),
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

    in_both = served_ops & designed_ops
    if in_both:
        failures.append(
            ("paths declared in both openapi.yaml and openapi-designed.yaml", format_ops(in_both))
        )
    for name, document in (("openapi.yaml", spec), ("openapi-designed.yaml", designed)):
        nulls = null_keys(document)
        if nulls:
            failures.append((f"{name}: keys with a null value (an unquoted ', ' split a mapping?)", nulls))

    field_problems = designed_field_problems(spec, designed)
    if field_problems:
        failures.append((f"{DESIGNED_FIELDS} disagrees with the served operations", field_problems))

    read_problems = memory_read_problems(spec)
    if read_problems:
        failures.append(("a memory field read that the served Memory schema does not declare",
                         read_problems))

    retired = retired_verb_problems()
    if retired:
        failures.append(("a reader page runs a CLI verb ADR-0039 d15 retired", retired))

    pattern_problems = retired_name_pattern_problems()
    if pattern_problems:
        failures.append(("the v0.3.0 spelling pattern misreads a probe line", pattern_problems))

    retired_names = retired_name_problems()
    if retired_names:
        failures.append(("a start page names a v0.3.0 spelling ADR-0039 d13 breaks", retired_names))

    catalog_problems = mcp_catalog_problems()
    if catalog_problems:
        failures.append(("the MCP catalog disagrees with the served tools/list", catalog_problems))

    sdk_problems = rust_sdk_problems(spec)
    if sdk_problems:
        failures.append(("the Rust SDK's operation table disagrees with openapi.yaml", sdk_problems))

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
    print(f"  path+method operations agreeing: {len(spec_ops)} "
          f"({len(served_ops)} served, {len(designed_ops)} designed)")
    print("  MCP tools: mcp-tools.json is the served tools/list, mcp-tools.designed.json disjoint from it")
    print("  the Rust SDK's operations agree with openapi.yaml in method, path, fields and response shape")
    return 0


if __name__ == "__main__":
    sys.exit(main())
