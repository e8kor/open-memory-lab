#!/usr/bin/env python3
"""Ontology arms of the recall eval: synonym expansion through term labels and a term match.

Reads ../corpus.json, ../queries.json, vocabulary.json and tags.txt, and writes one JSON per arm
to results/, plus results/tables.md. Needs the eval's scratch venv for BM25:

    /srv/build/eval-venv/bin/python eval/recall/ontology/run.py --pg-dsn postgresql://...

Arms (k = 10, scored by ../metrics.py):

- any:            ../baselines.py's pg_simple_or exactly, on a throwaway PostgreSQL: the
                  served 'simple' index, the query's words joined by OR, ts_rank_cd. Expansion
                  words go in a second OR query whose rank is added at half weight.
- bm25:           ../baselines.py's bm25, unchanged.
- <base>+syn:     the query's words, plus every label of every term whose label occurs in the
                  query, the added words at half weight.
- <base>+syn+term: <base>+syn fused by reciprocal rank (k = 60) with a term list: memories
                  tagged with a matched term, more matched terms first, then <base>+syn order.
- <base>+syn+boost: <base>+syn's scores, normalised to the top hit, plus 0.15 for each matched
                  term the memory is tagged with; a tagged memory outside the list enters at
                  its boost alone.
- <base>+term:    <base> fused with the term list, with no synonym expansion of the text.
- <base>+rareterm: <base>+term, counting only matched terms that tag at most 10 memories (5%),
                  so a broad term such as topic:invoicing contributes no list.

Tag sets: curated (tags.txt), lexical (the three terms whose labels occur most often in the
memory's text: what a server-side lexical matcher would attach with no judgement), noisy
(curated, each tag dropped with p = 0.3 and replaced by a random term with p = 0.2, seed 7).
"""

import json
import random
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import metrics  # noqa: E402

from baselines import STOP, bm25_ranker  # noqa: E402

K = 10
DEPTH = 100
RRF_K = 60
SYN_WEIGHT = 0.5
BOOST = 0.15
RARE_MAX = 10


def words(text):
    return re.findall(r"[a-z0-9]+", text.lower())


def load_vocab():
    v = json.loads((HERE / "vocabulary.json").read_text())
    return {t["id"]: [t["pref"]] + t["alt"] for t in v["terms"]}


def load_tags():
    tags = {}
    for line in (HERE / "tags.txt").read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        m, _, rest = line.partition(":")
        tags[m.strip()] = rest.split()
    return tags


class LabelMatcher:
    """Finds the terms whose labels occur in a text, as stemmed contiguous word sequences."""

    def __init__(self, vocab):
        import snowballstemmer

        self.stem = snowballstemmer.stemmer("english").stemWord
        self.labels = []
        for tid, labels in vocab.items():
            for label in labels:
                seq = tuple(self.stem(w) for w in words(label))
                if seq:
                    self.labels.append((seq, tid, label))

    def matches(self, text):
        toks = [self.stem(w) for w in words(text)]
        found = {}
        for seq, tid, label in self.labels:
            n = len(seq)
            hits = sum(1 for i in range(len(toks) - n + 1) if tuple(toks[i:i + n]) == seq)
            if hits:
                found.setdefault(tid, []).append((label, hits))
        return found


PG_TABLE = """CREATE TABLE IF NOT EXISTS memories (id serial, claim_key text, claim text,
    status text DEFAULT 'promoted', is_tip boolean DEFAULT true, retirement boolean DEFAULT false)"""

PG_ANY = """
    WITH q AS (SELECT NULLIF(replace(websearch_to_tsquery('simple', %(q)s)::text,
                                     ' & ', ' | '), '')::tsquery AS tq,
                      NULLIF(%(x)s, '')::tsquery AS xq)
    SELECT substr(m.claim_key, 13),
           coalesce(ts_rank_cd(to_tsvector('simple', m.claim), (SELECT tq FROM q)), 0)
           + %(w)s * coalesce(ts_rank_cd(to_tsvector('simple', m.claim), (SELECT xq FROM q)), 0)
    FROM memories m
    WHERE m.claim_key LIKE 'recall-eval-%%' AND m.status = 'promoted' AND m.is_tip
      AND NOT m.retirement
      AND (to_tsvector('simple', m.claim) @@ (SELECT tq FROM q)
           OR to_tsvector('simple', m.claim) @@ (SELECT xq FROM q))
    ORDER BY 2 DESC, m.claim_key ASC LIMIT 100"""


def any_ranker(memories, dsn):
    """../baselines.py's pg_simple_or, on a throwaway PostgreSQL, with an optional second,
    half-weight OR query of expansion words. Loads the corpus when the table is empty."""
    import psycopg

    conn = psycopg.connect(dsn, autocommit=True)
    conn.execute(PG_TABLE)
    if conn.execute("SELECT count(*) FROM memories").fetchone()[0] == 0:
        with conn.cursor() as cur:
            cur.executemany("INSERT INTO memories (claim_key, claim) VALUES (%s, %s)",
                            [(f"recall-eval-{m['id']}", m["text"]) for m in memories])

    def scores(query, extra=()):
        x = " | ".join(sorted(set(extra)))
        rows = conn.execute(PG_ANY, {"q": query, "x": x, "w": SYN_WEIGHT}).fetchall()
        return {mid: float(score) for mid, score in rows}

    return scores


def order(score_map):
    return [m for m, _ in sorted(score_map.items(), key=lambda kv: (-kv[1], kv[0]))]


def rrf(*lists):
    score = {}
    for ranked in lists:
        for i, m in enumerate(ranked):
            score[m] = score.get(m, 0.0) + 1.0 / (RRF_K + i + 1)
    return order(score)


class Arms:
    def __init__(self, memories, vocab, dsn):
        self.vocab = vocab
        self.matcher = LabelMatcher(vocab)
        self.bm25 = bm25_ranker(memories)
        import snowballstemmer

        self.stem = snowballstemmer.stemmer("english").stemWord
        from rank_bm25 import BM25Okapi

        self.ids = [m["id"] for m in memories]
        toks = [[self.stem(t) for t in words(m["text"]) if t not in STOP] for m in memories]
        self.bm25_index = BM25Okapi(toks)
        self.any_scores = any_ranker(memories, dsn)

    def expansion(self, query):
        matched = self.matcher.matches(query)
        qwords = set(words(query))
        extra = []
        for tid in matched:
            for label in self.vocab[tid]:
                extra += [w for w in words(label) if w not in qwords and w not in extra]
        return matched, extra

    def base_scores(self, base, query, extra=()):
        if base == "bm25":
            def bm(ws):
                toks = [self.stem(w) for w in ws if w not in STOP]
                return self.bm25_index.get_scores(toks) if toks else [0.0] * len(self.ids)

            s = bm(words(query))
            if extra:
                e = bm(extra)
                s = [a + SYN_WEIGHT * b for a, b in zip(s, e)]
            return {self.ids[i]: v for i, v in enumerate(s) if v > 0}
        return self.any_scores(query, extra)

    def rank(self, arm, query, tags):
        base, _, rest = arm.partition("+")
        mods = rest.split("+") if rest else []
        matched, extra = self.expansion(query)
        scores = self.base_scores(base, query, extra if "syn" in mods else ())
        ranked = order(scores)[:DEPTH]
        terms = set(matched)
        if "term" in mods or "rareterm" in mods:
            if "rareterm" in mods:
                df = {}
                for ts in tags.values():
                    for t in ts:
                        df[t] = df.get(t, 0) + 1
                terms = {t for t in terms if df.get(t, 0) <= RARE_MAX}
            tagged = {m: len(terms & set(ts)) for m, ts in tags.items() if terms & set(ts)}
            pos = {m: i for i, m in enumerate(ranked)}
            term_list = sorted(tagged, key=lambda m: (-tagged[m], pos.get(m, DEPTH), m))
            ranked = rrf(ranked, term_list)[:DEPTH]
        if "boost" in mods:
            top = max(scores.values(), default=1.0) or 1.0
            final = {}
            for m in set(scores) | {m for m, ts in tags.items() if terms & set(ts)}:
                final[m] = scores.get(m, 0.0) / top + BOOST * len(terms & set(tags.get(m, [])))
            ranked = order(final)[:DEPTH]
        return ranked, sorted(matched)


def lexical_tags(memories, matcher):
    out = {}
    for m in memories:
        found = matcher.matches(m["text"])
        best = sorted(found, key=lambda t: (-sum(h for _, h in found[t]), t))[:3]
        out[m["id"]] = best
    return out


def noisy_tags(curated, term_ids, seed=7):
    rnd = random.Random(seed)
    out = {}
    for m in sorted(curated):
        ts = []
        for t in curated[m]:
            r = rnd.random()
            if r < 0.3:
                continue
            ts.append(rnd.choice(term_ids) if r < 0.5 else t)
        out[m] = sorted(set(ts))
    return out


def run(arms, arm, queries, tags):
    per_query = {}
    for q in queries:
        ranked, matched = arms.rank(arm, q["query"], tags)
        per_query[q["id"]] = {"ranked": ranked[:K], "matched_terms": matched}
    summary = metrics.aggregate(per_query, queries)
    return {"method": arm, "summary": summary, "per_query": per_query}


ARMS = ["any", "any+syn", "any+term", "any+syn+term", "any+syn+boost",
        "bm25", "bm25+syn", "bm25+term", "bm25+rareterm", "any+rareterm", "bm25+syn+term", "bm25+syn+boost"]


def main():
    import argparse

    ap = argparse.ArgumentParser()
    ap.add_argument("--pg-dsn", required=True, help="a throwaway PostgreSQL for the any arms")
    a = ap.parse_args()
    memories, queries = metrics.load()
    vocab = load_vocab()
    arms = Arms(memories, vocab, a.pg_dsn)
    curated = load_tags()
    tagsets = {
        "curated": curated,
        "lexical": lexical_tags(memories, arms.matcher),
        "noisy": noisy_tags(curated, sorted(vocab)),
    }
    out = HERE / "results"
    out.mkdir(exist_ok=True)
    (out / "tagsets.json").write_text(json.dumps(tagsets, indent=1, sort_keys=True))
    for tagset, tags in tagsets.items():
        for arm in ARMS:
            if tagset != "curated" and "term" not in arm and "boost" not in arm:
                continue
            name = arm if tagset == "curated" else f"{arm}@{tagset}"
            result = run(arms, arm, queries, tags)
            result["tags"] = tagset
            (out / f"{name}.json").write_text(json.dumps(result, indent=1))
            o = result["summary"]["overall"]
            print(f"{name:28s} s1 {o['s1']:.3f} s5 {o['s5']:.3f} ndcg {o['ndcg10']:.3f}")


if __name__ == "__main__":
    main()
