#!/usr/bin/env python3
"""Scatter-gather of the learned sparse leg on one PostgreSQL, a research prototype (no served
path): is one statement over every readable workarea slower than one statement per workarea,
or per group of workareas, run in parallel on a pool of connections and merged by score?

    python bench/scale/fanout.py --dsn "host=DIR port=PORT user=bench dbname=postgres" \
        --cache DIR --out bench/scale/results/fanout.json

Needs psycopg 3 (a scratch venv, as sparse.py). The postings are the real doc-v3-distill
encodings of the 5,383 texts `eval/recall/mapreduce.py encode` wrote (SciFact and the recall
corpus), reused by digest as `sparse.py --reuse` does, so 1,000, 10,000 and 100,000 memories
carry the model's real token distribution; the queries are SciFact's 300 test queries' terms,
weighted by idf.json. Each size is one unlogged table shaped as `search_postings_served` for
kind 2 (token, workarea, memory, weight), its rows assigned uniformly at random to 5, 50 and
500 workareas, one covering index per assignment `(token_id, wa_N) INCLUDE (memory_id,
weight)`, built, measured and dropped in turn. The statement is `oml-pg`'s `sparse_sql` without row security, held rows, anchors
or expiry: the postings of the query's tokens on the readable workareas, scored
`sum(idf * weight)`, best `LIMIT` by score then id.

Modes, each the warm second run of each query, every mode on each query in turn with the order
rotated per query, p50 / p95 ms of the whole recall:
- `single`: one statement over every workarea (today's shape);
- `single-parallel`: the same with PostgreSQL's parallel query allowed (4 workers per gather,
  costs zeroed so the planner may take it);
- `fanout-each`: one statement per workarea on a pool of `--pool` connections, merged by score;
- `fanout-groups-P`: the workareas cut into P groups, one statement per group, P in 2, 4, 8.

`--throughput` instead runs 1, 2, 4 and 8 concurrent clients at 100,000 memories and 50
workareas, each client every query once per cell, under `single` and `fanout-groups-2`, `-4` and
`-8`, for `--rounds` rounds with the mode order rotated per round: each cell's median recalls a
second, p50 and p95 and their range, so the fan-out's extra backends are paid for under load.

Every mode returns the same top `LIMIT` (checked per query): the leg's score is a function of
the row and the query alone, so the merge is exact.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import statistics
import threading
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import psycopg

SEED = 20261111
SIZES = [1_000, 10_000, 100_000]
WORKAREAS = [5, 50, 500]
GROUPS = [2, 4, 8]

PREPARE = [5]   # psycopg's default: a statement run 5 times on a connection is prepared

PARALLEL = ["SET max_parallel_workers_per_gather = 4", "SET parallel_setup_cost = 0",
            "SET parallel_tuple_cost = 0", "SET min_parallel_index_scan_size = 0",
            "SET min_parallel_table_scan_size = 0"]

STATEMENT = """
WITH q AS (SELECT * FROM unnest(%(t)s::int8[], %(w)s::float8[]) AS q(token_id, w))
SELECT p.memory_id, sum(q.w * p.weight::float8)::float4 AS score
FROM {table} p JOIN q ON q.token_id = p.token_id
WHERE p.token_id = ANY(%(t)s::int8[]) AND p.{col} = ANY(%(wa)s::int4[])
GROUP BY p.memory_id
ORDER BY score DESC, p.memory_id ASC
LIMIT %(k)s
"""


def load_cache(cache):
    def rd(name):
        with open(Path(cache) / name) as f:
            return {r["id"]: r["terms"] for r in map(json.loads, f)}
    return rd("docs.jsonl"), rd("queries.jsonl")


def build(conn, size, docs):
    table = f"postings_{size}"
    rng = random.Random(SEED + size)
    ids = sorted(docs)
    with conn.cursor() as cur:
        cur.execute(f"DROP TABLE IF EXISTS {table}")
        cur.execute(f"CREATE UNLOGGED TABLE {table} (token_id int8, memory_id int8, weight float4, "
                    + ", ".join(f"wa_{n} int4" for n in WORKAREAS) + ")")
        t0 = time.perf_counter()
        with cur.copy(f"COPY {table} (token_id, memory_id, weight, "
                      + ", ".join(f"wa_{n}" for n in WORKAREAS) + ") FROM STDIN") as cp:
            for m in range(size):
                was = [rng.randrange(n) for n in WORKAREAS]
                for tok, w, _ in docs[ids[m % len(ids)]]:
                    cp.write_row((tok, m, w, *was))
        load_s = time.perf_counter() - t0
        conn.commit()
        conn.autocommit = True
        cur.execute(f"VACUUM ANALYZE {table}")
        conn.autocommit = False
        cur.execute(f"SELECT count(*), pg_relation_size('{table}') FROM {table}")
        rows, heap = cur.fetchone()
    conn.commit()
    return {"postings": rows, "heap_mib": round(heap / 2**20, 1), "load_s": round(load_s, 1)}


def index(conn, table, n):
    """One assignment's covering index at a time, so the disk holds one; returns its MiB."""
    with conn.cursor() as cur:
        cur.execute(f"CREATE INDEX {table}_wa{n} ON {table} (token_id, wa_{n}) INCLUDE (memory_id, weight)")
        conn.commit()
        conn.autocommit = True
        cur.execute(f"VACUUM ANALYZE {table}")
        conn.autocommit = False
        cur.execute(f"SELECT pg_relation_size('{table}_wa{n}')")
        size = cur.fetchone()[0]
    conn.commit()
    return round(size / 2**20, 1)


def drop_index(conn, table, n):
    with conn.cursor() as cur:
        cur.execute(f"DROP INDEX {table}_wa{n}")
    conn.commit()


def run_one(conn, table, col, q, wa, k):
    with conn.cursor() as cur:
        cur.execute(STATEMENT.format(table=table, col=col), {"t": q[0], "w": q[1], "wa": wa, "k": k})
        return cur.fetchall()


def merged(lists, k):
    return sorted((r for lst in lists for r in lst), key=lambda r: (-r[1], r[0]))[:k]


class Pool:
    def __init__(self, dsn, n):
        self.conns = [psycopg.connect(dsn, autocommit=True, prepare_threshold=PREPARE[0]) for _ in range(n)]
        self.free = list(self.conns)
        self.lock = threading.Lock()
        self.ex = ThreadPoolExecutor(n)

    def take(self):
        with self.lock:
            return self.free.pop()

    def give(self, c):
        with self.lock:
            self.free.append(c)

    def map(self, fn, items):
        def job(item):
            c = self.take()
            try:
                return fn(c, item)
            finally:
                self.give(c)
        return list(self.ex.map(job, items))

    def close(self):
        self.ex.shutdown()
        for c in self.conns:
            c.close()


def pct(xs, p):
    xs = sorted(xs)
    return round(xs[min(len(xs) - 1, int(round(p * (len(xs) - 1))))], 2)


def throughput(a, docs, qs, size, n, clients_list, modes, rounds):
    """Concurrent clients, each running every query once per cell: recalls a second, p50 and
    p95 per (clients, mode). Every round runs every cell, the mode order rotated per round and
    the client counts in turn, so the cells share the host's drift; the report is each cell's
    median over the rounds and its range."""
    conn = psycopg.connect(a.dsn, autocommit=False)
    info = build(conn, size, docs)
    table = f"postings_{size}"
    info["index_mib"] = index(conn, table, n)
    col, all_wa = f"wa_{n}", list(range(n))
    out = {"size": size, "workareas": n, "rounds": rounds, "table": info, "cells": {}, "runs": []}
    groups_of = {m: 1 if m == "single" else int(m.rsplit("-", 1)[1]) for m in modes}
    pools = {(c, m): [Pool(a.dsn, groups_of[m]) for _ in range(c)] for c in clients_list for m in modes}

    def recall(pool, groups, q):
        parts = [all_wa[i::groups] for i in range(groups)]
        return merged(pool.map(lambda c, wa: run_one(c, table, col, q, wa, a.k), parts), a.k)

    for (c, m), ps in pools.items():          # warm every connection's plan and the cache
        for p in ps:
            for q in qs[:20]:
                recall(p, groups_of[m], q)
    for r in range(rounds):
        for clients in clients_list:
            order = modes[r % len(modes):] + modes[:r % len(modes)]
            for mode in order:
                ps, groups = pools[(clients, mode)], groups_of[mode]
                lat, lock = [], threading.Lock()

                def client(i):
                    for q in qs[i % len(qs):] + qs[:i % len(qs)]:
                        t0 = time.perf_counter()
                        recall(ps[i], groups, q)
                        with lock:
                            lat.append((time.perf_counter() - t0) * 1000)

                load0 = os.getloadavg()[0]
                t0 = time.perf_counter()
                threads = [threading.Thread(target=client, args=(i,)) for i in range(clients)]
                for t in threads:
                    t.start()
                for t in threads:
                    t.join()
                wall = time.perf_counter() - t0
                row = {"round": r, "clients": clients, "mode": mode,
                       "recalls_per_s": round(len(lat) / wall, 1), "p50": pct(lat, 0.5),
                       "p95": pct(lat, 0.95), "load_avg_1m": round(load0, 2)}
                out["runs"].append(row)
                print(json.dumps(row), flush=True)
    for clients in clients_list:
        for mode in modes:
            rs = [x for x in out["runs"] if x["clients"] == clients and x["mode"] == mode]
            out["cells"][f"{clients}/{mode}"] = {
                k: {"median": statistics.median(x[k] for x in rs), "min": min(x[k] for x in rs),
                    "max": max(x[k] for x in rs)} for k in ("recalls_per_s", "p50", "p95")}
    for ps in pools.values():
        for p in ps:
            p.close()
    with conn.cursor() as cur:
        cur.execute(f"DROP TABLE {table}")
    conn.commit()
    conn.close()
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dsn", required=True)
    ap.add_argument("--cache", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--pool", type=int, default=8)
    ap.add_argument("--queries", type=int, default=100)
    ap.add_argument("--k", type=int, default=100, help="each list's depth; 100 is CC_DEPTH")
    ap.add_argument("--sizes", type=int, nargs="*", default=SIZES)
    ap.add_argument("--rounds", type=int, default=5)
    ap.add_argument("--no-prepare", action="store_true",
                    help="never prepare: every statement planned for its own parameters")
    ap.add_argument("--workareas", type=int, nargs="*", default=WORKAREAS)
    ap.add_argument("--modes", nargs="*", default=None, help="only these modes in the matrix")
    ap.add_argument("--throughput", action="store_true",
                    help="instead: concurrent clients at 100,000 memories and 50 workareas")
    a = ap.parse_args()
    if a.no_prepare:
        PREPARE[0] = None
    docs, enc_q = load_cache(a.cache)
    qs = [([t for t, _, _ in v], [w for _, w, _ in v]) for i, v in sorted(enc_q.items()) if i.startswith("sf-")]
    qs = qs[: a.queries]
    load_before = os.getloadavg()
    if a.throughput:
        res = throughput(a, docs, qs, 100_000, 50, [1, 2, 4, 8],
                         ["single", "fanout-groups-2", "fanout-groups-4", "fanout-groups-8"], a.rounds)
        res["load_avg_before"] = load_before
        res["load_avg_after"] = os.getloadavg()
        res.update({"k": a.k, "queries": len(qs), "cpus": os.cpu_count()})
        Path(a.out).write_text(json.dumps(res, indent=1) + "\n")
        return
    report = {"k": a.k, "pool": a.pool, "queries": len(qs), "cpus": os.cpu_count(), "sizes": {}}
    main_conn = psycopg.connect(a.dsn, autocommit=False)
    pool = Pool(a.dsn, a.pool)
    for size in a.sizes:
        info = build(main_conn, size, docs)
        table = f"postings_{size}"
        info["load_avg_before"] = os.getloadavg()
        info["workareas"] = {}
        info["index_mib"] = {}
        for n in a.workareas:
            info["index_mib"][str(n)] = index(main_conn, table, n)
            col = f"wa_{n}"
            all_wa = list(range(n))
            modes = {}

            def single(q, parallel=False):
                c = pool.take()
                try:
                    if parallel:
                        for s in PARALLEL:
                            c.execute(s)
                    try:
                        return run_one(c, table, col, q, all_wa, a.k)
                    finally:
                        if parallel:
                            c.execute("RESET ALL")
                finally:
                    pool.give(c)

            def fan(q, groups):
                parts = [all_wa[i::groups] for i in range(groups)]
                return merged(pool.map(lambda c, wa: run_one(c, table, col, q, wa, a.k), parts), a.k)

            runs = {"single": lambda q: single(q),
                    "single-parallel": lambda q: single(q, True),
                    "fanout-each": lambda q: fan(q, n)}
            for g in GROUPS:
                if g < n:
                    runs[f"fanout-groups-{g}"] = (lambda g: lambda q: fan(q, g))(g)
            if a.modes:
                runs = {m: f for m, f in runs.items() if m in a.modes}
            # every mode on each query in turn, the order rotated per query, so each mode
            # meets the same host load and the same cache; each timed run is the second
            names = list(runs)
            times = {m: [] for m in names}
            equal = Counter()
            for i, q in enumerate(qs):
                ref = single(q)
                for name in names[i % len(names):] + names[:i % len(names)]:
                    runs[name](q)
                    t0 = time.perf_counter()
                    got = runs[name](q)
                    times[name].append((time.perf_counter() - t0) * 1000)
                    equal[name] += [r[0] for r in got] == [r[0] for r in ref]
            for name in names:
                modes[name] = {"p50": pct(times[name], 0.5), "p95": pct(times[name], 0.95),
                               "same_list": equal[name] / len(qs)}
            # did the planner take a parallel plan?
            with main_conn.cursor() as cur:
                for s in PARALLEL:
                    cur.execute(s)
                cur.execute("EXPLAIN " + STATEMENT.format(table=table, col=col),
                            {"t": qs[0][0], "w": qs[0][1], "wa": all_wa, "k": a.k})
                plan = "\n".join(r[0] for r in cur.fetchall())
                cur.execute("RESET ALL")
            main_conn.rollback()
            if "single-parallel" in modes:
                modes["single-parallel"]["plan_has_gather"] = "Gather" in plan
            info["workareas"][str(n)] = modes
            drop_index(main_conn, table, n)
            print(size, n, json.dumps(modes), flush=True)
        info["load_avg_after"] = os.getloadavg()
        report["sizes"][str(size)] = info
        with main_conn.cursor() as cur:
            cur.execute(f"DROP TABLE {table}")
        main_conn.commit()
    pool.close()
    Path(a.out).write_text(json.dumps(report, indent=1) + "\n")


if __name__ == "__main__":
    main()
