#!/usr/bin/env python3
"""Write docs/api/mcp-plugin.md from docs/api/mcp-tools.json, the catalog the server serves.

The server includes that file at build time (`crates/transports/oml-transport-http/src/mcp.rs`),
so the page lists the tools, arguments and descriptions an agent receives from `tools/list`.

    python scripts/gen_mcp_reference.py [--check]
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import docgen  # noqa: E402

CATALOG = docgen.DOCS / "api" / "mcp-tools.json"
OUT = docgen.DOCS / "api" / "mcp-plugin.md"
HINTS = {
    "readOnlyHint": "read-only",
    "destructiveHint": "destructive",
    "idempotentHint": "idempotent",
    "openWorldHint": "open-world",
}


def cell(text: str) -> str:
    return docgen.strip_handles(" ".join(text.split())).replace("|", "\\|")


def type_of(schema: dict) -> str:
    if schema.get("enum"):
        return " \\| ".join(f"`{v}`" for v in schema["enum"])
    t = schema.get("type", "object")
    if t == "array":
        return f"array of {type_of(schema.get('items', {}))}"
    return t


def rows(schema: dict, prefix: str = "") -> list[str]:
    required = set(schema.get("required", []))
    out = []
    for name, p in schema.get("properties", {}).items():
        extra = f" Default `{p['default']}`." if "default" in p else ""
        out.append(f"| `{prefix}{name}` | {type_of(p)} | {'yes' if name in required else 'no'} | {cell(p.get('description', ''))}{extra} |")
        if p.get("type") == "object" and p.get("properties"):
            out += rows(p, prefix + name + ".")
    return out


# Prose for a decided change the catalog does not serve yet; removed when the build lands.
DESIGNED = (
    "**Designed, not built: search by kind and the catalog** "
    "([ADR-0092](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/"
    "ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md), "
    "planned for v0.8.0). The arguments below are what the server serves today. In v0.8.0 `kinds` takes every kind in recall and "
    "listing modes and replaces the listing's `kind`; a recall naming none returns hits of every kind the key reads, each marked "
    "with its kind, steering first and unranked unless `kinds` leaves it out; skill hits carry a name, description and resource URI "
    "and guardrail hits their text and rule, both data and never run or binding. Without a query, `oml_read` returns the catalog: "
    "`kinds`, `workspace`, `prefix` and `state` filter it, `include_text` adds bodies, and the first page carries per-kind counts. "
    "`own` then defaults to `false`, so an agent asks for its own work with `own: true`.\n"
)


# The prompts and resources the server serves beside the tools (ADR-0037 d11; ADR-0080 d5 to d7):
# not in the tool catalog, so written here.
PROMPTS = (
    "## Prompts and resources\n\n"
    "`prompts/list` names the `guardrails` prompt first, then each skill offered at the key's scope, 50 a page with MCP's "
    "`cursor` and `nextCursor`; Claude Code lists them as `/oml:<name> (MCP)`. Each skill prompt takes one optional argument, "
    "`input`. `prompts/get` of a skill answers one user message: a header line naming its workspace, version and approval, "
    "or beginning `UNREVIEWED` for a version no reviewer has approved, then the body, then a line `Input:` and the input when "
    "one is sent. A name not offered here answers `-32602`, \"no skill <name> is offered here\", whatever the reason.\n\n"
    "`resources/list` serves the same skills as resources at `oml://workspace/<workspace>/skill/<name>`, `text/markdown`; "
    "`resources/templates/list` serves that template; `resources/read` answers the header and body as one text content, with "
    "the artifact id, version, approval and `unreviewed` in `_meta.oml`. A URI segment holding `%` or a reserved character is "
    "`-32602`; any other URI not served, a skill shadowed out of reach and a document URI among them, answers `-32002`, "
    "\"Resource not found\". Serving a prompt or a resource is a read, not a recall. When the server cannot read the "
    "skills, page one of `prompts/list` holds the `guardrails` prompt alone, and `resources/list`, `prompts/get` and "
    "`resources/read` answer `-32603`, \"the skills could not be read; retry later\".\n"
)


def render(doc: dict) -> str:
    meta = doc["_meta"]
    tools = doc["tools"]
    names = ", ".join(f"`{t['name']}`" for t in tools)
    out = [
        docgen.front_matter("the MCP tools an agent receives, with every argument"),
        "# MCP tools\n",
        f"The server speaks MCP over streamable HTTP at `/mcp`, stateless, targeting protocol revision {meta['spec_revision_targeted']}. "
        f"It serves {len(tools)} tools: {names}. The tools write and read and include no verb that approves, signs, demotes or deletes ([R1](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md)).\n",
        f"*{docgen.BANNER.format(script='scripts/gen_mcp_reference.py')} Source: `docs/api/mcp-tools.json`, the file the server includes at build time.*\n",
        "A field outside the mode a call chose is refused with `request.validation`, naming the field in `details.field`. "
        "Errors use the [error codes](errors.md). Authenticate with a bearer key, or with an OAuth access token where connections are on.\n",
        DESIGNED,
    ]
    for t in tools:
        hints = [HINTS[k] if v else "not " + HINTS[k] for k, v in t.get("annotations", {}).items() if k in HINTS]
        out.append(f"## `{t['name']}`\n")
        if t.get("title"):
            out.append(f"Shown in clients as **{t['title']}**.\n")
        out.append(cell(t["description"]) + "\n")
        out.append("Annotations: " + ", ".join(hints) + ".\n")
        sch = t["inputSchema"]
        if sch.get("properties"):
            out.append("| Argument | Type | Required | Meaning |\n|---|---|---|---|")
            out += rows(sch)
        else:
            out.append("It takes no arguments.")
        out.append("")
    out.append(PROMPTS)
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="fail if the page differs from mcp-tools.json")
    args = ap.parse_args()
    return docgen.finish(OUT, docgen.mark_unreleased(render(json.loads(CATALOG.read_text())), "mcp"), args.check)


if __name__ == "__main__":
    sys.exit(main())
