"""Scale bench runner: OML's candidate search approaches at 10k / 100k / 1M
memories per tenant, on OML's real schema (migrations/0001..0016) with the
owner/serving role split and row-level security, queried as the serving role
with oml.tenant_id set, as the lead does (crates/adapters/oml-pg/src/lib.rs,
RECALL_FTS and set_tenant).

Usage (a throwaway cluster must be running; see README.md):

    python run.py --scale 10000 --out results/s10k.json
    python run.py --scale 100000 --tenants 10 --out results/mt.json --multi

Each run creates a fresh database, migrates it as the owner role, loads one or
more tenants, then for each approach measures cold and warm latency over the
query set, index build time and size, and write cost.
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

import numpy as np
import psycopg
from psycopg import sql as S

import gen

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
PG_BIN = "/usr/lib/postgresql/18/bin"
PGDATA = os.environ.get("PGDATA_SCALE", "/srv/build/pg-scale/data")
HOST, PORT = "127.0.0.1", 55439
LOOP_IMAGE = "/mnt/HC_Volume_107228841/build-area.img"
K = 10
KINDS = ["memory", "derived_fact"]
TIMEOUT_MS = 10000
DIM = 384


def dsn(user: str, db: str) -> str:
    return f"host={HOST} port={PORT} user={user} dbname={db}"


def su(db: str = "postgres", autocommit: bool = True) -> psycopg.Connection:
    return psycopg.connect(dsn("postgres", db), autocommit=autocommit)


# ---------------------------------------------------------------------------
# Cluster control: a cold cache is a restart (shared_buffers) plus
# POSIX_FADV_DONTNEED over every data file and over the loop image that backs
# /srv/build (the host's page cache would otherwise serve the loop device).
# ---------------------------------------------------------------------------

def evict_os_cache() -> None:
    for root, _, files in os.walk(PGDATA):
        for f in files:
            try:
                fd = os.open(os.path.join(root, f), os.O_RDONLY)
                os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
                os.close(fd)
            except OSError:
                pass
    try:
        fd = os.open(LOOP_IMAGE, os.O_RDONLY)
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        os.close(fd)
    except OSError:
        pass


def restart(cold: bool) -> None:
    subprocess.run([f"{PG_BIN}/pg_ctl", "-D", PGDATA, "-m", "fast", "-w", "stop"],
                   check=True, capture_output=True)
    if cold:
        subprocess.run(["sync"], check=False)
        evict_os_cache()
    subprocess.run([f"{PG_BIN}/pg_ctl", "-D", PGDATA, "-l", f"{PGDATA}/../log", "-w", "start"],
                   check=True, capture_output=True)


# ---------------------------------------------------------------------------
# Schema: a fresh database migrated by the owner role as the lead does.
# ---------------------------------------------------------------------------

def create_db(db: str) -> None:
    with su() as c:
        c.execute(S.SQL("DROP DATABASE IF EXISTS {} WITH (FORCE)").format(S.Identifier(db)))
        c.execute(S.SQL("CREATE DATABASE {} OWNER oml_owner").format(S.Identifier(db)))
    # Extensions need a superuser here (vector and pg_trgm are installed
    # beside the system's, not by the owner role).
    with su(db) as c:
        c.execute("CREATE EXTENSION IF NOT EXISTS vector")
        c.execute("CREATE EXTENSION IF NOT EXISTS pg_trgm")
    with psycopg.connect(dsn("oml_owner", db), autocommit=True) as c:
        c.execute("SET oml.serving_role = 'oml_serving'")
        c.execute("""CREATE TABLE _sqlx_migrations (version bigint PRIMARY KEY,
            description text NOT NULL, installed_on timestamptz NOT NULL DEFAULT now(),
            success boolean NOT NULL, checksum bytea NOT NULL, execution_time bigint NOT NULL)""")
        for f in sorted((REPO / "migrations").glob("*.sql")):
            with c.transaction():
                c.execute(f.read_text())
        # Bench additions, owned by the owner role like the rest:
        #  - idents: the identifier-like tokens of claim, app-computed (approach e);
        #  - the created_event_id key re-added NOT VALID after the bulk load,
        #    so the loaded rows need no journal rows but every new write is
        #    checked (see load_tenant).
        c.execute("ALTER TABLE memories ADD COLUMN idents text")
        c.execute("ALTER TABLE memories DROP CONSTRAINT memories_created_event_id_fkey")


def owner(db: str) -> psycopg.Connection:
    c = psycopg.connect(dsn("oml_owner", db), autocommit=True)
    c.execute("SET maintenance_work_mem = '1GB'")
    return c


def rel_size(c, name: str) -> int:
    return c.execute("SELECT pg_relation_size(%s::regclass)", (name,)).fetchone()[0]


def total_size(c, name: str) -> int:
    return c.execute("SELECT pg_total_relation_size(%s::regclass)", (name,)).fetchone()[0]


BASE_INDEXES = {
    # today's per-partition indexes (0009), built after the load and attached
    "pkey": "ALTER TABLE {p} ADD CONSTRAINT {p}_pkey PRIMARY KEY (tenant_id, id)",
    "workarea": "CREATE INDEX {p}_workarea ON {p} (tenant_id, workarea_id, status)",
    "claim_fts": "CREATE INDEX {p}_claim_fts ON {p} USING gin (to_tsvector('simple', claim))",
    "claim_version": "CREATE UNIQUE INDEX {p}_claim_version ON {p} (tenant_id, workarea_id, claim_key, version) "
                     "WHERE status IN ('held', 'promoted', 'demoted')",
    "served_tip": "CREATE UNIQUE INDEX {p}_served_tip ON {p} (tenant_id, workarea_id, claim_key) "
                  "WHERE is_tip AND status = 'promoted'",
    "author": "CREATE INDEX {p}_author ON {p} (tenant_id, author_key_id, created_at DESC)",
}

COLS = ["id", "tenant_id", "workarea_id", "claim", "claim_digest", "status", "created_event_id",
        "claim_key", "author_principal", "author_key_id", "version", "kind", "is_tip",
        "body_bytes", "retirement", "severity", "idents"]


def setup_tenant(db: str, r: random.Random, n: int, name: str):
    t = uuid.UUID(int=r.getrandbits(128), version=4)
    tree = gen.make_tree(r, n)
    keys = [uuid.UUID(int=r.getrandbits(128), version=4) for _ in range(20)]
    with su(db) as c:  # superuser: genesis rows, as the lead's bootstrap writes them
        c.execute("INSERT INTO tenants (id, name) VALUES (%s, %s)", (t, name))
        c.execute("INSERT INTO workareas (id, tenant_id, parent_id, status, name) "
                  "VALUES (%s, %s, NULL, 'committed', 'universe')", (tree.universe, t))
        for i, p in enumerate(tree.projects):
            c.execute("INSERT INTO workareas (id, tenant_id, parent_id, status, name) "
                      "VALUES (%s, %s, %s, 'committed', %s)", (p, t, tree.universe, f"p{i}"))
            for j, l in enumerate(tree.leaves[p]):
                c.execute("INSERT INTO workareas (id, tenant_id, parent_id, status, name) "
                          "VALUES (%s, %s, %s, 'committed', %s)", (l, t, p, f"p{i}-l{j}"))
    return t, tree, keys


def load_tenant(db: str, t, tree, keys, n: int, seed: int, stats: dict) -> dict:
    """COPY the tenant's rows into a fresh table, build today's indexes on it
    (timed), fence it as create_partition_on does, and attach it."""
    p = "memories_t_" + t.hex
    seeds_of = []
    with owner(db) as c:
        c.execute(f"CREATE TABLE {p} (LIKE memories INCLUDING DEFAULTS INCLUDING CONSTRAINTS INCLUDING STORAGE)")
        t0 = time.perf_counter()
        with c.cursor().copy(f"COPY {p} ({', '.join(COLS)}) FROM STDIN") as cp:
            for row in gen.rows(n, t, tree, seed, keys):
                cp.write_row([row[k] for k in COLS])
                seeds_of.append(row["seed_index"])
        stats["copy_s"] = time.perf_counter() - t0
        stats["heap_bytes"] = rel_size(c, p)
        stats["toast_bytes"] = total_size(c, p) - rel_size(c, p)
        stats["index"] = {}
        for name, ddl in BASE_INDEXES.items():
            t0 = time.perf_counter()
            c.execute(ddl.format(p=p))
            idx = f"{p}_{name}"
            stats["index"][name] = {"build_s": time.perf_counter() - t0, "bytes": rel_size(c, idx)}
        c.execute(f"ALTER TABLE {p} ENABLE ROW LEVEL SECURITY")
        c.execute(f"ALTER TABLE {p} FORCE ROW LEVEL SECURITY")
        c.execute(f"CREATE POLICY tenant_isolation ON {p} "
                  "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) "
                  "WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)")
        t0 = time.perf_counter()
        c.execute(f"ALTER TABLE memories ATTACH PARTITION {p} FOR VALUES IN ('{t}')")
        stats["attach_s"] = time.perf_counter() - t0
        c.execute(f"ANALYZE {p}")
    return {"partition": p, "seeds": seeds_of}


def restore_fk(db: str) -> None:
    with owner(db) as c:
        c.execute("ALTER TABLE memories ADD CONSTRAINT memories_created_event_id_fkey "
                  "FOREIGN KEY (tenant_id, created_event_id) REFERENCES journal (tenant_id, event_id) "
                  "NOT VALID")


# ---------------------------------------------------------------------------
# The approaches. Each is (sql, kind of parameter); every statement binds $1
# tenant, $2 readable workareas, $3 query, $4 kinds, $5 k, as RECALL_FTS does.
# ---------------------------------------------------------------------------

SCOPE = ("m.tenant_id = %(t)s AND m.status = 'promoted' AND m.is_tip "
         "AND NOT m.retirement AND m.kind = ANY(%(kinds)s::text[]) "
         "AND m.workarea_id = ANY(%(ws)s::uuid[])")
ORQ = "replace(plainto_tsquery('simple', %(q)s)::text, '&', '|')::tsquery"
ORQ_EN = "replace(plainto_tsquery('english', %(q)s)::text, '&', '|')::tsquery"

APPROACHES = {
    # a) today, verbatim (RECALL_FTS)
    "a_simple_and": f"""SELECT m.id, m.body_bytes,
            ts_rank(to_tsvector('simple', m.claim), plainto_tsquery('simple', %(q)s), 0) AS score
        FROM memories m WHERE {SCOPE}
          AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', %(q)s)
        ORDER BY score DESC, m.id ASC LIMIT %(k)s""",
    # b) any word, ts_rank and ts_rank_cd
    "b_simple_or_rank": f"""SELECT m.id, m.body_bytes,
            ts_rank(to_tsvector('simple', m.claim), {ORQ}, 0) AS score
        FROM memories m WHERE {SCOPE}
          AND to_tsvector('simple', m.claim) @@ {ORQ}
        ORDER BY score DESC, m.id ASC LIMIT %(k)s""",
    "b_simple_or_rank_cd": f"""SELECT m.id, m.body_bytes,
            ts_rank_cd(to_tsvector('simple', m.claim), {ORQ}, 0) AS score
        FROM memories m WHERE {SCOPE}
          AND to_tsvector('simple', m.claim) @@ {ORQ}
        ORDER BY score DESC, m.id ASC LIMIT %(k)s""",
    # c) english + simple, stored generated columns, GIN each, any word
    "c_dual_or": f"""SELECT m.id, m.body_bytes,
            ts_rank(m.fts_english, {ORQ_EN}, 0) + ts_rank(m.fts_simple, {ORQ}, 0) AS score
        FROM memories m WHERE {SCOPE}
          AND (m.fts_english @@ {ORQ_EN} OR m.fts_simple @@ {ORQ})
        ORDER BY score DESC, m.id ASC LIMIT %(k)s""",
    # d) BM25 (k1 1.2, b 0.75) over the readable served rows, any word.
    # Query-time statistics: N, avgdl and df counted over exactly the rows
    # this statement may read, under row security.
    "d_bm25_querytime": f"""WITH qt AS MATERIALIZED (
            SELECT coalesce(array_agg(DISTINCT x), '{{}}') AS terms
            FROM unnest(tsvector_to_array(to_tsvector('simple', %(q)s))) x),
        st AS MATERIALIZED (SELECT count(*)::float8 AS n, coalesce(avg(m.body_bytes), 1)::float8 AS avgdl
               FROM memories m WHERE {SCOPE}),
        tf AS MATERIALIZED (SELECT m.id, m.body_bytes, x.lexeme, coalesce(array_length(x.positions, 1), 1) AS tf
                 FROM memories m CROSS JOIN qt CROSS JOIN LATERAL unnest(to_tsvector('simple', m.claim)) x
                 WHERE {SCOPE} AND to_tsvector('simple', m.claim) @@ {ORQ} AND x.lexeme = ANY(qt.terms)),
        df AS MATERIALIZED (SELECT lexeme AS term, count(*)::float8 AS df FROM tf GROUP BY lexeme)
        SELECT tf.id, tf.body_bytes, sum(ln(1 + (st.n - df.df + 0.5) / (df.df + 0.5))
               * (tf.tf * 2.2) / (tf.tf + 1.2 * (0.25 + 0.75 * tf.body_bytes / st.avgdl))) AS score
        FROM tf CROSS JOIN st JOIN df ON df.term = tf.lexeme
        GROUP BY tf.id, tf.body_bytes ORDER BY score DESC, tf.id ASC LIMIT %(k)s""",
    # Maintained statistics: per (workarea, term) document frequency and per
    # workarea N and length sum over served rows, summed over the readable
    # workareas only, so a key's idf counts only rows it may read.
    "d_bm25_maintained": f"""WITH qt AS MATERIALIZED (
            SELECT coalesce(array_agg(DISTINCT x), '{{}}') AS terms
            FROM unnest(tsvector_to_array(to_tsvector('simple', %(q)s))) x),
        st AS MATERIALIZED (SELECT coalesce(sum(s.n), 0)::float8 AS n,
                      coalesce(sum(s.len) / nullif(sum(s.n), 0), 1)::float8 AS avgdl
               FROM bm25_scope s WHERE s.tenant_id = %(t)s AND s.workarea_id = ANY(%(ws)s::uuid[])),
        df AS MATERIALIZED (SELECT d.term, sum(d.df)::float8 AS df FROM bm25_df d, qt
               WHERE d.tenant_id = %(t)s AND d.term = ANY(qt.terms)
                 AND d.workarea_id = ANY(%(ws)s::uuid[]) GROUP BY d.term),
        tf AS MATERIALIZED (SELECT m.id, m.body_bytes, x.lexeme, coalesce(array_length(x.positions, 1), 1) AS tf
                 FROM memories m CROSS JOIN qt CROSS JOIN LATERAL unnest(to_tsvector('simple', m.claim)) x
                 WHERE {SCOPE} AND to_tsvector('simple', m.claim) @@ {ORQ} AND x.lexeme = ANY(qt.terms))
        SELECT tf.id, tf.body_bytes, sum(ln(1 + (st.n - df.df + 0.5) / (df.df + 0.5))
               * (tf.tf * 2.2) / (tf.tf + 1.2 * (0.25 + 0.75 * tf.body_bytes / st.avgdl))) AS score
        FROM tf CROSS JOIN st JOIN df ON df.term = tf.lexeme
        GROUP BY tf.id, tf.body_bytes ORDER BY score DESC, tf.id ASC LIMIT %(k)s""",
    # e) trigram word similarity on the claim text and on its identifiers
    "e_trgm_text": f"""SELECT m.id, m.body_bytes, word_similarity(%(q)s, m.claim) AS score
        FROM memories m WHERE {SCOPE} AND %(q)s <%% m.claim
        ORDER BY score DESC, m.id ASC LIMIT %(k)s""",
    "e_trgm_ident": f"""SELECT m.id, m.body_bytes, word_similarity(%(q)s, m.idents) AS score
        FROM memories m WHERE {SCOPE} AND %(q)s <%% m.idents
        ORDER BY score DESC, m.id ASC LIMIT %(k)s""",
    # f) pgvector HNSW, cosine, the same scope as a filter
    "f_hnsw": """SELECT v.id FROM memory_vectors v
        WHERE v.tenant_id = %(t)s AND v.served AND v.kind = ANY(%(kinds)s::text[])
          AND v.workarea_id = ANY(%(ws)s::uuid[])
        ORDER BY v.embedding <=> %(vec)s::vector LIMIT %(k)s""",
}

# Variants over the stored simple tsvector of stage c: today's AND, any-word
# OR and maintained BM25 reading fts_simple instead of computing
# to_tsvector per row, to separate "stored column" from "english".
APPROACHES["a_stored_and"] = APPROACHES["a_simple_and"].replace(
    "to_tsvector('simple', m.claim)", "m.fts_simple")
APPROACHES["b_stored_or"] = APPROACHES["b_simple_or_rank"].replace(
    "to_tsvector('simple', m.claim)", "m.fts_simple")
APPROACHES["d_bm25_stored"] = APPROACHES["d_bm25_maintained"].replace(
    "unnest(to_tsvector('simple', m.claim))", "unnest(m.fts_simple)").replace(
    "to_tsvector('simple', m.claim) @@", "m.fts_simple @@")

# approach -> the stage (index set) it needs
NEEDS = {"c_dual_or": "dual", "d_bm25_maintained": "bm25", "e_trgm_text": "trgm",
         "e_trgm_ident": "trgm", "f_hnsw": "vector"}


# ---------------------------------------------------------------------------
# Stages: each adds an approach's columns/indexes to the partitioned table,
# timed and sized.
# ---------------------------------------------------------------------------

def stage_dual(db: str, out: dict) -> None:
    with owner(db) as c:
        t0 = time.perf_counter()
        c.execute("ALTER TABLE memories "
                  "ADD COLUMN fts_simple tsvector GENERATED ALWAYS AS (to_tsvector('simple', claim)) STORED, "
                  "ADD COLUMN fts_english tsvector GENERATED ALWAYS AS (to_tsvector('english', claim)) STORED")
        rewrite = time.perf_counter() - t0
        t1 = time.perf_counter()
        c.execute("CREATE INDEX memories_fts_simple ON memories USING gin (fts_simple)")
        b1 = time.perf_counter() - t1
        t2 = time.perf_counter()
        c.execute("CREATE INDEX memories_fts_english ON memories USING gin (fts_english)")
        b2 = time.perf_counter() - t2
        # The stored simple column duplicates today's expression index; the
        # dual set replaces it, so it is dropped here (its size is recorded in
        # the base stage).
        c.execute("DROP INDEX memories_claim_fts")
        c.execute("ANALYZE memories")
        out["dual"] = {"rewrite_s": rewrite, "gin_simple_s": b1, "gin_english_s": b2,
                       "gin_simple_bytes": part_index_bytes(c, "memories_fts_simple"),
                       "gin_english_bytes": part_index_bytes(c, "memories_fts_english"),
                       "heap_bytes_after": part_heap_bytes(c)}


def stage_trgm(db: str, out: dict) -> None:
    with owner(db) as c:
        t0 = time.perf_counter()
        c.execute("CREATE INDEX memories_claim_trgm ON memories USING gin (claim gin_trgm_ops)")
        b1 = time.perf_counter() - t0
        t1 = time.perf_counter()
        c.execute("CREATE INDEX memories_idents_trgm ON memories USING gin (idents gin_trgm_ops)")
        b2 = time.perf_counter() - t1
        out["trgm"] = {"claim_build_s": b1, "idents_build_s": b2,
                       "claim_bytes": part_index_bytes(c, "memories_claim_trgm"),
                       "idents_bytes": part_index_bytes(c, "memories_idents_trgm")}


BM25_DDL = """
CREATE TABLE bm25_df (tenant_id uuid NOT NULL, workarea_id uuid NOT NULL, term text NOT NULL,
    df integer NOT NULL, PRIMARY KEY (tenant_id, term, workarea_id));
CREATE TABLE bm25_scope (tenant_id uuid NOT NULL, workarea_id uuid NOT NULL,
    n bigint NOT NULL, len bigint NOT NULL, PRIMARY KEY (tenant_id, workarea_id));
ALTER TABLE bm25_df ENABLE ROW LEVEL SECURITY; ALTER TABLE bm25_df FORCE ROW LEVEL SECURITY;
ALTER TABLE bm25_scope ENABLE ROW LEVEL SECURITY; ALTER TABLE bm25_scope FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON bm25_df
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY tenant_isolation ON bm25_scope
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE ON bm25_df, bm25_scope TO oml_serving;
"""

BM25_TRIGGER = """
CREATE FUNCTION bm25_maintain() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE was boolean := false; now_ boolean;
BEGIN
    now_ := NEW.status = 'promoted' AND NEW.is_tip AND NOT NEW.retirement
            AND NEW.kind IN ('memory', 'derived_fact');
    IF TG_OP = 'UPDATE' THEN
        was := OLD.status = 'promoted' AND OLD.is_tip AND NOT OLD.retirement
               AND OLD.kind IN ('memory', 'derived_fact');
    END IF;
    IF now_ AND NOT was THEN
        INSERT INTO bm25_df (tenant_id, workarea_id, term, df)
            SELECT NEW.tenant_id, NEW.workarea_id, x, 1
            FROM unnest(tsvector_to_array(to_tsvector('simple', NEW.claim))) x
            ON CONFLICT (tenant_id, term, workarea_id) DO UPDATE SET df = bm25_df.df + 1;
        INSERT INTO bm25_scope VALUES (NEW.tenant_id, NEW.workarea_id, 1, NEW.body_bytes)
            ON CONFLICT (tenant_id, workarea_id) DO UPDATE
            SET n = bm25_scope.n + 1, len = bm25_scope.len + EXCLUDED.len;
    ELSIF was AND NOT now_ THEN
        UPDATE bm25_df SET df = df - 1 WHERE tenant_id = OLD.tenant_id AND workarea_id = OLD.workarea_id
            AND term = ANY(tsvector_to_array(to_tsvector('simple', OLD.claim)));
        UPDATE bm25_scope SET n = n - 1, len = len - OLD.body_bytes
            WHERE tenant_id = OLD.tenant_id AND workarea_id = OLD.workarea_id;
    END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER bm25_maintain AFTER INSERT OR UPDATE OF status, is_tip, retirement ON memories
    FOR EACH ROW EXECUTE FUNCTION bm25_maintain();
"""


def stage_bm25(db: str, tenants: list, out: dict) -> None:
    with owner(db) as c:
        c.execute(BM25_DDL)
        t0 = time.perf_counter()
        for t in tenants:
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
                c.execute(f"""INSERT INTO bm25_df (tenant_id, workarea_id, term, df)
                    SELECT m.tenant_id, m.workarea_id, x, count(*)
                    FROM memories m, unnest(tsvector_to_array(to_tsvector('simple', m.claim))) x
                    WHERE m.tenant_id = %s AND m.status = 'promoted' AND m.is_tip AND NOT m.retirement
                      AND m.kind IN ('memory', 'derived_fact')
                    GROUP BY 1, 2, 3""", (t,))
                c.execute(f"""INSERT INTO bm25_scope
                    SELECT m.tenant_id, m.workarea_id, count(*), sum(m.body_bytes) FROM memories m
                    WHERE m.tenant_id = %s AND m.status = 'promoted' AND m.is_tip AND NOT m.retirement
                      AND m.kind IN ('memory', 'derived_fact') GROUP BY 1, 2""", (t,))
        build = time.perf_counter() - t0
        c.execute(BM25_TRIGGER)
        c.execute("ANALYZE bm25_df; ANALYZE bm25_scope")
        with c.transaction():
            c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenants[0]),))
            rows = c.execute("SELECT count(*) FROM bm25_df").fetchone()[0]
        out["bm25"] = {"build_s": build, "df_rows": rows,
                       "df_total_bytes": total_size(c, "bm25_df"),
                       "scope_total_bytes": total_size(c, "bm25_scope")}


def embed_centers(seed: int = 42):
    rng = np.random.default_rng(seed)
    seeds = gen.Seeds.load()
    topics = sorted({m["topic"] for m in seeds.memories})
    tc = {t: rng.standard_normal(DIM) for t in topics}
    c = np.stack([rng.standard_normal(DIM) * 0.8 + tc[m["topic"]] for m in seeds.memories])
    return c


def vec_text(v) -> str:
    return "[" + ",".join(f"{x:.4f}" for x in v) + "]"


def stage_vector(db: str, loaded: list, out: dict, halfvec: bool = False) -> None:
    """Embeddings in their own table, list-partitioned per tenant as memories
    is, fenced by the same policy. Vectors are synthetic: the seed text's
    centre (topic centre plus a per-seed offset) plus noise, unit length, so
    neighbours cluster by seed and topic as model embeddings cluster by
    meaning; latency and size do not depend on what the numbers mean."""
    typ = "halfvec" if halfvec else "vector"
    ops = "halfvec_cosine_ops" if halfvec else "vector_cosine_ops"
    centers = embed_centers()
    rng = np.random.default_rng(7)
    with owner(db) as c:
        c.execute(f"""CREATE TABLE memory_vectors (tenant_id uuid NOT NULL, id uuid NOT NULL,
            workarea_id uuid NOT NULL, served boolean NOT NULL, kind text NOT NULL,
            embedding {typ}({DIM}) NOT NULL, PRIMARY KEY (tenant_id, id)) PARTITION BY LIST (tenant_id)""")
        c.execute("ALTER TABLE memory_vectors ENABLE ROW LEVEL SECURITY")
        c.execute("ALTER TABLE memory_vectors FORCE ROW LEVEL SECURITY")
        c.execute("CREATE POLICY tenant_isolation ON memory_vectors "
                  "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) "
                  "WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)")
        c.execute("GRANT SELECT, INSERT, UPDATE ON memory_vectors TO oml_serving")
        load_s = build_s = 0.0
        for item in loaded:
            t, p = item["tenant"], "memory_vectors_t_" + item["tenant"].hex
            c.execute(f"CREATE TABLE {p} (LIKE memory_vectors INCLUDING DEFAULTS INCLUDING CONSTRAINTS)")
            t0 = time.perf_counter()
            # read the rows on a second connection: one connection cannot
            # fetch from a cursor while it is copying in
            rd = owner(db)
            rd.autocommit = False
            rd.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(t),))
            cur = rd.cursor(name="mv")
            cur.execute("SELECT id, workarea_id, (status = 'promoted' AND is_tip AND NOT retirement), kind "
                        f"FROM {item['partition']} WHERE claim_key NOT LIKE 'w-%%' ORDER BY ctid")
            seeds_of = np.asarray(item["seeds"])
            i = 0
            with c.cursor().copy(f"COPY {p} (tenant_id, id, workarea_id, served, kind, embedding) "
                                 "FROM STDIN") as cp:
                while True:
                    batch = cur.fetchmany(5000)
                    if not batch:
                        break
                    noise = rng.standard_normal((len(batch), DIM)) * 0.9
                    vs = centers[seeds_of[i:i + len(batch)]] + noise
                    vs /= np.linalg.norm(vs, axis=1, keepdims=True)
                    for (mid, wa, served, kind), v in zip(batch, vs):
                        cp.write_row([t, mid, wa, served, kind, vec_text(v)])
                    i += len(batch)
            cur.close()
            rd.close()
            load_s += time.perf_counter() - t0
            c.execute(f"ALTER TABLE {p} ADD PRIMARY KEY (tenant_id, id)")
            c.execute("SET maintenance_work_mem = '4GB'")
            c.execute("SET max_parallel_maintenance_workers = 7")
            t0 = time.perf_counter()
            c.execute(f"CREATE INDEX {p}_hnsw ON {p} USING hnsw (embedding {ops}) WITH (m = 16, ef_construction = 64)")
            build_s += time.perf_counter() - t0
            c.execute("SET maintenance_work_mem = '1GB'")
            c.execute(f"ALTER TABLE {p} ENABLE ROW LEVEL SECURITY")
            c.execute(f"ALTER TABLE {p} FORCE ROW LEVEL SECURITY")
            c.execute(f"CREATE POLICY tenant_isolation ON {p} "
                      "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) "
                      "WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)")
            c.execute(f"ALTER TABLE memory_vectors ATTACH PARTITION {p} FOR VALUES IN ('{t}')")
            c.execute(f"ANALYZE {p}")
        out["vector"] = {"type": typ, "dim": DIM, "load_s": load_s, "hnsw_build_s": build_s,
                         "heap_bytes": c.execute("SELECT sum(pg_table_size(inhrelid)) FROM pg_inherits "
                                                 "WHERE inhparent = 'memory_vectors'::regclass").fetchone()[0],
                         "hnsw_bytes": c.execute(
                             "SELECT sum(pg_relation_size(i.indexrelid)) FROM pg_index i JOIN pg_class c "
                             "ON c.oid = i.indexrelid JOIN pg_am a ON a.oid = c.relam WHERE a.amname = 'hnsw'"
                         ).fetchone()[0]}


def part_index_bytes(c, parent_index: str) -> int:
    return c.execute("SELECT coalesce(sum(pg_relation_size(inhrelid)), 0) FROM pg_inherits "
                     "WHERE inhparent = %s::regclass", (parent_index,)).fetchone()[0]


def part_heap_bytes(c) -> int:
    return c.execute("SELECT sum(pg_table_size(inhrelid)) FROM pg_inherits "
                     "WHERE inhparent = 'memories'::regclass").fetchone()[0]


# ---------------------------------------------------------------------------
# Measurement
# ---------------------------------------------------------------------------

def pct(xs, p):
    if not xs:
        return None
    xs = sorted(xs)
    i = min(len(xs) - 1, max(0, int(round(p / 100 * len(xs) + 0.5)) - 1))
    return xs[i]


def summarize(lat: list[float], rows: list[int], timeouts: int) -> dict:
    return {"n": len(lat), "p50_ms": pct(lat, 50), "p95_ms": pct(lat, 95), "p99_ms": pct(lat, 99),
            "mean_ms": statistics.fmean(lat) if lat else None, "max_ms": max(lat) if lat else None,
            "timeouts": timeouts, "empty": sum(1 for r in rows if r == 0),
            "mean_rows": statistics.fmean(rows) if rows else None}


def serving(db: str, user: str = "oml_serving") -> psycopg.Connection:
    # prepare_threshold=0: every statement is server-prepared, as sqlx's
    # persistent statements are, so plan_cache_mode auto may pick a generic plan.
    c = psycopg.connect(dsn(user, db), prepare_threshold=0)
    c.execute(f"SET statement_timeout = {TIMEOUT_MS}")
    c.execute("SET hnsw.ef_search = 40")
    c.execute("SET hnsw.iterative_scan = relaxed_order")
    c.commit()
    return c


def run_pass(c, sqltext: str, tenant, workload: list[dict]) -> tuple[list, list, int, dict]:
    lat, rows, to = [], [], 0
    by_cat: dict[str, list[float]] = {}
    by_scope: dict[str, list[float]] = {}
    for w in workload:
        params = {"t": tenant, "ws": w["ws"], "q": w["q"], "kinds": KINDS, "k": K, "vec": w.get("vec")}
        with c.transaction():
            c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
            t0 = time.perf_counter()
            try:
                with c.transaction():
                    got = c.execute(sqltext, params).fetchall()
                ms = (time.perf_counter() - t0) * 1000
                rows.append(len(got))
            except psycopg.errors.QueryCanceled:
                ms = float(TIMEOUT_MS)
                to += 1
                rows.append(0)
        lat.append(ms)
        by_cat.setdefault(w["cat"], []).append(ms)
        by_scope.setdefault(w["scope"], []).append(ms)
    run_pass.by_scope = by_scope
    return lat, rows, to, by_cat


def measure(db: str, name: str, tenant, workload: list[dict], user: str = "oml_serving",
            cold: bool = True, warmup: bool = True) -> dict:
    sqltext = APPROACHES[name]
    res = {}
    if cold:
        restart(cold=True)
        with serving(db, user) as c:
            lat, rows, to, _ = run_pass(c, sqltext, tenant, workload)
        res["cold"] = summarize(lat, rows, to)
        res["cold"]["first_ms"] = lat[0]
    with serving(db, user) as c:
        if not cold and warmup:
            run_pass(c, sqltext, tenant, workload)  # warm-up pass
        lat, rows, to, by_cat = run_pass(c, sqltext, tenant, workload)
    res["warm"] = summarize(lat, rows, to)
    res["warm_by_category"] = {k: {"p50_ms": pct(v, 50), "p95_ms": pct(v, 95)} for k, v in by_cat.items()}
    res["warm_by_scope"] = {k: {"p50_ms": pct(v, 50), "p95_ms": pct(v, 95)}
                            for k, v in run_pass.by_scope.items()}
    return res


def explain(db: str, name: str, tenant, w: dict, user: str = "oml_serving") -> str:
    sqltext = APPROACHES[name]
    with psycopg.connect(dsn(user, db), cursor_factory=psycopg.ClientCursor) as c:
        c.execute(f"SET statement_timeout = {TIMEOUT_MS * 3}")
        c.execute("SET hnsw.ef_search = 40; SET hnsw.iterative_scan = relaxed_order")
        c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
        params = {"t": tenant, "ws": w["ws"], "q": w["q"], "kinds": KINDS, "k": K, "vec": w.get("vec")}
        try:
            plan = c.execute("EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) " + sqltext, params).fetchall()
            return "\n".join(r[0] for r in plan)
        except psycopg.errors.QueryCanceled:
            return "(timed out)"


def vector_recall(db: str, tenant, workload: list[dict], n: int = 60) -> dict:
    """recall@k of the HNSW scan against the exact top-k (index off) over the
    same filtered rows, and how often the index returned fewer than k rows."""
    got_r, short = [], 0
    sqltext = APPROACHES["f_hnsw"]
    with serving(db) as c:
        for w in workload[:n]:
            params = {"t": tenant, "ws": w["ws"], "q": w["q"], "kinds": KINDS, "k": K, "vec": w["vec"]}
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
                a = [r[0] for r in c.execute(sqltext, params).fetchall()]
                c.execute("SET LOCAL enable_indexscan = off")
                c.execute("SET LOCAL enable_bitmapscan = off")
                b = [r[0] for r in c.execute(sqltext, params).fetchall()]
            if len(a) < min(K, len(b)):
                short += 1
            if b:
                got_r.append(len(set(a) & set(b)) / len(b))
    return {"queries": len(got_r), "recall_at_k_mean": statistics.fmean(got_r) if got_r else None,
            "short_results": short}


def write_cost(db: str, tenant, tree, keys, texts: list[str], seq0: int) -> dict:
    """One write as the lead's submit makes it, as the serving role under
    row security: a journal row and a held memories row in one transaction,
    then the promotion UPDATE in another. synchronous_commit is off for the
    session so the figure is the statement and index work, not the fsync
    (the fsync is the same for every approach)."""
    leaves = tree.leaf_list()
    ins, prom = [], []
    with serving(db) as c:
        c.execute("SET synchronous_commit = off")
        c.commit()
        for i, text in enumerate(texts):
            mid, ev = uuid.uuid4(), uuid.uuid4()
            b = text.encode()
            t0 = time.perf_counter()
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
                c.execute("INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, "
                          "payload, signed_bytes_hash, sig, signed_bytes) VALUES "
                          "(%s, %s, %s, 'submission.held', %s, 'agent', '{}', %s, %s, %s)",
                          (tenant, seq0 + i, ev, b"\0" * 32, b"\1" * 32, b"\2" * 64, b"x"))
                c.execute("INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, "
                          "created_event_id, claim_key, author_principal, author_key_id, version, kind, "
                          "is_tip, body_bytes, idents) VALUES (%s, %s, %s, %s, %s, 'held', %s, %s, 'agent', "
                          "%s, 1, 'memory', false, %s, %s)",
                          (mid, tenant, random.choice(leaves), text, os.urandom(32), ev,
                           f"w-{seq0}-{i}", random.choice(keys), len(b), gen.idents_of(text)))
            ins.append((time.perf_counter() - t0) * 1000)
            t0 = time.perf_counter()
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
                c.execute("UPDATE memories SET status = 'promoted', is_tip = true, promoted_at = now() "
                          "WHERE tenant_id = %s AND id = %s", (tenant, mid))
            prom.append((time.perf_counter() - t0) * 1000)
    return {"insert": summarize(ins, [1] * len(ins), 0), "promote": summarize(prom, [1] * len(prom), 0)}


def vector_write_cost(db: str, tenant, tree, count: int = 300) -> dict:
    centers = embed_centers()
    rng = np.random.default_rng(99)
    leaves = tree.leaf_list()
    lat = []
    with serving(db) as c:
        c.execute("SET synchronous_commit = off")
        c.commit()
        for i in range(count):
            v = centers[rng.integers(len(centers))] + rng.standard_normal(DIM) * 0.9
            v /= np.linalg.norm(v)
            t0 = time.perf_counter()
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
                c.execute("INSERT INTO memory_vectors VALUES (%s, %s, %s, true, 'memory', %s::vector)",
                          (tenant, uuid.uuid4(), random.choice(leaves), vec_text(v)))
            lat.append((time.perf_counter() - t0) * 1000)
    return summarize(lat, [1] * count, 0)


# ADR-0050 d1: one any-word recall per write over the writer's readable scope
# (served promoted tips at the target workarea's recall scope, plus the key's
# own held rows there), k 3, the new text as the query.
ADR0050 = {
    "simple": (APPROACHES["b_simple_or_rank"],
               f"""SELECT m.id, m.body_bytes, ts_rank(to_tsvector('simple', m.claim), {ORQ}, 0) AS score
        FROM memories m WHERE m.tenant_id = %(t)s AND m.status = 'held' AND m.author_key_id = %(key)s
          AND NOT m.retirement AND m.kind = ANY(%(kinds)s::text[]) AND m.workarea_id = ANY(%(ws)s::uuid[])
          AND to_tsvector('simple', m.claim) @@ {ORQ}
        ORDER BY score DESC, m.id ASC LIMIT %(k)s"""),
}


def adr0050_check(db: str, tenant, tree, keys, texts: list[str], variant: str = "simple") -> dict:
    served_sql, own_sql = ADR0050[variant]
    lat, rows, to = [], [], 0
    r = random.Random(5)
    with serving(db) as c:
        for text in texts:
            leaf = r.choice(tree.leaf_list())
            params = {"t": tenant, "ws": [str(leaf), str(tree.universe)], "q": text, "kinds": KINDS,
                      "k": 3, "key": r.choice(keys)}
            t0 = time.perf_counter()
            try:
                with c.transaction():
                    c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
                    a = c.execute(served_sql, params).fetchall()
                    b = c.execute(own_sql, params).fetchall()
                rows.append(len(a) + len(b))
                lat.append((time.perf_counter() - t0) * 1000)
            except psycopg.errors.QueryCanceled:
                to += 1
                lat.append(float(TIMEOUT_MS))
                rows.append(0)
    return summarize(lat, rows, to)


def workload_for(n: int, tree, count: int, seed: int) -> list[dict]:
    qs = gen.queries(count, seed, n)
    r = random.Random(seed + 3)
    scopes = gen.readable_sets(r, tree, len(qs))
    centers = embed_centers()
    rng = np.random.default_rng(seed)
    out = []
    for q, (sk, ws) in zip(qs, scopes):
        v = centers[rng.integers(len(centers))] + rng.standard_normal(DIM) * 0.9
        v /= np.linalg.norm(v)
        out.append({"cat": q["category"], "q": q["query"], "scope": sk, "ws": ws, "vec": vec_text(v)})
    return out


def disk_free() -> int:
    st = os.statvfs("/srv/build")
    return st.f_bavail * st.f_frsize


def save(out: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(out, indent=1, default=str))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--scale", type=int, required=True, help="memories per tenant")
    ap.add_argument("--tenants", type=int, default=1)
    ap.add_argument("--queries", type=int, default=330)
    ap.add_argument("--writes", type=int, default=300)
    ap.add_argument("--out", required=True)
    ap.add_argument("--skip", default="", help="comma-separated approaches or stages to skip")
    ap.add_argument("--halfvec", action="store_true")
    ap.add_argument("--keep", action="store_true", help="keep the database at the end")
    ap.add_argument("--timeout-ms", type=int, default=10000)
    ap.add_argument("--norls-one-pass", action="store_true",
                    help="no-RLS comparison: one pass, the RLS passes having warmed the heap")
    a = ap.parse_args()
    skip = set(filter(None, a.skip.split(",")))
    global TIMEOUT_MS
    TIMEOUT_MS = a.timeout_ms
    out_path = HERE / a.out
    db = f"oml_scale_{a.scale}_{a.tenants}"
    r = random.Random(a.scale * 31 + a.tenants)
    out = {"scale": a.scale, "tenants": a.tenants, "k": K, "queries": a.queries,
           "timeout_ms": TIMEOUT_MS, "norls_one_pass": a.norls_one_pass,
           "started": time.strftime("%Y-%m-%dT%H:%M:%S"), "load": [], "stages": {},
           "latency": {}, "latency_no_rls": {}, "write": {}, "explain": {}, "disk_free_start": disk_free()}
    log = lambda *x: print(time.strftime("%H:%M:%S"), *x, flush=True)  # noqa: E731

    log("create", db)
    create_db(db)
    loaded = []
    for i in range(a.tenants):
        t, tree, keys = setup_tenant(db, r, a.scale, f"tenant{i}")
        st = {"tenant": str(t)}
        info = load_tenant(db, t, tree, keys, a.scale, seed=a.scale + i, stats=st)
        loaded.append({"tenant": t, "tree": tree, "keys": keys, **info})
        out["load"].append(st)
        log("loaded tenant", i, st["copy_s"], "s")
    restore_fk(db)
    with su(db) as c:
        c.execute("VACUUM ANALYZE")
    out["workareas"] = len(loaded[0]["tree"].all())
    # The measured tenant is the first; in a multi-tenant run the others are
    # its neighbours in the same partitioned table.
    m = loaded[0]
    t, tree, keys = m["tenant"], m["tree"], m["keys"]
    work = workload_for(a.scale, tree, a.queries, seed=a.scale)
    out["workload"] = {"categories": sorted({w["cat"] for w in work}),
                       "scopes": {s: sum(1 for w in work if w["scope"] == s) for s in ("leaf", "project", "universe")}}
    with su(db) as c:
        c.execute("SELECT set_config('oml.tenant_id', %s, false)", (str(t),))
        out["served_rows"] = c.execute(
            f"SELECT count(*) FROM memories m WHERE {SCOPE.replace('%(t)s', '%s').replace('%(kinds)s', '%s').replace('%(ws)s', '%s')}",
            (t, KINDS, [str(x) for x in tree.all()])).fetchone()[0]
    ex_pick = {}
    for cat in ("exact", "long", "identifier", "typo"):
        ex_pick[cat] = next(w for w in work if w["cat"] == cat and w["scope"] == "universe") \
            if any(w["cat"] == cat and w["scope"] == "universe" for w in work) else next(w for w in work if w["cat"] == cat)
    save(out, out_path)

    def run(name: str, no_rls: bool = False) -> None:
        if name in skip:
            return
        log("measure", name)
        out["latency"][name] = measure(db, name, t, work)
        out["explain"][name] = {cat: explain(db, name, t, w) for cat, w in ex_pick.items()}
        if no_rls:
            # The same statement as a role row security does not bind (the
            # superuser): what the plan would be if the predicate could use
            # its index under RLS.
            out["latency_no_rls"][name] = measure(db, name, t, work, user="postgres", cold=False,
                                                     warmup=not a.norls_one_pass)
            out["explain"][name + "@no_rls"] = {cat: explain(db, name, t, w, user="postgres")
                                                for cat, w in ex_pick.items()}
        save(out, out_path)

    seq = 1_000_000
    texts = gen.new_texts(a.writes, seed=a.scale + 999, n=a.scale)

    # stage: base (today's indexes)
    run("a_simple_and", no_rls=True)
    run("b_simple_or_rank", no_rls=True)
    run("b_simple_or_rank_cd")
    run("d_bm25_querytime")
    log("adr0050")
    out["adr0050"] = {"base": adr0050_check(db, t, tree, keys, texts[:a.writes])}
    log("write base")
    out["write"]["base"] = write_cost(db, t, tree, keys, texts, seq); seq += a.writes
    save(out, out_path)

    if a.tenants > 1 and "flat" not in skip:
        # The pre-0009 shape: one table for every tenant, one GIN, the same
        # policy; the measured tenant's statements read it instead.
        log("stage flat")
        with owner(db) as c:
            c.execute("CREATE TABLE memories_flat (LIKE memories INCLUDING DEFAULTS INCLUDING CONSTRAINTS)")
            c.execute("ALTER TABLE memories_flat ENABLE ROW LEVEL SECURITY")
            c.execute("ALTER TABLE memories_flat FORCE ROW LEVEL SECURITY")
            c.execute("CREATE POLICY tenant_isolation ON memories_flat "
                      "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) "
                      "WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)")
            c.execute("GRANT SELECT ON memories_flat TO oml_serving")
            for x in loaded:
                with c.transaction():
                    c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(x["tenant"]),))
                    c.execute("INSERT INTO memories_flat SELECT * FROM memories WHERE tenant_id = %s", (x["tenant"],))
            t0 = time.perf_counter()
            c.execute("CREATE INDEX memories_flat_fts ON memories_flat USING gin (to_tsvector('simple', claim))")
            gin_s = time.perf_counter() - t0
            c.execute("CREATE INDEX memories_flat_workarea ON memories_flat (tenant_id, workarea_id, status)")
            c.execute("CREATE UNIQUE INDEX memories_flat_served_tip ON memories_flat (tenant_id, workarea_id, claim_key) "
                      "WHERE is_tip AND status = 'promoted'")
            c.execute("ANALYZE memories_flat")
            out["stages"]["flat"] = {"rows": sum(1 for _ in loaded) * a.scale, "gin_build_s": gin_s,
                                     "gin_bytes": rel_size(c, "memories_flat_fts"),
                                     "heap_bytes": rel_size(c, "memories_flat")}
        for base in ("a_simple_and", "b_simple_or_rank"):
            APPROACHES[base + "@flat"] = APPROACHES[base].replace("FROM memories m", "FROM memories_flat m")
            run(base + "@flat", no_rls=True)
        with su(db) as c:
            c.execute("DROP TABLE memories_flat")
    if "bm25" not in skip:
        log("stage bm25"); stage_bm25(db, [x["tenant"] for x in loaded], out["stages"])
        run("d_bm25_maintained")
        out["bm25_rls_check"] = bm25_rls_check(db, t, tree, work[:40])
        out["write"]["+bm25_trigger"] = write_cost(db, t, tree, keys, texts, seq); seq += a.writes
        with owner(db) as c:  # the trigger's cost is measured alone, then removed
            c.execute("DROP TRIGGER bm25_maintain ON memories")
        save(out, out_path)
    if "dual" not in skip:
        log("stage dual"); stage_dual(db, out["stages"])
        run("c_dual_or", no_rls=True)
        run("a_stored_and", no_rls=True)
        run("b_stored_or")
        if "bm25" not in skip:
            run("d_bm25_stored")
        out["write"]["+dual"] = write_cost(db, t, tree, keys, texts, seq); seq += a.writes
        save(out, out_path)
    if "trgm" not in skip:
        log("stage trgm"); stage_trgm(db, out["stages"])
        run("e_trgm_text", no_rls=True)
        run("e_trgm_ident", no_rls=True)
        out["write"]["+trgm"] = write_cost(db, t, tree, keys, texts, seq); seq += a.writes
        save(out, out_path)
    if "vector" not in skip:
        log("stage vector", "free", disk_free() // 2**20, "MiB")
        stage_vector(db, loaded, out["stages"], halfvec=a.halfvec)
        if a.halfvec:
            APPROACHES["f_hnsw"] = APPROACHES["f_hnsw"].replace("::vector", "::halfvec")
        run("f_hnsw")
        out["vector_recall"] = vector_recall(db, t, work)
        out["write"]["vector_insert"] = vector_write_cost(db, t, tree, a.writes)
        save(out, out_path)

    with su(db) as c:
        out["db_bytes"] = c.execute("SELECT pg_database_size(current_database())").fetchone()[0]
    out["disk_free_end"] = disk_free()
    out["finished"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    save(out, out_path)
    if not a.keep:
        with su() as c:
            c.execute(S.SQL("DROP DATABASE {} WITH (FORCE)").format(S.Identifier(db)))
    log("done")


def bm25_rls_check(db: str, tenant, tree, work: list[dict]) -> dict:
    """Do the two BM25 statistics agree, and do they count only readable rows?
    For each query: df per term from the maintained counts summed over the
    readable workareas, from the query-time count under row security, and from
    a tenant-wide count (every workarea, the form that would leak)."""
    agree = differs_tenant_wide = terms = 0
    with serving(db) as c:
        for w in work:
            with c.transaction():
                c.execute("SELECT set_config('oml.tenant_id', %s, true)", (str(tenant),))
                qterms = [r[0] for r in c.execute(
                    "SELECT DISTINCT unnest(tsvector_to_array(to_tsvector('simple', %s)))", (w["q"],))]
                for term in qterms:
                    p = {"t": tenant, "ws": w["ws"], "kinds": KINDS, "term": term,
                         "all": [str(x) for x in tree.all()]}
                    mc = c.execute("SELECT coalesce(sum(df), 0) FROM bm25_df WHERE tenant_id = %(t)s "
                                   "AND term = %(term)s AND workarea_id = ANY(%(ws)s::uuid[])", p).fetchone()[0]
                    qt = c.execute(f"SELECT count(*) FROM memories m WHERE {SCOPE} AND "
                                   "to_tsvector('simple', m.claim) @@ format('%%L', %(term)s::text)::tsquery",
                                   {**p, "q": None}).fetchone()[0]
                    tw = c.execute("SELECT coalesce(sum(df), 0) FROM bm25_df WHERE tenant_id = %(t)s "
                                   "AND term = %(term)s AND workarea_id = ANY(%(all)s::uuid[])", p).fetchone()[0]
                    terms += 1
                    agree += int(mc == qt)
                    differs_tenant_wide += int(tw != qt)
    return {"terms": terms, "maintained_equals_querytime": agree, "tenant_wide_differs": differs_tenant_wide}


if __name__ == "__main__":
    main()
