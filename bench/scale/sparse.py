#!/usr/bin/env python3
"""The learned sparse lane of the scale bench (ADR-0052 d16, FR-100, FR-101):
`sparse` and `bm25-sparse`'s two lists beside `bm25-postings`, at a given
number of memories per tenant, through the adapter's own statements, as the
serving role under row security.

    python bench/scale/sparse.py --rows 100000 --model-dir DIR \
        --out bench/scale/results/sparse-100k.json

As postings.py, plus: migration 0022 applied and timed (the queue of every
existing version); every version with postings encoded by the real model
(`sparse_encode`, oml-rerank's example: SPARSE_ENCODE names its path), its
throughput recorded, and its kind 2 rows written with the version's state, as
the encoder's write does, then the queue emptied, as the encoder leaves it;
each query's tokens encoded the same way; then the 330 queries through
`search_scale` (PgStore::retrieve, warm) for `bm25_postings` at k 10,
`sparse` at k 10, and the two lists `bm25-sparse` reads, each at its fusion
depth of 100, whose sum per query is the pipeline's retrieval time (the core
reads them one after the other and fuses them in microseconds). The load
average is read before and after each run.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import time
import uuid
from pathlib import Path

import psycopg
from psycopg import sql as S

import postings as P

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ENCODE = os.environ.get("SPARSE_ENCODE", str(REPO / "target/release/examples/sparse_encode"))


def encode(model_dir: str, threads: int, mode: str, texts: list[str]) -> tuple[list, dict]:
    stdin = "".join(json.dumps({"text": t}) + "\n" for t in texts)
    res = subprocess.run([ENCODE, model_dir, str(threads), mode], input=stdin,
                         capture_output=True, text=True, check=True)
    rows = [json.loads(line) for line in res.stdout.splitlines()]
    return rows, json.loads(res.stderr.strip().splitlines()[-1])


def load_average() -> list[float]:
    return [float(x) for x in Path("/proc/loadavg").read_text().split()[:3]]


def apply_0022(db: str, owner: str, serving: str) -> float:
    with psycopg.connect(P.dsn(owner, db), autocommit=True) as c:
        c.execute(S.SQL("SET oml.serving_role = {}").format(S.Literal(serving)))
        t0 = time.perf_counter()
        with c.transaction():
            c.execute((REPO / "migrations" / "0022_search_sparse_queue.sql").read_text())
        return time.perf_counter() - t0


def write_kind2(db: str, owner: str, t: uuid.UUID, model_dir: str, threads: int,
                cache: Path | None) -> dict:
    with P.su(db) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        versions = c.execute(
            "SELECT m.id, m.workarea_id, oml_search_memory_kind(m.kind), "
            "oml_search_state(m.status, m.is_tip, m.retirement, m.kind), m.claim "
            "FROM search_sparse_queue q JOIN memories m ON m.tenant_id = q.tenant_id "
            "AND m.id = q.memory_id WHERE q.tenant_id = %s", (t,)).fetchall()
    known = {}
    if cache and cache.exists():
        known = {r["text"]: r["postings"] for r in map(json.loads, cache.read_text().splitlines())}
    todo = sorted({v[4] for v in versions} - known.keys())
    stats = {"versions": len(versions), "distinct_texts": len({v[4] for v in versions}),
             "encoded_now": len(todo)}
    if todo:
        la0 = load_average()
        rows, cost = encode(model_dir, threads, "docs", todo)
        stats["encode"] = {**cost, "load_before": la0, "load_after": load_average()}
        for text, row in zip(todo, rows):
            known[text] = row
        if cache:
            with cache.open("a") as f:
                for text in todo:
                    f.write(json.dumps({"text": text, "postings": known[text]}) + "\n")
    part = "search_postings_t_" + t.hex
    n = 0
    with psycopg.connect(P.dsn(owner, db), autocommit=True) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        t0 = time.perf_counter()
        # INSERT, not COPY: COPY FROM is refused under forced row security.
        batch = []

        def flush():
            if batch:
                cols = list(zip(*batch))
                c.execute(f"INSERT INTO {part} (tenant_id, memory_id, workarea_id, token_id, "
                          "weight, doc_len, kind, memory_kind, state) SELECT %s, m, w, t, x, d, 2, "
                          "k, s FROM unnest(%s::uuid[], %s::uuid[], %s::int8[], %s::float4[], "
                          "%s::int4[], %s::int2[], %s::int2[]) AS u(m, w, t, x, d, k, s)",
                          (t, *[list(col) for col in cols]))
                batch.clear()
        for mid, wa, mk, state, text in versions:
            for tok, w, in_text in known[text]:
                batch.append((mid, wa, tok, w, 1 if in_text else 0, mk, state))
                n += 1
            if len(batch) >= 50000:
                flush()
        flush()
        c.execute("DELETE FROM search_sparse_queue WHERE tenant_id = %s", (t,))
        c.execute(f"VACUUM ANALYZE {part}")
        stats["copy_s"] = round(time.perf_counter() - t0, 1)
    stats["kind2_postings"] = n
    stats["per_version"] = round(n / max(1, len(versions)), 1)
    return stats


def summarize(items, got) -> dict:
    out = {"all": {"p50": P.pct([g["ms"] for g in got], 0.5),
                   "p95": P.pct([g["ms"] for g in got], 0.95)}}
    for scope in ("leaf", "project", "universe"):
        ms = [g["ms"] for g, it in zip(got, items) if it["scope"] == scope]
        out[scope] = {"n": len(ms), "p50": P.pct(ms, 0.5), "p95": P.pct(ms, 0.95)}
    out["empty_share"] = round(sum(not g["ids"] for g in got) / len(got), 4)
    return out


def timed(url, items, retriever) -> tuple[list, dict]:
    la0 = load_average()
    got = P.drive(url, items, retriever)
    return got, {"load_before": la0, "load_after": load_average()}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--rows", type=int, required=True)
    ap.add_argument("--seed", type=int, default=20261105)
    ap.add_argument("--model-dir", required=True)
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("--cache", default=None, help="a JSONL cache of encoded texts")
    ap.add_argument("--out", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    tag = uuid.uuid4().hex[:8]
    db, owner, serving = f"scale_{tag}", f"scale_{tag}_owner", f"scale_{tag}_serving"
    with P.su() as c:
        c.execute(S.SQL("CREATE ROLE {} LOGIN").format(S.Identifier(owner)))
        c.execute(S.SQL("CREATE ROLE {} LOGIN").format(S.Identifier(serving)))
        c.execute(S.SQL("CREATE DATABASE {} OWNER {}").format(S.Identifier(db), S.Identifier(owner)))
    result = {"rows": a.rows, "seed": a.seed, "date": time.strftime("%Y-%m-%d"),
              "postgres": P.su().execute("SHOW server_version").fetchone()[0],
              "host": {"cpus": os.cpu_count()}}
    try:
        P.migrate(db, owner, serving, 20)
        t, tree, keys, load_s = P.load(db, owner, a.rows, a.seed)
        result["load_s"] = round(load_s, 1)
        result["fill_0021_s"] = round(P.apply_0021(db, owner, serving), 2)
        result["fill_0022_s"] = round(apply_0022(db, owner, serving), 2)
        result["kind2"] = write_kind2(db, owner, t, a.model_dir, a.threads,
                                      Path(a.cache) if a.cache else None)
        result["space"] = P.sizes(db, owner, t)
        url = f"postgres://{serving}@{P.HOST}:{P.PORT}/{db}"
        items = P.workload(t, tree, a.rows, a.seed)
        terms, _ = encode(a.model_dir, 1, "queries", [it["query"] for it in items])
        for it, q in zip(items, terms):
            it["sparse"] = q
        with P.su(db) as c:
            c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
            result["readable_rows"] = {
                scope: round(statistics.mean(
                    c.execute("SELECT COALESCE(sum(rows), 0) FROM search_workarea_rows "
                              "WHERE workarea_id = ANY(%s::uuid[])", (it["nodes"],)).fetchone()[0]
                    for it in items if it["scope"] == scope))
                for scope in ("leaf", "project", "universe")}
            result["query_tokens"] = {
                "mean": round(statistics.mean(len(it["sparse"]) for it in items), 1),
                "max": max(len(it["sparse"]) for it in items)}
        runs = {}
        got, la = timed(url, items, "bm25_postings")
        runs["bm25_postings_k10"] = {**summarize(items, got), **la}
        got, la = timed(url, items, "sparse")
        runs["sparse_k10"] = {**summarize(items, got), **la}
        deep = [{**it, "k": 100} for it in items]
        b, la_b = timed(url, deep, "bm25_postings")
        s, la_s = timed(url, deep, "sparse")
        runs["bm25_postings_k100"] = {**summarize(deep, b), **la_b}
        runs["sparse_k100"] = {**summarize(deep, s), **la_s}
        both = [{**x, "ms": x["ms"] + y["ms"], "ids": x["ids"] + y["ids"]} for x, y in zip(b, s)]
        runs["bm25_sparse_lists"] = summarize(deep, both)
        result["runs"] = runs
    finally:
        Path(a.out).parent.mkdir(parents=True, exist_ok=True)
        Path(a.out).write_text(json.dumps(result, indent=1) + "\n")
        if not a.keep:
            with P.su() as c:
                c.execute(S.SQL("DROP DATABASE IF EXISTS {} WITH (FORCE)").format(S.Identifier(db)))
                c.execute(S.SQL("DROP ROLE IF EXISTS {}").format(S.Identifier(owner)))
                c.execute(S.SQL("DROP ROLE IF EXISTS {}").format(S.Identifier(serving)))
    print(json.dumps({k: result.get(k) for k in ("rows", "fill_0022_s", "readable_rows")}))


if __name__ == "__main__":
    main()
