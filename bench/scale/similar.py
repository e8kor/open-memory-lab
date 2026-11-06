#!/usr/bin/env python3
"""The similar check's lane of the scale bench (ADR-0050 d17, #141): the
write-time near-copy lookup at a given number of memories per tenant, through
the adapter's own statement, as the serving role under row security.

    python bench/scale/similar.py --rows 100000 --out bench/scale/results/similar-100k.json

A throwaway PostgreSQL cluster must be running (PGHOST, PGPORT, PGUSER: a
superuser). Each run:

1. creates a database owned by a fresh owner role and migrates it through
   0024 as that role, as `PgStore::migrate` does;
2. loads one tenant's synthetic memories (gen.py, the scale lane's
   generator, templated from the recall eval's 200 memories, so its texts
   share far more words than real ones: the candidate counts are an upper
   end) into its partition, as postings.py does;
3. applies 0027 and times it: the fill of every held version's and served
   tip's similar keys;
4. measures the keys' space;
5. asks 330 writes, each the text of a loaded memory with one word added (a
   near copy) or a new text, at leaf, project and universe scopes: the
   write's keys, computed by `oml_similar_keys` and not timed, then
   `oml_pg::SIMILAR_CANDIDATES` (at most 32) and the read of the candidates'
   rows by id, each timed, with the candidates counted;
6. times a write's insert and promotion with the similar keys trigger and
   without it.

The bench's only change to the schema: created_event_id's key is dropped
before the load, as postings.py does. Postings are not filled: no statement
here reads them.
"""

from __future__ import annotations

import argparse
import json
import random
import re
import time
import uuid
from pathlib import Path

import psycopg
from psycopg import sql as S

import gen
from postings import COLS, HOST, PORT, REPO, dsn, migrate, pct, su

PROBES = 330


def statement() -> str:
    """oml-pg's SIMILAR_CANDIDATES, read from the adapter's source so the
    bench runs the statement the lead runs."""
    src = (REPO / "crates/adapters/oml-pg/src/lib.rs").read_text()
    m = re.search(r'pub const SIMILAR_CANDIDATES: &str = "(.*?)";', src, re.S)
    text = re.sub(r"\\\n\s*", "", m.group(1))
    # Its placeholders, $1 to $6 each once and in order, as psycopg spells them.
    return re.sub(r"\$\d", "%s", text)


def load(db: str, owner: str, rows: int, seed: int):
    r = random.Random(seed)
    t = uuid.UUID(int=r.getrandbits(128), version=4)
    tree = gen.make_tree(r, rows)
    keys = [uuid.UUID(int=r.getrandbits(128), version=4) for _ in range(20)]
    with su(db) as c:
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
    sp = "search_postings_t_" + t.hex
    policy = ("USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) "
              "WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)")
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute("SET maintenance_work_mem = '1GB'")
        # The tenant's postings partition, which a write's postings trigger
        # needs, empty: nothing here reads it.
        for ddl in [f"CREATE TABLE {sp} (LIKE search_postings INCLUDING ALL)",
                    f"ALTER TABLE {sp} ENABLE ROW LEVEL SECURITY",
                    f"ALTER TABLE {sp} FORCE ROW LEVEL SECURITY",
                    f"CREATE POLICY tenant_isolation ON {sp} {policy}",
                    f"ALTER TABLE search_postings ATTACH PARTITION {sp} FOR VALUES IN ('{t}')"]:
            c.execute(ddl)
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
            f"CREATE UNIQUE INDEX {p}_claim_version ON {p} (tenant_id, workarea_id, claim_key, "
            "version) WHERE status IN ('held', 'promoted', 'demoted')",
            f"CREATE UNIQUE INDEX {p}_served_tip ON {p} (tenant_id, workarea_id, claim_key) "
            "WHERE is_tip AND status = 'promoted'",
            f"ALTER TABLE {p} ENABLE ROW LEVEL SECURITY",
            f"ALTER TABLE {p} FORCE ROW LEVEL SECURITY",
            f"CREATE POLICY tenant_isolation ON {p} {policy}",
            f"ALTER TABLE memories ATTACH PARTITION {p} FOR VALUES IN ('{t}')",
            f"ANALYZE {p}",
        ]:
            c.execute(ddl)
    return t, tree, keys, load_s


def apply_0025(db: str, owner: str, serving: str) -> float:
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute(S.SQL("SET oml.serving_role = {}").format(S.Literal(serving)))
        c.execute("SET maintenance_work_mem = '1GB'")
        t0 = time.perf_counter()
        with c.transaction():
            c.execute((REPO / "migrations" / "0027_similar_keys.sql").read_text())
        return time.perf_counter() - t0


def space(db: str, owner: str, t: uuid.UUID) -> dict:
    part = "similar_keys_t_" + t.hex
    with psycopg.connect(dsn(owner, db), autocommit=True) as c:
        c.execute(f"VACUUM ANALYZE {part}")
        mib = lambda q: round(c.execute(q).fetchone()[0] / 2**20, 1)  # noqa: E731
        out = {"keys_heap_mib": mib(f"SELECT pg_relation_size('{part}')"),
               "keys_total_mib": mib(f"SELECT pg_total_relation_size('{part}')"),
               "memories_heap_mib": mib(f"SELECT pg_relation_size('memories_t_{t.hex}')")}
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        out["keys"] = c.execute(f"SELECT count(*) FROM {part}").fetchone()[0]
        out["versions_keyed"] = c.execute(
            f"SELECT count(DISTINCT memory_id) FROM {part}").fetchone()[0]
    return out


def probes(db: str, t, tree, seed: int):
    r = random.Random(seed + 21)
    scopes = gen.readable_sets(r, tree, PROBES)
    news = gen.new_texts(PROBES, seed + 5, 1000)
    out = []
    with su(db) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        for i, (kind, nodes) in enumerate(scopes):
            if i % 2 == 0:
                text = c.execute(
                    "SELECT claim FROM memories WHERE tenant_id = %s AND status = 'promoted' "
                    "AND is_tip AND workarea_id = ANY(%s::uuid[]) ORDER BY random() LIMIT 1",
                    (t, nodes)).fetchone()
                text, label = ((text[0] + " Confirmed.", "near_copy") if text
                               else (news[i], "new"))
            else:
                text, label = news[i], "new"
            keys = [k for (k,) in c.execute("SELECT key FROM oml_similar_keys(%s)", (text,))]
            out.append({"scope": kind, "nodes": nodes, "label": label, "keys": keys})
    return out


def lookup(db: str, serving: str, t, items, author) -> list[dict]:
    q = statement()
    got = []
    with psycopg.connect(dsn(serving, db)) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        c.commit()
        for it in items * 2:  # the first pass warms; the second is measured
            t0 = time.perf_counter()
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                ids = [m for (m,) in c.execute(q, (t, it["keys"], it["nodes"], [author],
                                                   it["nodes"], 32))]
            t1 = time.perf_counter()
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                c.execute("SELECT id, claim FROM memories WHERE tenant_id = %s AND id = ANY(%s)",
                          (t, ids)).fetchall()
            t2 = time.perf_counter()
            got.append({"scope": it["scope"], "label": it["label"], "candidates": len(ids),
                        "lookup_ms": (t1 - t0) * 1000, "with_rows_ms": (t2 - t0) * 1000})
    return got[len(items):]


def summary(got: list[dict]) -> dict:
    out = {}
    for scope in ("all", "leaf", "project", "universe"):
        xs = [g for g in got if scope in ("all", g["scope"])]
        out[scope] = {
            "n": len(xs),
            "lookup_ms": {"p50": pct([g["lookup_ms"] for g in xs], 0.5),
                          "p95": pct([g["lookup_ms"] for g in xs], 0.95)},
            "with_rows_ms": {"p50": pct([g["with_rows_ms"] for g in xs], 0.5),
                             "p95": pct([g["with_rows_ms"] for g in xs], 0.95)},
            "candidates": {"p50": pct([g["candidates"] for g in xs], 0.5),
                           "max": max(g["candidates"] for g in xs)},
        }
    return out


def plan(db: str, serving: str, t, item, author) -> str:
    with psycopg.connect(dsn(serving, db)) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        rows = c.execute("EXPLAIN " + statement(),
                         (t, item["keys"], item["nodes"], [author], item["nodes"], 32)).fetchall()
    return "\n".join(r[0] for r in rows)


def write_cost(db: str, serving: str, t, tree, keys, count: int = 200) -> dict:
    texts = gen.new_texts(count, 98, 1000)
    leaves = tree.leaf_list()
    out = {}
    for label in ("with_trigger", "without_trigger"):
        enable = "ENABLE" if label == "with_trigger" else "DISABLE"
        with su(db) as c:
            c.execute(f"ALTER TABLE memories {enable} TRIGGER memories_similar_keys")
        times = {"insert": [], "promote": []}
        with psycopg.connect(dsn(serving, db)) as c:
            for i, text in enumerate(texts):
                wa, key, a = leaves[i % len(leaves)], keys[i % len(keys)], uuid.uuid4()
                for step, (q, params) in [
                    ("insert", ("INSERT INTO memories (id, tenant_id, workarea_id, claim, "
                                "claim_digest, status, created_event_id, claim_key, "
                                "author_principal, author_key_id, version, kind, body_bytes) "
                                "VALUES (%s, %s, %s, %s, '\\x00', 'held', %s, %s, 'agent', %s, "
                                "1, 'memory', %s)",
                                (a, t, wa, text, uuid.uuid4(), f"w-{label}-{i}", key,
                                 len(text)))),
                    ("promote", ("UPDATE memories SET status = 'promoted', is_tip = true "
                                 "WHERE tenant_id = %s AND id = %s", (t, a))),
                ]:
                    t0 = time.perf_counter()
                    with c.transaction():
                        c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                        c.execute(q, params)
                    times[step].append((time.perf_counter() - t0) * 1000)
        out[label] = {k: {"p50": pct(v, 0.5), "p95": pct(v, 0.95)} for k, v in times.items()}
    with su(db) as c:
        c.execute("ALTER TABLE memories ENABLE TRIGGER memories_similar_keys")
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--rows", type=int, required=True)
    ap.add_argument("--seed", type=int, default=20261105)
    ap.add_argument("--out", required=True)
    ap.add_argument("--keep", action="store_true", help="keep the database")
    a = ap.parse_args()
    tag = uuid.uuid4().hex[:8]
    db, owner, serving = f"similar_{tag}", f"similar_{tag}_owner", f"similar_{tag}_serving"
    with su() as c:
        c.execute(S.SQL("CREATE ROLE {} LOGIN").format(S.Identifier(owner)))
        c.execute(S.SQL("CREATE ROLE {} LOGIN").format(S.Identifier(serving)))
        c.execute(S.SQL("CREATE DATABASE {} OWNER {}").format(S.Identifier(db), S.Identifier(owner)))
    result = {"rows": a.rows, "seed": a.seed, "date": time.strftime("%Y-%m-%d"),
              "postgres": su().execute("SHOW server_version").fetchone()[0]}
    try:
        migrate(db, owner, serving, 24)
        t, tree, keys, load_s = load(db, owner, a.rows, a.seed)
        result["load_s"] = round(load_s, 1)
        result["workareas"] = len(tree.all())
        result["fill_0025_s"] = round(apply_0025(db, owner, serving), 2)
        result["space"] = space(db, owner, t)
        items = probes(db, t, tree, a.seed)
        author = keys[0]
        got = lookup(db, serving, t, items, author)
        result["lookup"] = {"all": summary(got),
                            "near_copy": summary([g for g in got if g["label"] == "near_copy"]),
                            "new": summary([g for g in got if g["label"] == "new"])}
        widest = max(items, key=lambda i: len(i["nodes"]))
        result["plan_universe"] = plan(db, serving, t, widest, author)
        result["write_ms"] = write_cost(db, serving, t, tree, keys)
    finally:
        if not a.keep:
            with su() as c:
                c.execute(S.SQL("DROP DATABASE IF EXISTS {} WITH (FORCE)").format(S.Identifier(db)))
                c.execute(S.SQL("DROP ROLE IF EXISTS {}").format(S.Identifier(owner)))
                c.execute(S.SQL("DROP ROLE IF EXISTS {}").format(S.Identifier(serving)))
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(result, indent=1) + "\n")
    print(json.dumps({k: result[k] for k in ("rows", "fill_0025_s", "space")}))
    print(json.dumps(result["lookup"]["all"]["all"]))


if __name__ == "__main__":
    main()
