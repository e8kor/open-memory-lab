#!/usr/bin/env python3
"""Run the recall eval against a live OML server. Standard library only.

    python3 eval/recall/run.py --url http://127.0.0.1:8437 \
        --writer-key-file W --reviewer-key-file R --out eval/recall/results

1. load: writes every corpus memory with the writer (agent) key through
   POST /v1/memories, then approves each held row with the reviewer key
   (a different principal, so R2 holds) through POST /v2/review/next and
   /v2/review/verdicts until the queue is empty. The claim_key -> artifact_id
   map is kept in <out>/idmap.json so a second run skips the load.
2. query: every query goes through the tool the oml plugin calls, oml_read in
   recall mode over POST /mcp (JSON-RPC tools/call), with the writer key, a
   fixed k and max_tokens, and the hits are mapped back to corpus ids.
3. score: metrics.py, written to <out>/oml.json.

Keys are read from files and never printed.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402

CLAIM_PREFIX = "recall-eval-"

# --keywords drops these, as an agent following the server's instructions
# ("pass 2-4 exact keywords") would; the list is baselines.py's plus
# contractions.
STOP = set("""a about above after again against all am an and any are as at be because been before
being below between both but by can could did do does doing don't down during each few for from
further had has have having he her here hers him his how i if in into is it its itself just me more
most my no nor not of off on once only or other our ours out over own same she should so some such
than that the their them then there these they this those through to too under until up very was we
were what when where which while who whom why will with would you your yours""".split())


def keywords(query):
    """The query minus stop words and trailing punctuation, identifiers kept whole."""
    words = [w.strip("?,!") for w in query.split()]
    return " ".join(w for w in words if w and w.lower() not in STOP)


class Client:
    def __init__(self, url, key):
        self.url = url.rstrip("/")
        self.key = key

    def call(self, method, path, body=None, idempotent=False):
        headers = {
            "Authorization": "Bearer " + self.key,
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
        if idempotent:
            headers["Idempotency-Key"] = str(uuid.uuid4())
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.url + path, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                raw = resp.read().decode()
                return resp.status, (json.loads(raw) if raw else None)
        except urllib.error.HTTPError as e:
            raw = e.read().decode()
            try:
                return e.code, json.loads(raw)
            except ValueError:
                return e.code, raw

    def recall(self, query, k, max_tokens, workarea):
        args = {"query": query, "k": k, "max_tokens": max_tokens}
        if workarea:
            args["workarea"] = workarea
        body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "oml_read", "arguments": args}}
        t0 = time.perf_counter()
        status, resp = self.call("POST", "/mcp", body)
        ms = (time.perf_counter() - t0) * 1000
        if status != 200 or "result" not in resp or resp["result"].get("isError"):
            raise SystemExit(f"oml_read failed for {query!r}: {status} {resp}")
        return resp["result"]["structuredContent"], ms


def own_rows(writer):
    """artifact_id -> corpus id for every eval row the writer already wrote."""
    found, cursor = {}, None
    while True:
        path = "/v1/memories?own=true&limit=200" + (f"&cursor={cursor}" if cursor else "")
        status, resp = writer.call("GET", path)
        if status != 200:
            raise SystemExit(f"own listing: {status} {resp}")
        for row in resp["items"]:
            if row["claim_key"].startswith(CLAIM_PREFIX):
                found[row["artifact_id"]] = row["claim_key"][len(CLAIM_PREFIX):]
        cursor = resp.get("next_cursor")
        if not cursor:
            return found


def load(writer, reviewer, memories, workarea):
    idmap = own_rows(writer)
    have = set(idmap.values())
    for m in memories:
        if m["id"] in have:
            continue
        body = {"body": m["text"], "claim_key": CLAIM_PREFIX + m["id"], "kind": "memory"}
        if workarea:
            body["workarea"] = workarea
        status, resp = writer.call("POST", "/v1/memories", body, idempotent=True)
        if status != 202:
            raise SystemExit(f"write {m['id']} refused: {status} {resp}")
        idmap[resp["artifact_id"]] = m["id"]
    approved = 0
    while True:
        status, item = reviewer.call("POST", "/v2/review/next", {}, idempotent=True)
        if status == 204:
            break
        if status != 200:
            raise SystemExit(f"review next: {status} {item}")
        serve_id = item["serve_id"]
        status, v = reviewer.call(
            "POST", "/v2/review/verdicts",
            {"serve_id": serve_id, "verdict": "approve"}, idempotent=True)
        if status != 200 or v.get("state") != "promoted":
            raise SystemExit(f"approve: {status} {v}")
        approved += 1
    print(f"loaded {len(idmap)} memories, approved {approved}", file=sys.stderr)
    return idmap


def hit_id(hit):
    mem = hit.get("memory", hit)
    return mem["artifact_id"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", required=True)
    ap.add_argument("--writer-key-file", required=True)
    ap.add_argument("--reviewer-key-file", required=True)
    ap.add_argument("--workarea", default=None, help="default: the writer key's default workarea")
    ap.add_argument("--k", type=int, default=10)
    ap.add_argument("--max-tokens", type=int, default=2000)
    ap.add_argument("--out", required=True)
    ap.add_argument("--keywords", action="store_true",
                    help="send each query minus stop words; writes oml_keywords.json")
    a = ap.parse_args()

    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    writer = Client(a.url, Path(a.writer_key_file).read_text().strip())
    reviewer = Client(a.url, Path(a.reviewer_key_file).read_text().strip())
    memories, queries = metrics.load()

    idmap_path = out / "idmap.json"
    if idmap_path.exists():
        idmap = json.loads(idmap_path.read_text())
    else:
        idmap = load(writer, reviewer, memories, a.workarea)
        idmap_path.write_text(json.dumps(idmap, indent=1))

    writer.recall("warm up", a.k, a.max_tokens, a.workarea)
    per_query = {}
    for q in queries:
        sent = keywords(q["query"]) if a.keywords else q["query"]
        res, ms = writer.recall(sent, a.k, a.max_tokens, a.workarea)
        ranked = [idmap.get(hit_id(h), "?") for h in res.get("hits", [])]
        per_query[q["id"]] = {
            "sent": sent,
            "ranked": ranked,
            "scores_raw": [h.get("score") for h in res.get("hits", [])],
            "steering": len(res.get("steering", [])),
            "budget": res.get("budget"),
            "latency_ms": round(ms, 2),
        }
    summary = metrics.aggregate(per_query, queries)
    name = "oml_keywords" if a.keywords else "oml"
    result = {
        "method": name,
        "description": "OML recall as served: oml_read over /mcp, plainto_tsquery('simple') AND "
                       "semantics, ts_rank normalization 0"
                       + ("; query sent minus stop words" if a.keywords else ""),
        "params": {"k": a.k, "max_tokens": a.max_tokens},
        "summary": summary,
        "per_query": per_query,
    }
    (out / f"{name}.json").write_text(json.dumps(result, indent=1))
    print(json.dumps(summary["overall"]))


if __name__ == "__main__":
    main()
