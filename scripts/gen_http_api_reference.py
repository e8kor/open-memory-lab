#!/usr/bin/env python3
"""Write docs/api/http-api.md from docs/api/openapi.yaml, the contract the server routes.

Every operation the page lists is one the build serves; a route the server does not have cannot
appear here. Run with --check in CI so the page cannot drift from the contract.

    python scripts/gen_http_api_reference.py [--check]
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

import yaml

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import docgen  # noqa: E402

SPEC = docgen.DOCS / "api" / "openapi.yaml"
OUT = docgen.DOCS / "api" / "http-api.md"
COMMON = {"XRequestId", "XCorrelationId", "Traceparent"}
TAGS = {
    "memories": "Write, list, recall and replace memories on the provider ring.",
    "workareas": "The workarea tree and its status ladder.",
    "crews": "Named crew records.",
    "edges": "Links between memories and the places a memory holds in a workarea brief.",
    "events": "The per-tenant journal tail.",
    "review": "Serve held memories to a reviewer and record verdicts.",
    "keys": "Key issuance, the calling key's own record, connect codes and bootstrap.",
    "oauth": "OAuth endpoints for hosted MCP clients, served only where `oauth.issuer` is set.",
    "web": "The sign-in, user and admin pages, and the email allowlist.",
    "admin": "An administrator's reads of memory text and attribution.",
    "search": "The served search pipelines and the tenant default.",
    "usage": "Recall telemetry and the replay of labelled recalls.",
    "health": "Liveness and readiness.",
}


def sentences(text: str, n: int) -> str:
    text = docgen.strip_handles(" ".join(text.split()))
    parts = re.split(r"(?<=[.!?])\s+(?=[A-Z`])", text)
    return " ".join(parts[:n])


def cell(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ")


class Spec:
    def __init__(self, doc: dict):
        self.doc = doc

    def deref(self, node: dict) -> dict:
        while isinstance(node, dict) and "$ref" in node:
            cur = self.doc
            for part in node["$ref"].lstrip("#/").split("/"):
                cur = cur[part]
            node = cur
        return node

    def type_of(self, schema: dict) -> str:
        if "$ref" in schema:
            name = schema["$ref"].split("/")[-1]
            target = self.deref(schema)
            if target.get("enum"):
                return " \\| ".join(f"`{v}`" for v in target["enum"])
            if target.get("type") in ("string", "integer", "boolean", "number"):
                return target["type"]
            return "object" if target.get("type") == "object" else name
        if schema.get("enum"):
            return " \\| ".join(f"`{v}`" for v in schema["enum"])
        t = schema.get("type", "object")
        if t == "array":
            return f"array of {self.type_of(schema.get('items', {}))}"
        return t

    def props(self, schema: dict) -> list[tuple[str, str, bool, str]]:
        schema = self.deref(schema)
        required = set(schema.get("required", []))
        rows = []
        for name, p in schema.get("properties", {}).items():
            desc = p.get("description") or self.deref(p).get("description", "")
            rows.append((name, self.type_of(p), name in required, sentences(desc, 1)))
        return rows


def render(doc: dict) -> str:
    spec = Spec(doc)
    ops: dict[str, list[tuple[str, str, dict]]] = {}
    for path, item in doc["paths"].items():
        for method, op in item.items():
            if isinstance(op, dict) and "responses" in op:
                ops.setdefault((op.get("tags") or ["other"])[0], []).append((method.upper(), path, op))
    total = sum(len(v) for v in ops.values())
    out = [
        docgen.front_matter("every route the server serves, with its parameters, request fields and responses"),
        "# HTTP API\n",
        "OML serves one JSON API on one keyed listener. `/v1` is the provider ring that agents use: write, read, recall and replace. `/v2` is the lab ring for review, workareas, crews and keys. "
        "A request names its workarea in one field, `workarea`, by id or name; omitted, it takes the key's default workarea.\n",
        f"*{docgen.BANNER.format(script='scripts/gen_http_api_reference.py')} Source: `docs/api/openapi.yaml`. It lists {total} operations.*\n",
        "## Conventions\n",
        "- **Authentication.** Send `Authorization: Bearer <key>`. The key carries one role (`reader`, `writer`, `dreamer`, `admin`, `operator`); the server derives the tenant from it, never from the request.",
        "- **Idempotency.** Every `POST` that creates state needs an `Idempotency-Key` header. A replay returns the stored answer and sets `Idempotency-Replay: true`. A refused request records nothing, so a retry under the same key runs again and may answer differently.",
        "- **Request ids.** Every response carries `X-Request-Id` and `X-Correlation-Id`. The caller may send its own `X-Request-Id`, `X-Correlation-Id` and `traceparent`.",
        "- **Errors.** Every failure has one JSON shape with a `code` from the [error codes](errors.md). Any operation can answer `429 limit.rate` or `limit.quota`.",
        "- **Absence.** A read the key may not make answers `404 access.not_found`, the same answer as a missing id.",
        "- **Limits.** Body size, page size and rate bounds are on the [limits page](../reference/limits.md).\n",
        "## Operations by area\n",
        "| Area | Operations | What it covers |\n|---|---|---|",
    ]
    for tag in [t["name"] for t in doc.get("tags", [])] + [t for t in ops if t not in [x["name"] for x in doc.get("tags", [])]]:
        if tag in ops:
            out.append(f"| [{tag}](#{tag}) | {len(ops[tag])} | {TAGS.get(tag, '')} |")
    for tag in [t["name"] for t in doc.get("tags", [])] + [t for t in ops if t not in [x["name"] for x in doc.get("tags", [])]]:
        if tag not in ops:
            continue
        out.append(f"\n## {tag}\n")
        out.append(TAGS.get(tag, "") + "\n")
        out.append("| Operation | What it does |\n|---|---|")
        for method, path, op in ops[tag]:
            out.append(f"| `{method} {path}` | {cell(sentences(op.get('summary', ''), 1))} |")
        for method, path, op in ops[tag]:
            out.append(f"\n### `{method} {path}`\n")
            out.append(sentences(op.get("summary", ""), 1) + "\n")
            desc = op.get("description", "")
            req = re.match(r"(Requires:[^.]*(?:\.[^A-Z\n][^.]*)*\.)", desc)
            if req:
                out.append(sentences(req.group(1), 1) + "\n")
            params = []
            for p in op.get("parameters", []):
                name = p.get("$ref", "").split("/")[-1]
                if name in COMMON:
                    continue
                p = spec.deref(p)
                params.append((p["name"], p["in"], spec.type_of(p.get("schema", {})), p.get("required", False), sentences(p.get("description", ""), 1)))
            if params:
                out.append("| Parameter | In | Type | Required | Meaning |\n|---|---|---|---|---|")
                out += [f"| `{n}` | {i} | {t} | {'yes' if r else 'no'} | {cell(d)} |" for n, i, t, r, d in params]
                out.append("")
            body = op.get("requestBody", {}).get("content", {})
            for media, mt in body.items():
                schema = mt.get("schema")
                rows = spec.props(schema) if schema else []
                if rows:
                    out.append("**Request body** (`" + media + "`)\n")
                    out.append("| Field | Type | Required | Meaning |\n|---|---|---|---|")
                    out += [f"| `{n}` | {t} | {'yes' if r else 'no'} | {cell(d)} |" for n, t, r, d in rows]
                    out.append("")
                elif schema:
                    out.append(f"**Request body** (`{media}`): {spec.type_of(schema)}\n")
            resp = []
            for code, r in op["responses"].items():
                r = spec.deref(r)
                resp.append(f"`{code}` {sentences(r.get('description', ''), 1)}")
            out.append("**Responses.** " + "; ".join(resp) + "\n")
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="fail if the page differs from openapi.yaml")
    args = ap.parse_args()
    return docgen.finish(OUT, render(yaml.safe_load(SPEC.read_text())), args.check)


if __name__ == "__main__":
    sys.exit(main())
