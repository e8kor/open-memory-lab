#!/usr/bin/env python3
"""The prune constants' sweep on the external sets (ADR-0052 d6, open question 2).

    prune_sweep.py --state S --out DIR [--pair SHARE BUDGET]... <dataset>...

Reads a lead's benchmark database while boot.sh serves it (the corpus already
loaded by `bench.py load`), and asks every query of each dataset through the
PostgreSQL adapter's own `retrieve` (crates/adapters/oml-pg/examples/
search_scale.rs, as the serving role under row security, scoped to the
dataset's workarea as the recall's core scopes it) for `bm25` and for
`bm25_postings` at each pair of prune constants, then scores each ranked list
with ir_measures as bench.py does. It bypasses HTTP and the core, so its
latency is the store call's alone; the served numbers are bench.py's. Its one
purpose is to choose the constants: what each pair costs in nDCG@10 against
`bm25`, and how often the scan answers.

PGHOST, PGPORT and PGUSER name a superuser of boot.sh's cluster; SEARCH_SCALE
the driver binary (default target/release/examples/search_scale).
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from adapters import ADAPTERS, workarea  # noqa: E402
from bench import DEFAULT_CACHE, claim_key, score  # noqa: E402

REPO = HERE.parent.parent
DRIVER = os.environ.get("SEARCH_SCALE", str(REPO / "target/release/examples/search_scale"))
PAIRS = [("inf", str(2**63 - 1)), ("0.5", "50000"), ("0.3", "50000"), ("0.2", "50000"),
         ("0.1", "50000"), ("0.05", "50000"), ("0.1", "20000"), ("0.1", "10000"),
         ("0.1", "5000")]


def psql(db, sql):
    return subprocess.run(
        ["psql", "-h", os.environ.get("PGHOST", "127.0.0.1"), "-p", os.environ.get("PGPORT", "5432"),
         "-U", os.environ.get("PGUSER", "postgres"), "-d", db, "-qtAX", "-c", sql],
        capture_output=True, text=True, check=True).stdout.strip()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--state", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--cache", default=DEFAULT_CACHE)
    ap.add_argument("--seed", type=int, default=20261104)
    ap.add_argument("--pair", nargs=2, action="append", metavar=("SHARE", "BUDGET"))
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()
    state = Path(a.state)
    db = (state / "database").read_text().strip()
    url = (f"postgres://{db}_serving@{os.environ.get('PGHOST', '127.0.0.1')}:"
           f"{os.environ.get('PGPORT', '5432')}/{db}")
    tenant = psql(db, "SELECT id FROM tenants WHERE name = 'bench'")
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    table = {}
    for ds in a.datasets:
        bench = ADAPTERS[ds]().load(a.cache, a.seed, None)
        idmap = json.loads((state / f"idmap-{ds}.json").read_text())
        back = {aid: ck for ck, aid in idmap.items()}
        doc_of = {claim_key(ds, d.id): d.id for d in bench.docs}
        nodes = {}
        for q in bench.queries:
            name = workarea(ds, q.group)
            if name not in nodes:
                nodes[name] = psql(db, f"SELECT id FROM workareas WHERE name = '{name}'")
        items = [{"tenant": tenant, "nodes": [nodes[workarea(ds, q.group)]], "query": q.text,
                  "k": 100} for q in bench.queries]
        work = out / f".workload-{ds}.json"
        work.write_text(json.dumps(items))
        rows = {}
        for label, retriever, pair in [("bm25", "bm25", None)] + [
                (f"postings {s} {b}", "bm25_postings", (s, b)) for s, b in (a.pair or PAIRS)]:
            args = [DRIVER, url, str(work), retriever] + (list(pair) if pair else [])
            lines = subprocess.run(args, capture_output=True, text=True, check=True).stdout
            got = [json.loads(line) for line in lines.splitlines() if line.strip()]
            runs = {q.id: {"ranked": [doc_of.get(back.get(i)) for i in g["ids"]]}
                    for q, g in zip(bench.queries, got)}
            per = score(bench, runs)
            ids = [q.id for q in bench.queries]
            ms = sorted(g["ms"] for g in got)
            rows[label] = {
                "ndcg@10": round(sum(per[i]["nDCG@10"] for i in ids) / len(ids), 4),
                "scan_share": round(sum(g["access"] == "scan" for g in got) / len(got), 4),
                "pruned_share": round(sum(bool(g["pruned"]) for g in got) / len(got), 4),
                "store_p50_ms": round(ms[len(ms) // 2], 2),
                "store_p95_ms": round(ms[min(len(ms) - 1, int(0.95 * len(ms)))], 2),
            }
            print(ds, label, rows[label], flush=True)
        work.unlink()
        table[ds] = rows
    (out / "prune_sweep.json").write_text(json.dumps(table, indent=1) + "\n")


if __name__ == "__main__":
    main()
