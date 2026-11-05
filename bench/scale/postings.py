#!/usr/bin/env python3
"""The postings lane of the scale bench (ADR-0052, FR-100): `bm25` and
`bm25-postings` at a given number of memories per tenant, through the adapter's
own statements, as the serving role under row security.

    python bench/scale/postings.py --rows 100000 --out bench/scale/results/postings-100k.json

A throwaway PostgreSQL cluster must be running (PGHOST, PGPORT, PGUSER: a
superuser), and the driver built (`cargo build --release -p oml-pg --example
search_scale`; SEARCH_SCALE names its path). Each run:

1. creates a database owned by a fresh owner role, migrates it through 0020
   as that role with `oml.serving_role` set, as `PgStore::migrate` does;
2. loads one tenant's synthetic memories (gen.py, the scale lane's
   generator) into its partition, computing each row's stored lexemes with
   0020's `oml_search_vector`, as a write does;
3. applies 0021 and times it: the fill of every row's postings and counts;
4. measures the space of the postings, their two indexes and the counts;
5. runs the scale lane's 330 queries over leaf, project and universe scopes
   through `search_scale` (PgStore::retrieve, warm) for `bm25`, for
   `bm25_postings` unpruned, and for `bm25_postings` at each candidate pair of
   prune constants; and with `(kind, token_id, workarea_id)` as the access
   index in place of `(kind, token_id)` (ADR-0052 open question 1);
6. measures a key holding 1,000 and 10,000 own held rows (open question 7);
7. times the trigger's share of a write: insert, promote and supersede, with
   and without it.

It writes one JSON file of every number. The bench's only additions to the
schema: the created_event_id key is dropped before the load, so loaded rows
need no journal rows, as the scale lane did.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import statistics
import subprocess
import time
import uuid
from pathlib import Path

import psycopg
from psycopg import sql as S

import gen

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
HOST = os.environ.get("PGHOST", "127.0.0.1")
PORT = os.environ.get("PGPORT", "55481")
SUPER = os.environ.get("PGUSER", "oml")
DRIVER = os.environ.get("SEARCH_SCALE", str(REPO / "target/release/examples/search_scale"))
QUERIES = 330
# Candidate prune constants (frequent share, posting budget); "off" is
# PruneLimits::NONE.
SWEEP = [(0.1, 5000), (0.1, 20000)]


def dsn(user: str, db: str) -> str:
    return f"host={HOST} port={PORT} user={user} dbname={db}"


def su(db: str = "postgres") -> psycopg.Connection:
    return psycopg.connect(dsn(SUPER, db), autocommit=True)


def migrate(db: str, owner: str, serving: str, through: int) -> None:
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute(S.SQL("SET oml.serving_role = {}").format(S.Literal(serving)))
        # sqlx's bookkeeping table, which the migrations' grants name.
        c.execute("CREATE TABLE IF NOT EXISTS _sqlx_migrations (version bigint PRIMARY KEY, "
                  "description text NOT NULL, installed_on timestamptz NOT NULL DEFAULT now(), "
                  "success boolean NOT NULL, checksum bytea NOT NULL, "
                  "execution_time bigint NOT NULL)")
        for f in sorted((REPO / "migrations").glob("*.sql")):
            if int(f.name[:4]) <= through:
                with c.transaction():
                    c.execute(f.read_text())


def apply_0021(db: str, owner: str, serving: str) -> float:
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute(S.SQL("SET oml.serving_role = {}").format(S.Literal(serving)))
        c.execute("SET maintenance_work_mem = '1GB'")
        t0 = time.perf_counter()
        with c.transaction():
            c.execute((REPO / "migrations" / "0021_search_postings.sql").read_text())
        return time.perf_counter() - t0


COLS = ["id", "tenant_id", "workarea_id", "claim", "claim_digest", "status", "created_event_id",
        "claim_key", "author_principal", "author_key_id", "version", "kind", "is_tip",
        "body_bytes", "retirement", "severity"]


def load(db: str, owner: str, rows: int, seed: int):
    r = random.Random(seed)
    t = uuid.UUID(int=r.getrandbits(128), version=4)
    tree = gen.make_tree(r, rows)
    keys = [uuid.UUID(int=r.getrandbits(128), version=4) for _ in range(20)]
    with su(db) as c:  # the genesis rows, as the lead's bootstrap writes them
        c.execute("INSERT INTO tenants (id, name) VALUES (%s, 'scale')", (t,))
        c.execute("INSERT INTO workareas (id, tenant_id, parent_id, status, name) "
                  "VALUES (%s, %s, NULL, 'committed', 'universe')", (tree.universe, t))
        for i, p in enumerate(tree.projects):
            c.execute("INSERT INTO workareas (id, tenant_id, parent_id, status, name) "
                      "VALUES (%s, %s, %s, 'committed', %s)", (p, t, tree.universe, f"p{i}"))
            for j, leaf in enumerate(tree.leaves[p]):
                c.execute("INSERT INTO workareas (id, tenant_id, parent_id, status, name) "
                          "VALUES (%s, %s, %s, 'committed', %s)", (leaf, t, p, f"p{i}-l{j}"))
    p = "memories_t_" + t.hex
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute("SET maintenance_work_mem = '1GB'")
        c.execute("ALTER TABLE memories DROP CONSTRAINT IF EXISTS memories_created_event_id_fkey")
        c.execute(f"CREATE TABLE {p} (LIKE memories INCLUDING DEFAULTS INCLUDING CONSTRAINTS "
                  "INCLUDING STORAGE)")
        c.execute(f"CREATE TEMP TABLE staging (LIKE {p} INCLUDING DEFAULTS)")
        c.execute("ALTER TABLE staging ALTER COLUMN search_vector DROP NOT NULL, "
                  "ALTER COLUMN search_len DROP NOT NULL")
        t0 = time.perf_counter()
        with c.cursor().copy(f"COPY staging ({', '.join(COLS)}) FROM STDIN") as cp:
            for row in gen.rows(rows, t, tree, seed, keys):
                cp.write_row([row[k] for k in COLS])
        cols = ", ".join(COLS)
        c.execute(f"INSERT INTO {p} ({cols}, search_vector, search_len) "
                  f"SELECT {cols}, v, length(v) FROM (SELECT *, oml_search_vector(claim) AS v "
                  "FROM staging) s")
        load_s = time.perf_counter() - t0
        for ddl in [
            f"ALTER TABLE {p} ADD CONSTRAINT {p}_pkey PRIMARY KEY (tenant_id, id)",
            f"CREATE INDEX {p}_workarea ON {p} (tenant_id, workarea_id, status)",
            f"CREATE INDEX {p}_claim_fts ON {p} USING gin (to_tsvector('simple', claim))",
            f"CREATE UNIQUE INDEX {p}_claim_version ON {p} (tenant_id, workarea_id, claim_key, "
            "version) WHERE status IN ('held', 'promoted', 'demoted')",
            f"CREATE UNIQUE INDEX {p}_served_tip ON {p} (tenant_id, workarea_id, claim_key) "
            "WHERE is_tip AND status = 'promoted'",
            f"CREATE INDEX {p}_author ON {p} (tenant_id, author_key_id, created_at DESC)",
            f"ALTER TABLE {p} ENABLE ROW LEVEL SECURITY",
            f"ALTER TABLE {p} FORCE ROW LEVEL SECURITY",
            f"CREATE POLICY tenant_isolation ON {p} USING (tenant_id = NULLIF(current_setting("
            "'oml.tenant_id', true), '')::uuid) WITH CHECK (tenant_id = NULLIF(current_setting("
            "'oml.tenant_id', true), '')::uuid)",
            f"ALTER TABLE memories ATTACH PARTITION {p} FOR VALUES IN ('{t}')",
            f"ANALYZE {p}",
        ]:
            c.execute(ddl)
    return t, tree, keys, load_s


def sizes(db: str, owner: str, t: uuid.UUID) -> dict:
    part = "search_postings_t_" + t.hex
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute(f"ANALYZE {part}")
        c.execute("ANALYZE search_term_rows")
        c.execute("ANALYZE search_workarea_rows")
        mib = lambda q: round(c.execute(q).fetchone()[0] / 2**20, 1)  # noqa: E731
        out = {
            "postings_heap_mib": mib(f"SELECT pg_relation_size('{part}')"),
            "term_rows_mib": mib("SELECT pg_total_relation_size('search_term_rows')"),
            "workarea_rows_mib": mib("SELECT pg_total_relation_size('search_workarea_rows')"),
            "memories_heap_mib": mib(f"SELECT pg_relation_size('memories_t_{t.hex}')"),
        }
        for parent, size in c.execute(
                "SELECT p.indexrelid::regclass::text, pg_relation_size(i.indexrelid) "
                "FROM pg_index i JOIN pg_inherits h ON h.inhrelid = i.indexrelid "
                "JOIN pg_index p ON p.indexrelid = h.inhparent "
                "WHERE i.indrelid = %s::regclass", (part,)).fetchall():
            out[f"{parent}_mib"] = round(size / 2**20, 1)
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        out["postings"] = c.execute(f"SELECT count(*) FROM {part}").fetchone()[0]
        out["postings_by_state"] = dict(c.execute(
            f"SELECT state, count(*) FROM {part} GROUP BY state ORDER BY state").fetchall())
        out["term_rows"] = c.execute("SELECT count(*) FROM search_term_rows").fetchone()[0]
        out["served"] = c.execute(
            "SELECT COALESCE(sum(rows), 0) FROM search_workarea_rows").fetchone()[0]
    return out


def workload(t, tree, rows: int, seed: int, own=None):
    r = random.Random(seed + 11)
    qs = gen.queries(QUERIES, seed + 3, rows)
    scopes = gen.readable_sets(r, tree, len(qs))
    out = []
    for q, (kind, nodes) in zip(qs, scopes):
        item = {"tenant": str(t), "nodes": nodes, "query": q["query"], "k": 10,
                "category": q["category"], "scope": kind}
        if own:
            item["own_keys"] = [str(own)]
            item["own_nodes"] = nodes
        out.append(item)
    return out


def drive(serving_url: str, items: list[dict], retriever: str, limits=None) -> list[dict]:
    path = HERE / f".workload-{os.getpid()}.json"
    path.write_text(json.dumps(items))
    try:
        args = [DRIVER, serving_url, str(path), retriever]
        if limits:
            args += [str(limits[0]), str(limits[1])]
        res = subprocess.run(args, capture_output=True, text=True, check=True)
    finally:
        path.unlink(missing_ok=True)
    return [json.loads(line) for line in res.stdout.splitlines() if line.strip()]


def pct(xs, p):
    xs = sorted(xs)
    return round(xs[min(len(xs) - 1, int(p * len(xs)))], 2) if xs else None


def summarize(items, got, base=None) -> dict:
    out = {"all": {"p50": pct([g["ms"] for g in got], 0.5), "p95": pct([g["ms"] for g in got], 0.95)}}
    for scope in ("leaf", "project", "universe"):
        ms = [g["ms"] for g, it in zip(got, items) if it["scope"] == scope]
        out[scope] = {"n": len(ms), "p50": pct(ms, 0.5), "p95": pct(ms, 0.95)}
    out["scan_share"] = round(sum(g["access"] == "scan" for g in got) / len(got), 4)
    out["pruned_share"] = round(sum(bool(g["pruned"]) for g in got) / len(got), 4)
    if base:
        same = sum(g["ids"] == b["ids"] for g, b in zip(got, base))
        overlap = [len(set(g["ids"]) & set(b["ids"])) / max(1, len(b["ids"]))
                   for g, b in zip(got, base) if b["ids"]]
        out["same_list_share"] = round(same / len(got), 4)
        out["differing"] = [
            {"query": it["query"], "scope": it["scope"],
             "got": list(zip(g["ids"], g["scores"])), "base": list(zip(b["ids"], b["scores"]))}
            for g, b, it in zip(got, base, items) if g["ids"] != b["ids"]][:3]
        out["overlap_at_10"] = round(statistics.mean(overlap), 4) if overlap else None
    return out


def write_cost(db: str, serving: str, t, tree, keys, count: int = 200) -> dict:
    """Insert a held row, promote it, then supersede it, each its own
    transaction as the serving role under the tenant's row security; the
    p50/p95 of each, with the postings trigger and without it."""
    texts = gen.new_texts(count, 99, 1000)
    leaves = tree.leaf_list()
    out = {}
    for label in ("with_trigger", "without_trigger"):
        enable = "ENABLE" if label == "with_trigger" else "DISABLE"
        with su(db) as c:
            c.execute(f"ALTER TABLE memories {enable} TRIGGER memories_search_postings")
        times = {"insert": [], "promote": [], "supersede": []}
        with psycopg.connect(dsn(serving, db)) as c:
            for i, text in enumerate(texts):
                wa, key = leaves[i % len(leaves)], keys[i % len(keys)]
                a, b = uuid.uuid4(), uuid.uuid4()
                ck = f"w-{label}-{i}"
                for step, stmts in [
                    ("insert", [("INSERT INTO memories (id, tenant_id, workarea_id, claim, "
                                 "claim_digest, status, created_event_id, claim_key, "
                                 "author_principal, author_key_id, version, kind, body_bytes) "
                                 "VALUES (%s, %s, %s, %s, '\\x00', 'held', %s, %s, 'agent', %s, "
                                 "1, 'memory', %s)",
                                 (a, t, wa, text, uuid.uuid4(), ck, key, len(text)))]),
                    ("promote", [("UPDATE memories SET status = 'promoted', is_tip = true "
                                  "WHERE tenant_id = %s AND id = %s", (t, a))]),
                    ("supersede", [("UPDATE memories SET is_tip = false WHERE tenant_id = %s "
                                    "AND id = %s", (t, a)),
                                   ("INSERT INTO memories (id, tenant_id, workarea_id, claim, "
                                    "claim_digest, status, created_event_id, claim_key, "
                                    "author_principal, author_key_id, version, kind, body_bytes, "
                                    "is_tip, replaces_artifact_id) VALUES (%s, %s, %s, %s, '\\x00', "
                                    "'promoted', %s, %s, 'agent', %s, 2, 'memory', %s, true, %s)",
                                    (b, t, wa, text + " again", uuid.uuid4(), ck, key,
                                     len(text) + 6, a))]),
                ]:
                    t0 = time.perf_counter()
                    with c.transaction():
                        c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                        for q, params in stmts:
                            c.execute(q, params)
                    times[step].append((time.perf_counter() - t0) * 1000)
        out[label] = {k: {"p50": pct(v, 0.5), "p95": pct(v, 0.95)} for k, v in times.items()}
    with su(db) as c:
        c.execute("ALTER TABLE memories ENABLE TRIGGER memories_search_postings")
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--rows", type=int, required=True)
    ap.add_argument("--seed", type=int, default=20261105)
    ap.add_argument("--out", required=True)
    ap.add_argument("--keep", action="store_true", help="keep the database")
    ap.add_argument("--load-only", action="store_true",
                    help="stop after the load, the fill and the space, keeping the database")
    a = ap.parse_args()
    tag = uuid.uuid4().hex[:8]
    db, owner, serving = f"scale_{tag}", f"scale_{tag}_owner", f"scale_{tag}_serving"
    with su() as c:
        c.execute(S.SQL("CREATE ROLE {} LOGIN").format(S.Identifier(owner)))
        c.execute(S.SQL("CREATE ROLE {} LOGIN").format(S.Identifier(serving)))
        c.execute(S.SQL("CREATE DATABASE {} OWNER {}").format(S.Identifier(db), S.Identifier(owner)))
    result = {"rows": a.rows, "seed": a.seed, "date": time.strftime("%Y-%m-%d"),
              "postgres": su().execute("SHOW server_version").fetchone()[0]}
    try:
        migrate(db, owner, serving, 20)
        t, tree, keys, load_s = load(db, owner, a.rows, a.seed)
        result["load_s"] = round(load_s, 1)
        result["workareas"] = len(tree.all())
        result["fill_0021_s"] = round(apply_0021(db, owner, serving), 2)
        with psycopg.connect(dsn(owner, db), autocommit=True) as c:
            c.execute(f"VACUUM ANALYZE search_postings_t_{t.hex}")
        result["space"] = sizes(db, owner, t)
        if a.load_only:
            a.keep = True
            result["database"] = db
            result["workload"] = workload(t, tree, a.rows, a.seed)
            return
        url = f"postgres://{serving}@{HOST}:{PORT}/{db}"
        items = workload(t, tree, a.rows, a.seed)
        with su(db) as c:
            c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
            result["readable_rows"] = {
                scope: round(statistics.mean(
                    c.execute("SELECT COALESCE(sum(rows), 0) FROM search_workarea_rows "
                              "WHERE workarea_id = ANY(%s::uuid[])", (it["nodes"],)).fetchone()[0]
                    for it in items if it["scope"] == scope))
                for scope in ("leaf", "project", "universe")}
        runs = {}
        base = drive(url, items, "bm25")
        runs["bm25"] = summarize(items, base)
        unpruned = drive(url, items, "bm25_postings", (float("inf"), 2**63 - 1))
        runs["bm25_postings_unpruned"] = summarize(items, unpruned, base)
        for share, budget in SWEEP:
            got = drive(url, items, "bm25_postings", (share, budget))
            runs[f"bm25_postings_{share}_{budget}"] = summarize(items, got, base)
        served = drive(url, items, "bm25_postings")
        runs["bm25_postings_served"] = summarize(items, served, base)
        result["runs"] = runs
        # Open question 1: the access index as ADR-0052 d1 first wrote it,
        # (kind, token_id) over every posting, with the scope and the state
        # filtered from the heap, in place of the served covering index.
        part = "search_postings_t_" + t.hex
        served_def = "(kind, token_id, workarea_id) INCLUDE (memory_kind, memory_id, weight, " \
                     "doc_len, tenant_id) WHERE state = 1"
        with psycopg.connect(dsn(owner, db), autocommit=True) as c:
            c.execute("SET maintenance_work_mem = '1GB'")
            c.execute("DROP INDEX search_postings_served")
            t0 = time.perf_counter()
            c.execute("CREATE INDEX search_postings_served ON search_postings (kind, token_id)")
            build = time.perf_counter() - t0
            size = c.execute(
                "SELECT pg_relation_size(indexrelid) FROM pg_index WHERE indrelid = %s::regclass "
                "AND pg_get_indexdef(indexrelid) LIKE '%%(kind, token_id)'", (part,)).fetchone()[0]
            c.execute(f"VACUUM ANALYZE {part}")
        got = drive(url, items, "bm25_postings")
        result["index_kind_token"] = {"build_s": round(build, 2), "mib": round(size / 2**20, 1),
                                      **summarize(items, got, served)}
        with psycopg.connect(dsn(owner, db), autocommit=True) as c:
            c.execute("SET maintenance_work_mem = '1GB'")
            c.execute("DROP INDEX search_postings_served")
            c.execute(f"CREATE INDEX search_postings_served ON search_postings {served_def}")
            c.execute(f"VACUUM ANALYZE {part}")
        # Open question 7: a key holding many own held rows.
        own = {}
        for held in (1000, 10000):
            key = uuid.uuid4()
            with su(db) as c:
                c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
                n = c.execute(
                    "UPDATE memories SET author_key_id = %s WHERE tenant_id = %s AND id IN "
                    "(SELECT id FROM memories WHERE tenant_id = %s AND status = 'held' "
                    "AND kind IN ('memory', 'derived_fact') AND NOT retirement "
                    "AND author_key_id <> ALL(%s::uuid[]) LIMIT %s)",
                    (key, t, t, [str(k) for k in own], held)).rowcount
                c.execute(f"ANALYZE memories_t_{t.hex}")
            mine = workload(t, tree, a.rows, a.seed, own=key)
            own[str(key)] = n
            result.setdefault("own_held", {})[str(held)] = {
                "rows": n,
                "bm25": summarize(mine, drive(url, mine, "bm25")),
                "bm25_postings": summarize(mine, drive(url, mine, "bm25_postings")),
            }
        result["write_ms"] = write_cost(db, serving, t, tree, keys)
    finally:
        Path(a.out).parent.mkdir(parents=True, exist_ok=True)
        Path(a.out).write_text(json.dumps(result, indent=1) + "\n")
        if not a.keep:
            with su() as c:
                c.execute(S.SQL("DROP DATABASE IF EXISTS {} WITH (FORCE)").format(S.Identifier(db)))
                c.execute(S.SQL("DROP ROLE IF EXISTS {}").format(S.Identifier(owner)))
                c.execute(S.SQL("DROP ROLE IF EXISTS {}").format(S.Identifier(serving)))
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(result, indent=1) + "\n")
    print(json.dumps({k: result[k] for k in ("rows", "fill_0021_s", "readable_rows")}))


if __name__ == "__main__":
    main()
