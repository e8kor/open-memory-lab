#!/usr/bin/env python3
"""Offline baselines over the same corpus and queries as run.py.

Needs a scratch venv, never the repository's or the system's:

    uv venv /srv/build/eval-venv
    uv pip install --python /srv/build/eval-venv/bin/python \
        rank_bm25 snowballstemmer fastembed "psycopg[binary]"
    /srv/build/eval-venv/bin/python eval/recall/baselines.py \
        --out eval/recall/results [--pg-dsn postgresql://...] [--methods ...]

Methods (each writes <out>/<method>.json, scored by metrics.py, k = 10):

- bm25: rank_bm25's BM25Okapi over lower-cased alphanumeric tokens, English
  stop words dropped, Snowball English stemming.
- dense: cosine similarity of fastembed BAAI/bge-small-en-v1.5 embeddings,
  the query prefixed with BGE's retrieval instruction.
- hybrid_rrf: reciprocal rank fusion (k = 60) of the bm25 and dense top 100.
- pg_simple_and: the statement OML serves, run directly against the loaded
  database: plainto_tsquery('simple'), ts_rank normalization 0. A check that
  the offline harness reproduces the server.
- pg_simple_or: the served 'simple' configuration with the query terms joined
  by OR instead of AND, ranked by ts_rank_cd: what OR semantics alone buys.
- pg_simple_and_or: the served AND statement, falling back to pg_simple_or
  only when AND finds nothing, so every query AND answers is unchanged.
- pg_english_or: to_tsvector('english') with the websearch_to_tsquery terms
  joined by OR instead of AND, ranked by ts_rank_cd: keyword search with
  stemming, stop words and OR semantics, no new extension.
- pg_english_or_trgm: pg_english_or fused (RRF, k = 60) with a pg_trgm
  word_similarity ranking of the claim text, for typos and fragments.

The pg_* methods read the `memories` table of the throwaway database run.py
loaded, rows with claim_key 'recall-eval-<id>', promoted tips only, as a
superuser (row security does not apply), so they rank the same 200 rows.
"""

import argparse
import json
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402

K = 10
DEPTH = 100
RRF_K = 60

STOP = set("""a about above after again against all am an and any are as at be because been before
being below between both but by can could did do does doing down during each few for from further
had has have having he her here hers him his how i if in into is it its itself just me more most my
no nor not of off on once only or other our ours out over own same she should so some such than that
the their them then there these they this those through to too under until up very was we were what
when where which while who whom why will with would you your yours""".split())


def tokens(text, stem):
    return [stem(t) for t in re.findall(r"[a-z0-9]+", text.lower()) if t not in STOP]


def bm25_ranker(memories):
    import snowballstemmer
    from rank_bm25 import BM25Okapi

    stemmer = snowballstemmer.stemmer("english")
    stem = stemmer.stemWord
    index = BM25Okapi([tokens(m["text"], stem) for m in memories])
    ids = [m["id"] for m in memories]

    def rank(query):
        scores = index.get_scores(tokens(query, stem))
        order = sorted(range(len(ids)), key=lambda i: (-scores[i], ids[i]))
        return [ids[i] for i in order[:DEPTH] if scores[i] > 0]

    return rank


def dense_ranker(memories, cache):
    import numpy as np
    from fastembed import TextEmbedding

    model = TextEmbedding("BAAI/bge-small-en-v1.5", cache_dir=cache)
    ids = [m["id"] for m in memories]
    docs = np.array(list(model.embed([m["text"] for m in memories])))
    docs /= np.linalg.norm(docs, axis=1, keepdims=True)
    prefix = "Represent this sentence for searching relevant passages: "

    def rank(query):
        q = np.array(list(model.embed([prefix + query]))[0])
        q /= np.linalg.norm(q)
        sims = docs @ q
        order = np.argsort(-sims, kind="stable")
        return [ids[i] for i in order[:DEPTH]]

    return rank


def rrf(*lists):
    score = {}
    for ranked in lists:
        for i, m in enumerate(ranked):
            score[m] = score.get(m, 0.0) + 1.0 / (RRF_K + i + 1)
    return [m for m, _ in sorted(score.items(), key=lambda kv: (-kv[1], kv[0]))]


PG_ROWS = """FROM memories m WHERE m.claim_key LIKE 'recall-eval-%%' AND m.status = 'promoted'
             AND m.is_tip AND NOT m.retirement"""

PG_SQL = {
    "pg_simple_and": f"""
        SELECT substr(m.claim_key, 13) FROM (SELECT m.*,
               ts_rank(to_tsvector('simple', m.claim), plainto_tsquery('simple', %(q)s), 0) AS score
               {PG_ROWS} AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', %(q)s)) m
        ORDER BY score DESC, m.id ASC LIMIT {DEPTH}""",
    "pg_english_or": f"""
        WITH q AS (SELECT NULLIF(replace(websearch_to_tsquery('english', %(q)s)::text,
                                         ' & ', ' | '), '')::tsquery AS tq)
        SELECT substr(m.claim_key, 13) FROM (SELECT m.claim_key,
               ts_rank_cd(to_tsvector('english', m.claim), (SELECT tq FROM q)) AS score
               {PG_ROWS} AND to_tsvector('english', m.claim) @@ (SELECT tq FROM q)) m
        ORDER BY score DESC, m.claim_key ASC LIMIT {DEPTH}""",
    "pg_simple_or": f"""
        WITH q AS (SELECT NULLIF(replace(websearch_to_tsquery('simple', %(q)s)::text,
                                         ' & ', ' | '), '')::tsquery AS tq)
        SELECT substr(m.claim_key, 13) FROM (SELECT m.claim_key,
               ts_rank_cd(to_tsvector('simple', m.claim), (SELECT tq FROM q)) AS score
               {PG_ROWS} AND to_tsvector('simple', m.claim) @@ (SELECT tq FROM q)) m
        ORDER BY score DESC, m.claim_key ASC LIMIT {DEPTH}""",
    "pg_trgm": f"""
        SELECT substr(m.claim_key, 13) FROM (SELECT m.*, word_similarity(%(q)s, m.claim) AS score
               {PG_ROWS}) m
        WHERE m.score > 0.3 ORDER BY m.score DESC, m.claim_key ASC LIMIT {DEPTH}""",
}


def pg_ranker(dsn, name):
    import psycopg

    conn = psycopg.connect(dsn, autocommit=True)
    if name == "pg_trgm":
        conn.execute("CREATE EXTENSION IF NOT EXISTS pg_trgm")
    sql = PG_SQL[name]

    def rank(query):
        return [r[0] for r in conn.execute(sql, {"q": query}).fetchall()]

    return rank


def run(name, rank, queries, description):
    per_query = {}
    for q in queries:
        t0 = time.perf_counter()
        ranked = rank(q["query"])
        ms = (time.perf_counter() - t0) * 1000
        per_query[q["id"]] = {"ranked": ranked[:K], "latency_ms": round(ms, 2)}
    summary = metrics.aggregate(per_query, queries)
    return {"method": name, "description": description, "params": {"k": K},
            "summary": summary, "per_query": per_query}


DESCRIPTIONS = {
    "bm25": "BM25Okapi, Snowball English stems, English stop words dropped",
    "dense": "fastembed BAAI/bge-small-en-v1.5, cosine",
    "hybrid_rrf": "RRF (k=60) of bm25 and dense top 100",
    "pg_simple_and": "the served statement: plainto_tsquery('simple') AND, ts_rank",
    "pg_simple_or": "to_tsvector('simple') kept, websearch terms joined by OR, ts_rank_cd",
    "pg_simple_and_or": "the served AND statement; when it finds nothing, pg_simple_or",
    "pg_english_or": "to_tsvector('english'), websearch terms joined by OR, ts_rank_cd",
    "pg_english_or_trgm": "RRF (k=60) of pg_english_or and pg_trgm word_similarity > 0.3",
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--pg-dsn", default=None)
    ap.add_argument("--embed-cache", default=None)
    ap.add_argument("--methods", nargs="*", default=list(DESCRIPTIONS))
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    memories, queries = metrics.load()

    rankers = {}
    need = set(a.methods)
    if need & {"bm25", "hybrid_rrf"}:
        rankers["bm25"] = bm25_ranker(memories)
    if need & {"dense", "hybrid_rrf"}:
        rankers["dense"] = dense_ranker(memories, a.embed_cache)
    if "hybrid_rrf" in need:
        b, d = rankers["bm25"], rankers["dense"]
        rankers["hybrid_rrf"] = lambda q: rrf(b(q), d(q))
    if any(m.startswith("pg_") for m in need):
        if not a.pg_dsn:
            raise SystemExit("the pg_* methods need --pg-dsn")
        for name in ("pg_simple_and", "pg_simple_or", "pg_english_or", "pg_trgm"):
            rankers[name] = pg_ranker(a.pg_dsn, name)
        sa, so = rankers["pg_simple_and"], rankers["pg_simple_or"]
        rankers["pg_simple_and_or"] = lambda q: sa(q) or so(q)
        o, t = rankers["pg_english_or"], rankers["pg_trgm"]
        rankers["pg_english_or_trgm"] = lambda q: rrf(o(q), t(q))

    for name in a.methods:
        result = run(name, rankers[name], queries, DESCRIPTIONS[name])
        (out / f"{name}.json").write_text(json.dumps(result, indent=1))
        print(name, json.dumps(result["summary"]["overall"]))


if __name__ == "__main__":
    main()
