"""Two cases added after the main runs, on the same schema, roles and fences.

1. exact: an exact vector scan of the tenant's partition under row security
   (halfvec 384, no index used) against HNSW with and without pgvector 0.8's
   iterative scan, under selective scopes (a key reading ~10% and ~1% of the
   tenant's served rows): p50/p95 and recall@10 against the exact top 10.

       python extra.py exact --scale 10000 --out results/x_exact_10k.json

2. sparse: a learned-sparse inverted table (memory, token_id int, weight real),
   list-partitioned per tenant like memories and fenced by the same policy,
   ~60 non-zero tokens per memory with Zipf-distributed token ids over a
   30,522-token vocabulary (BERT's, as SPLADE and the OpenSearch
   inference-free models use) and ~10 weighted query tokens, scored as
   SUM(doc weight * query weight) GROUP BY memory over a btree on token_id.
   Cost only: the weights are random.

       python extra.py sparse --scale 100000 --out results/x_sparse_100k.json
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
import time
import uuid

import numpy as np
import psycopg

import gen
import run as R

K = 10
VOCAB = 30522


def scopes_by_fraction(r: random.Random, tree, wa_rows: dict, frac: float, count: int):
    """Readable sets that hold about `frac` of the served rows: random leaves
    added until the share is reached."""
    total = sum(wa_rows.values())
    leaves = tree.leaf_list()
    out = []
    for _ in range(count):
        r.shuffle(leaves)
        ws, got = [], 0
        for l in leaves:
            if got >= frac * total:
                break
            ws.append(str(l))
            got += wa_rows.get(l, 0)
        out.append(ws)
    return out


def timed(c, tenant, sqltext, params, setup=()):
    with c.transaction():
        c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
        for s in setup:
            c.execute(s)
        t0 = time.perf_counter()
        rows = c.execute(sqltext, params).fetchall()
        return (time.perf_counter() - t0) * 1000, [x[0] for x in rows]


def summ(lat):
    return {"n": len(lat), "p50_ms": R.pct(lat, 50), "p95_ms": R.pct(lat, 95), "p99_ms": R.pct(lat, 99),
            "mean_ms": statistics.fmean(lat)}


def exact_case(a):
    out = {"case": "exact", "scale": a.scale, "tenants": 1, "k": K, "type": "halfvec(384)",
           "started": time.strftime("%Y-%m-%dT%H:%M:%S"), "results": {}}
    db = f"oml_x_exact_{a.scale}"
    r = random.Random(a.scale * 7)
    R.create_db(db)
    t, tree, keys = R.setup_tenant(db, r, a.scale, "tenant0")
    info = R.load_tenant(db, t, tree, keys, a.scale, seed=a.scale, stats={})
    R.restore_fk(db)
    loaded = [{"tenant": t, "tree": tree, "keys": keys, **info}]
    R.stage_vector(db, loaded, out, halfvec=True)
    with R.su(db) as c:
        c.execute("VACUUM ANALYZE")
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        wa_rows = {uuid.UUID(str(w)): n for w, n in c.execute(
            "SELECT workarea_id, count(*) FROM memory_vectors WHERE served AND kind IN ('memory', 'derived_fact') "
            "GROUP BY 1").fetchall()}
    centers = R.embed_centers()
    rng = np.random.default_rng(3)
    sql = R.APPROACHES["f_hnsw"].replace("::vector", "::halfvec")
    variants = {
        "exact_scan": ["SET LOCAL enable_indexscan = off", "SET LOCAL enable_bitmapscan = off"],
        "hnsw_no_iterative": ["SET LOCAL hnsw.iterative_scan = off"],
        "hnsw_relaxed_order": ["SET LOCAL hnsw.iterative_scan = relaxed_order"],
    }
    with R.serving(db) as c:
        for frac in (1.0, 0.10, 0.01):
            scopes = scopes_by_fraction(r, tree, wa_rows, frac, a.queries)
            work = []
            for ws in scopes:
                v = centers[rng.integers(len(centers))] + rng.standard_normal(R.DIM) * 0.9
                v /= np.linalg.norm(v)
                work.append({"t": t, "ws": ws, "kinds": R.KINDS, "k": K, "vec": R.vec_text(v)})
            truth = []
            res = {}
            for name, setup in variants.items():
                # a distinct statement text per variant: a server-prepared
                # statement keeps the plan it was built with, so one text
                # under three settings would reuse one plan for all three
                vsql = sql + f" -- {name}"
                if name == "exact_scan":
                    # an ORDER BY expression the index cannot serve: a true
                    # scan of the readable rows (disabling index scans alone
                    # left PostgreSQL 18 on the HNSW plan, marked Disabled)
                    vsql = sql.replace("ORDER BY v.embedding <=> %(vec)s::halfvec",
                                       "ORDER BY (v.embedding <=> %(vec)s::halfvec) + 0") + " -- exact"
                for p in work[:30]:  # warm-up
                    timed(c, t, vsql, p, setup)
                lat, recalls, short = [], [], 0
                for i, p in enumerate(work):
                    ms, ids = timed(c, t, vsql, p, setup)
                    lat.append(ms)
                    if name == "exact_scan":
                        truth.append(ids)
                    else:
                        tr = truth[i]
                        if tr:
                            recalls.append(len(set(ids) & set(tr)) / len(tr))
                        if len(ids) < len(tr):
                            short += 1
                res[name] = {**summ(lat)}
                if name != "exact_scan":
                    res[name]["recall_at_10"] = statistics.fmean(recalls) if recalls else None
                    res[name]["short_results"] = short
            res["readable_share"] = frac
            plans = {}
            with psycopg.connect(R.dsn("oml_serving", db), cursor_factory=psycopg.ClientCursor) as ec:
                for name, setup in variants.items():
                    with ec.transaction():
                        ec.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                        ec.execute("SET LOCAL hnsw.ef_search = 40")
                        for st in setup:
                            ec.execute(st)
                        esql = sql if name != "exact_scan" else sql.replace(
                            "ORDER BY v.embedding <=> %(vec)s::halfvec", "ORDER BY (v.embedding <=> %(vec)s::halfvec) + 0")
                        plans[name] = "\n".join(x[0] for x in ec.execute(
                            "EXPLAIN (ANALYZE, COSTS OFF) " + esql, work[0]).fetchall())
            res["plans"] = plans
            out["results"][f"{int(frac * 100)}pct"] = res
            print(time.strftime("%H:%M:%S"), "exact", a.scale, frac,
                  {k: (round(v["p95_ms"], 1), v.get("recall_at_10")) for k, v in res.items() if isinstance(v, dict) and k != "plans"},
                  flush=True)
    with R.su() as c:
        c.execute(f"DROP DATABASE {db} WITH (FORCE)")
    return out


SPARSE_DDL = """
CREATE TABLE sparse_postings (tenant_id uuid NOT NULL, memory_id bigint NOT NULL, token_id integer NOT NULL,
    weight real NOT NULL, workarea_id uuid NOT NULL, served boolean NOT NULL) PARTITION BY LIST (tenant_id);
ALTER TABLE sparse_postings ENABLE ROW LEVEL SECURITY; ALTER TABLE sparse_postings FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON sparse_postings
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
GRANT SELECT, INSERT ON sparse_postings TO oml_serving;
"""

SPARSE_SQL = """SELECT p.memory_id, sum(p.weight * q.w) AS score
    FROM sparse_postings p
    JOIN unnest(%(tok)s::int[], %(qw)s::real[]) AS q(token_id, w) ON q.token_id = p.token_id
    WHERE p.tenant_id = %(t)s AND p.token_id = ANY(%(tok)s::int[]) AND p.served
      AND p.workarea_id = ANY(%(ws)s::uuid[])
    GROUP BY p.memory_id ORDER BY score DESC, p.memory_id LIMIT %(k)s"""


_CDF = np.cumsum(np.arange(1, VOCAB + 1, dtype=np.float64) ** -1.07)
_CDF /= _CDF[-1]


def zipf_tokens(rng, n):
    """Token ids by Zipf rank (exponent 1.07) over the vocabulary (id = rank)."""
    return np.searchsorted(_CDF, rng.random(n)).astype(np.int32)


def sparse_case(a):
    out = {"case": "sparse", "scale": a.scale, "tenants": 1, "k": K, "vocab": VOCAB, "nnz_per_doc": 60,
           "query_tokens": 10, "started": time.strftime("%Y-%m-%dT%H:%M:%S")}
    db = f"oml_x_sparse_{a.scale}"
    r = random.Random(a.scale * 11)
    rng = np.random.default_rng(a.scale)
    R.create_db(db)
    t, tree, keys = R.setup_tenant(db, r, a.scale, "tenant0")
    leaves = tree.leaf_list()
    lw = np.array([r.paretovariate(1.3) for _ in leaves])
    lw /= lw.sum()
    p = "sparse_postings_t_" + t.hex
    with R.owner(db) as c:
        c.execute(SPARSE_DDL)
        c.execute(f"CREATE TABLE {p} (LIKE sparse_postings INCLUDING DEFAULTS INCLUDING CONSTRAINTS)")
        t0 = time.perf_counter()
        nnz = 0
        with c.cursor().copy(f"COPY {p} (tenant_id, memory_id, token_id, weight, workarea_id, served) "
                             "FROM STDIN") as cp:
            batch = 2000
            for start in range(0, a.scale, batch):
                m = min(batch, a.scale - start)
                was = rng.choice(len(leaves), size=m, p=lw)
                served = rng.random(m) < 0.65
                for j in range(m):
                    toks = np.unique(zipf_tokens(rng, 75))[:60]
                    ws = rng.gamma(1.5, 0.6, size=len(toks)).astype(np.float32)
                    wa, sv, mid = leaves[was[j]], bool(served[j]), start + j
                    for tok, w in zip(toks.tolist(), ws.tolist()):
                        cp.write_row((t, mid, tok, w, wa, sv))
                    nnz += len(toks)
        out["load_s"] = time.perf_counter() - t0
        out["postings"] = nnz
        out["heap_bytes"] = R.rel_size(c, p)
        t0 = time.perf_counter()
        c.execute(f"CREATE INDEX {p}_token ON {p} (token_id)")
        out["index_build_s"] = time.perf_counter() - t0
        out["index_bytes"] = R.rel_size(c, f"{p}_token")
        c.execute(f"ALTER TABLE {p} ENABLE ROW LEVEL SECURITY")
        c.execute(f"ALTER TABLE {p} FORCE ROW LEVEL SECURITY")
        c.execute(f"CREATE POLICY tenant_isolation ON {p} "
                  "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) "
                  "WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)")
        c.execute(f"ALTER TABLE sparse_postings ATTACH PARTITION {p} FOR VALUES IN ('{t}')")
        c.execute(f"VACUUM ANALYZE {p}")
    # queries: 10 tokens by the same Zipf, weighted by an IDF-like lookup
    # (log of inverse rank frequency), as an inference-free query encoder does
    scopes = gen.readable_sets(r, tree, a.queries)
    work = []
    for sk, ws in scopes:
        toks = np.unique(zipf_tokens(rng, 14))[:10]
        qw = (np.log1p(toks.astype(np.float64)) + 0.5).astype(np.float32)
        work.append({"scope": sk, "p": {"t": t, "ws": ws, "tok": toks.tolist(), "qw": qw.tolist(), "k": K}})

    # the same queries with the 100 most frequent token ids dropped, the
    # stopword-like pruning sparse retrievers apply to queries
    pruned = []
    for w in work:
        keep = [(k, q) for k, q in zip(w["p"]["tok"], w["p"]["qw"]) if k >= 100] or [(w["p"]["tok"][-1], w["p"]["qw"][-1])]
        pruned.append({"scope": w["scope"], "p": {**w["p"], "tok": [k for k, _ in keep], "qw": [q for _, q in keep]}})

    def measure(label, setup=(), work=work):
        with R.serving(db) as c:
            for w in work[:30]:
                timed(c, t, SPARSE_SQL, w["p"], setup)
            lat, by = [], {}
            for w in work:
                ms, _ = timed(c, t, SPARSE_SQL, w["p"], setup)
                lat.append(ms)
                by.setdefault(w["scope"], []).append(ms)
        out[label] = {**summ(lat), "by_scope": {k: {"p50_ms": R.pct(v, 50), "p95_ms": R.pct(v, 95)}
                                                for k, v in by.items()}}
        print(time.strftime("%H:%M:%S"), "sparse", a.scale, label, round(out[label]["p50_ms"], 1),
              round(out[label]["p95_ms"], 1), flush=True)

    if not a.no_cold:
        R.restart(cold=True)
    with R.serving(db) as c:  # cold pass
        lat = [timed(c, t, SPARSE_SQL, w["p"])[0] for w in work]
    out["cold"] = summ(lat)
    measure("warm")
    measure("warm_pruned_top100", work=pruned)
    with psycopg.connect(R.dsn("oml_serving", db), cursor_factory=psycopg.ClientCursor) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
        w = next(x for x in work if x["scope"] == "universe")
        out["explain_universe"] = "\n".join(x[0] for x in c.execute(
            "EXPLAIN (ANALYZE, BUFFERS, COSTS OFF) " + SPARSE_SQL, w["p"]).fetchall())
    # postings matched per query: the work the GROUP BY does
    with R.su(db) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        counts = [c.execute(f"SELECT count(*) FROM {p} WHERE token_id = ANY(%s)", (w["p"]["tok"],)).fetchone()[0]
                  for w in work[:60]]
    out["postings_touched_p50"] = R.pct(counts, 50)
    out["postings_touched_p95"] = R.pct(counts, 95)
    # write: one memory's 60 postings, as the serving role
    wl = []
    with R.serving(db) as c:
        c.execute("SET synchronous_commit = off")
        c.commit()
        for i in range(300):
            toks = np.unique(zipf_tokens(rng, 75))[:60]
            rows = [(t, 10**9 + i, int(k), float(v), leaves[0], False)
                    for k, v in zip(toks, rng.gamma(1.5, 0.6, size=len(toks)))]
            t0 = time.perf_counter()
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                with c.cursor() as cur:
                    cur.executemany("INSERT INTO sparse_postings VALUES (%s, %s, %s, %s, %s, %s)", rows)
            wl.append((time.perf_counter() - t0) * 1000)
    out["write_60_postings"] = summ(wl)
    with R.su() as c:
        c.execute(f"DROP DATABASE {db} WITH (FORCE)")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("case", choices=["exact", "sparse"])
    ap.add_argument("--scale", type=int, required=True)
    ap.add_argument("--queries", type=int, default=330)
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-cold", action="store_true", help="skip the cluster restart (a smoke test)")
    a = ap.parse_args()
    out = exact_case(a) if a.case == "exact" else sparse_case(a)
    out["finished"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    R.save(out, R.HERE / a.out)


if __name__ == "__main__":
    main()
