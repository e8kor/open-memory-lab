#!/usr/bin/env python3
"""An offline BM25 reference over the same memories, queries and qrels.

    reference.py [--cache C] <out-dir> <dataset>...

It ranks each dataset's memories with bm25s (MIT), a textbook BM25, outside
OML, and scores the lists with bench.py's own scorer, so the gap between
BEIR's published BM25 and OML's served `bm25` splits in two: the mapping
(one body instead of title and text fields; BEIR used Anserini) and the
served implementation (OML's normaliser, lexemes and statistics).

Two settings, both over the full corpus of the dataset (each LoCoMo
conversation ranked alone, as the served recall scopes it):
- `ref_bm25_anserini`: Lucene's BM25 variant with k1 0.9 and b 0.4, English
  stop words removed and Snowball English stemming, BEIR's configuration
  short of its two fields;
- `ref_bm25_oml_params`: the same with OML's k1 1.2 and b 0.75.
Writes <out-dir>/<dataset>_<setting>.json in bench.py's result shape, with
no latency (the reference is not a served path).
"""

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import bench  # noqa: E402
from adapters import ADAPTERS  # noqa: E402

SETTINGS = {"ref_bm25_anserini": (0.9, 0.4), "ref_bm25_oml_params": (1.2, 0.75)}


def main():
    import bm25s
    import Stemmer

    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=bench.DEFAULT_CACHE)
    ap.add_argument("--seed", type=int, default=20261104)
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("out")
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()
    stemmer = Stemmer.Stemmer("english")
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    for name in a.datasets:
        b = ADAPTERS[name]().load(a.cache, a.seed, a.limit)
        groups = sorted({d.group for d in b.docs})
        for setting, (k1, bb) in SETTINGS.items():
            runs = {}
            for g in groups:
                docs = [d for d in b.docs if d.group == g]
                qs = [q for q in b.queries if q.group == g]
                if not qs:
                    continue
                retriever = bm25s.BM25(method="lucene", k1=k1, b=bb)
                retriever.index(bm25s.tokenize([d.text for d in docs], stopwords="en",
                                               stemmer=stemmer, show_progress=False),
                                show_progress=False)
                k = min(bench.K, len(docs))
                for q in qs:
                    toks = bm25s.tokenize([q.text], stopwords="en", stemmer=stemmer,
                                          show_progress=False)
                    if not toks.ids[0]:
                        runs[q.id] = {"ranked": [], "latency_ms": 0.0}
                        continue
                    idx, scores = retriever.retrieve(toks, k=k, show_progress=False)
                    ranked = [docs[i].id for i, s in zip(idx[0], scores[0]) if s > 0]
                    runs[q.id] = {"ranked": ranked, "latency_ms": 0.0}
            per = bench.score(b, runs)
            summary = bench.summarize(b, runs, per)
            for row in summary.values():
                row["p50_ms"] = row["p95_ms"] = None
            result = {"dataset": name, "pipeline": setting, "k": bench.K, "k1": k1, "b": bb,
                      "queries": len(b.queries), "docs": len(b.docs), "seed": a.seed,
                      "limit": a.limit, "success_rel": b.success_rel, "notes": b.notes,
                      "summary": summary,
                      "per_query": {q: {**runs[q], "scores": per[q]} for q in runs}}
            (out / f"{name}_{setting}.json").write_text(json.dumps(result, indent=1))
            head = summary.get("headline", summary["all"])
            print(f"{name} {setting}: " + " ".join(f"{m} {head[m]}" for m in bench.MEASURES)
                  + f" zero {head['zero']} n {head['n']}")


if __name__ == "__main__":
    main()
