#!/usr/bin/env python3
"""The served lexemes and the learned sparse vectors that score.py reads (ADR-0075 phase 0).

    prepare.py --dsn DSN --questions FILE --sparse-dir DIR --out CACHE.json

Lexemes come from a throwaway PostgreSQL with migration 0020's own functions,
so the text field is exactly what `bm25-postings` indexes: per memory,
`oml_search_vector(text)` with each lexeme's occurrence count and the vector's
length, and its `english` forms (`oml_search_stems`) for d6's grounding; per
query, the kept words and their forms, as `bm25_sql()` derives them; per
generated question, its `oml_search_vector` lexemes (the question field) and
its `english` content lexemes (d6's filters). Nothing is written to a table:
every value is a SELECT over a literal, under the five functions alone.

The sparse leg is `sparse` as served (ADR-0052 d13): each memory encoded by
opensearch-neural-sparse-encoding-doc-v3-distill (the model card's method, as
eval/bench/offline/encode.py), each query its tokenizer ids weighted by the
card's idf.json. Writes one JSON cache, which score.py reads and nobody
commits. Needs psycopg, torch and transformers.
"""

import argparse
import json
import sys
from pathlib import Path

import psycopg

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "eval/bench/offline"))
from served_bm25 import migration_functions  # noqa: E402

KEPT = """
WITH words AS (SELECT DISTINCT w, ts_lexize('english_stem', w) = '{}'::text[] AS stop
               FROM unnest(tsvector_to_array(to_tsvector('simple', oml_search_text(%(q)s)))) w),
     kept AS (SELECT w FROM words WHERE NOT stop OR NOT EXISTS (SELECT 1 FROM words WHERE NOT stop))
SELECT kept.w, array_agg(DISTINCT f ORDER BY f) FROM kept
CROSS JOIN LATERAL unnest(array_append(COALESCE(ts_lexize('english_stem', kept.w), '{}'::text[]),
                                       kept.w)) f
GROUP BY kept.w ORDER BY kept.w
"""


def vector(cur, text):
    cur.execute("SELECT lexeme, GREATEST(cardinality(positions), 1) FROM "
                "unnest(oml_search_vector(%s))", (text,))
    return dict(cur.fetchall())


def lexemes(cur, fn, text):
    cur.execute(f"SELECT coalesce(tsvector_to_array({fn}), '{{}}')", (text,))
    return sorted(cur.fetchone()[0])


def queries():
    syn = json.loads((ROOT / "eval/recall/queries.json").read_text())["queries"]
    grown = json.loads((ROOT / "eval/recall/queries-grown.json").read_text())["queries"]
    out = []
    for q in syn:
        if q["category"] == "unanswerable":
            continue
        out.append({"id": q["id"], "set": "synthetic", "category": q["category"],
                    "query": q["query"], "relevant": q["relevant"], "fold": None})
    for q in grown:
        out.append({"id": q["id"], "set": "grown", "category": q["category"],
                    "query": q["query"], "relevant": q["relevant"], "fold": q["fold"]})
    return out


def sparse(model_dir, docs, qs):
    import torch
    from transformers import AutoModelForMaskedLM, AutoTokenizer

    torch.set_num_threads(4)
    tok = AutoTokenizer.from_pretrained(model_dir)
    model = AutoModelForMaskedLM.from_pretrained(model_dir).eval()
    special = sorted(set(tok.all_special_ids))
    idf_raw = json.loads((Path(model_dir) / "idf.json").read_text())
    idf = {tok.convert_tokens_to_ids(t): w for t, w in idf_raw.items()}
    dv = {}
    with torch.inference_mode():
        for mid, text in docs:
            f = tok([text], truncation=True, max_length=512, return_tensors="pt",
                    return_token_type_ids=False)
            logits = model(**f)[0]
            v, _ = torch.max(logits * f["attention_mask"].unsqueeze(-1), dim=1)
            v = torch.log(1 + torch.log(1 + torch.relu(v)))[0]
            v[special] = 0
            nz = torch.nonzero(v).flatten().tolist()
            dv[mid] = {str(i): float(v[i]) for i in nz}
    qv = {}
    for q in qs:
        ids = {i for i in tok(q["query"], truncation=True, max_length=512)["input_ids"]
               if i not in special}
        qv[q["id"]] = {str(i): float(idf.get(i, 0.0)) for i in sorted(ids)}
    return dv, qv


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dsn", required=True)
    ap.add_argument("--questions", required=True, nargs="+")
    ap.add_argument("--sparse-dir", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    corpus = json.loads((ROOT / "eval/recall/corpus.json").read_text())["memories"]
    qs = queries()
    gen = {}
    for path in a.questions:
        for line in open(path):
            r = json.loads(line)
            if "id" in r:
                gen.setdefault(r["id"], []).extend(r["questions"])
    with psycopg.connect(a.dsn, autocommit=True) as conn, conn.cursor() as cur:
        for fn in migration_functions():
            cur.execute(fn.replace("CREATE FUNCTION", "CREATE OR REPLACE FUNCTION"))
        docs = {}
        for m in corpus:
            v = vector(cur, m["text"])
            docs[m["id"]] = {"tf": v, "len": len(v),
                             "english": lexemes(cur, "oml_search_stems(%s)", m["text"])}
        for q in qs:
            cur.execute(KEPT, {"q": q["query"]})
            q["kept"] = [[w, forms] for w, forms in cur.fetchall()]
        quest = {}
        for t in sorted({t for v in gen.values() for t in v}):
            quest[t] = {"lexemes": sorted(vector(cur, t)),
                        "content": lexemes(cur, "to_tsvector('english'::regconfig, %s)", t)}
    dv, qv = sparse(a.sparse_dir, [(m["id"], m["text"]) for m in corpus], qs)
    Path(a.out).write_text(json.dumps({"docs": docs, "queries": qs, "questions": quest,
                                       "sparse_docs": dv, "sparse_queries": qv}))


if __name__ == "__main__":
    main()
