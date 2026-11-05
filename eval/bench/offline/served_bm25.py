#!/usr/bin/env python3
"""The served `bm25` ranking, with its scores, from a throwaway Postgres.

    served_bm25.py --dsn DSN --runs DIR <dataset>...

The served run files (bench.py run) keep each query's ranked ids but not the
scores, and a convex combination needs the scores. This script loads each
dataset's memories into a scratch table of a throwaway cluster, computes
`search_vector`/`search_len` with migration 0020's own functions, and runs
`oml-pg`'s `bm25_sql()` statement with the scope reduced to the query's group
(the served scope is a workarea: one dataset, or one LoCoMo conversation).
No server, no row security, no review flow: the statement and the stored
lexemes are the served ones, so the ranking matches the served run up to ties
broken by a fresh uuid. Writes DIR/<dataset>_bm25.json: {qid: [[doc_id, score], ...]}
with at most 100 hits per query.
"""

import argparse
import json
import re
import sys
import uuid
from pathlib import Path

import psycopg

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE.parent))
from adapters import ADAPTERS  # noqa: E402

CACHE = "/srv/build/bench-data"
K = 100


def migration_functions():
    """The five search functions of migrations/0020_search.sql, verbatim."""
    sql = (ROOT / "migrations" / "0020_search.sql").read_text()
    names = ["oml_search_split", "oml_search_text", "oml_search_exact", "oml_search_stems",
             "oml_search_vector"]
    out = []
    for n in names:
        m = re.search(rf"CREATE FUNCTION {n}\(.*?;\n", sql, re.S)
        out.append(m.group(0))
    return out


def bm25_sql():
    """oml-pg's bm25_sql(), with `$n` placeholders as psycopg's %(name)s and
    SEARCH_SCOPE as the group. Kept textually close to crates/adapters/oml-pg/src/lib.rs."""
    words = ("words AS (SELECT DISTINCT w, ts_lexize('english_stem', w) = '{}'::text[] AS stop "
             "FROM unnest(tsvector_to_array(to_tsvector('simple', oml_search_text(%(q)s)))) w), "
             "kept AS (SELECT w FROM words WHERE NOT stop OR NOT EXISTS "
             "(SELECT 1 FROM words WHERE NOT stop))")
    scope = "m.grp = %(g)s"
    return (
        f"WITH {words}, "
        "forms AS (SELECT DISTINCT kept.w, f FROM kept "
        "CROSS JOIN LATERAL unnest(array_append(COALESCE(ts_lexize('english_stem', kept.w), "
        "'{}'::text[]), kept.w)) f), "
        "q AS (SELECT string_agg(DISTINCT '''' || replace(replace(f, '\\', '\\\\'), '''', '''''') "
        "|| '''', ' | ')::tsquery AS tq, array_agg(DISTINCT f) AS fa FROM forms), "
        "stats AS (SELECT count(*)::float8 AS n, "
        "GREATEST(COALESCE(avg(m.search_len), 1), 1)::float8 "
        f"AS avgdl FROM memories m WHERE {scope}), "
        "tf AS MATERIALIZED (SELECT m.id, m.doc_id, m.search_len::float8 AS dl, u.lexeme, "
        "GREATEST(cardinality(u.positions), 1)::float8 AS tf FROM memories m CROSS JOIN q "
        "CROSS JOIN LATERAL unnest(ts_filter(setweight(m.search_vector, 'A', q.fa), '{a}')) u "
        f"WHERE {scope} AND m.search_vector @@ q.tq), "
        "df AS (SELECT lexeme, count(*)::float8 AS df FROM tf GROUP BY lexeme), "
        "per AS (SELECT tf.id, tf.doc_id, forms.w, "
        "max(ln(1 + (s.n - df.df + 0.5) / (df.df + 0.5)) "
        "* tf.tf * 2.2 / (tf.tf + 1.2 * (0.25 + 0.75 * tf.dl / s.avgdl))) AS c "
        "FROM tf JOIN df USING (lexeme) JOIN forms ON forms.f = tf.lexeme CROSS JOIN stats s "
        "GROUP BY tf.id, tf.doc_id, forms.w), "
        "scored AS (SELECT id, doc_id, sum(c) AS score FROM per GROUP BY id, doc_id) "
        "SELECT doc_id, score::float4 AS score FROM scored "
        "ORDER BY score DESC, id ASC LIMIT %(k)s"
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dsn", required=True)
    ap.add_argument("--runs", required=True)
    ap.add_argument("--seed", type=int, default=20261104)
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()
    out = Path(a.runs)
    out.mkdir(parents=True, exist_ok=True)
    with psycopg.connect(a.dsn, autocommit=True) as c:
        c.execute("DROP TABLE IF EXISTS memories")
        for f in ["oml_search_vector", "oml_search_stems", "oml_search_exact",
                  "oml_search_text", "oml_search_split"]:
            c.execute(f"DROP FUNCTION IF EXISTS {f}(text)")
        for f in migration_functions():
            c.execute(f)
        c.execute("CREATE TABLE memories (id uuid PRIMARY KEY, grp text NOT NULL, "
                  "doc_id text NOT NULL, claim text NOT NULL, search_vector tsvector, "
                  "search_len integer)")
        sql = bm25_sql()
        for ds in a.datasets:
            b = ADAPTERS[ds]().load(CACHE, a.seed, None)
            c.execute("TRUNCATE memories")
            with c.cursor() as cur:
                with cur.copy("COPY memories (id, grp, doc_id, claim) FROM STDIN") as cp:
                    for d in b.docs:
                        cp.write_row((uuid.uuid4(), d.group, d.id, d.text))
            c.execute("UPDATE memories SET search_vector = oml_search_vector(claim)")
            c.execute("UPDATE memories SET search_len = length(search_vector)")
            c.execute("ANALYZE memories")
            runs = {}
            for q in b.queries:
                rows = c.execute(sql, {"q": q.text, "g": q.group, "k": K}).fetchall()
                runs[q.id] = [[d, float(s)] for d, s in rows]
            (out / f"{ds}_bm25.json").write_text(json.dumps(runs))
            print(ds, len(runs), flush=True)


if __name__ == "__main__":
    main()
