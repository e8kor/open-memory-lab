#!/usr/bin/env python3
"""Sweeps a relevance floor over runs made with none (ADR-0053 d6). Standard library only.

    rerank_floor.py --recall <eval/recall served_bm25_rerank.json> \
        [--locomo <bench.py raw locomo_bm25_rerank.json>] [--floors 0.01 0.05 ...]

A run of `bm25-rerank` with no floor answers each hit's relevance
(`matched.rerank`). A floor `t` withholds every hit under `t`, so a query
answers nothing exactly when its best relevance is under `t`. For each `t`
this prints:

- synthetic `unanswerable` (data version 2, 22 queries no memory answers):
  the share answered with nothing, the floor's precision on what it should
  refuse;
- synthetic answerable (121 queries): the share answered with nothing, the
  answers it costs, and success@1 and success@5 after the floor;
- LoCoMo by category: the share answered with nothing; `adversarial`
  questions presume something the conversation never says, though their
  evidence turns exist, so their empty share is a weak stand-in for
  "nothing relevant", reported beside categories 1-4's.

`--bm25 <served_bm25.json>` adds the same sweep over a first-stage score
normalised per query term (`score / terms`), the floor a skipped rerank
could fall back on.
"""

import argparse
import json
from pathlib import Path


def best(row):
    scores = [s for s in row.get("rerank_scores") or [] if s is not None]
    return max(scores) if scores else None


def recall_sweep(path, floors, key=best):
    r = json.loads(Path(path).read_text())
    queries = json.loads((Path(__file__).resolve().parent.parent / "recall" / "queries.json")
                         .read_text())["queries"]
    cat = {q["id"]: q for q in queries}
    print(f"\nsynthetic, {r['method']}: floor | unanswerable empty | answerable empty | S@1 | S@5")
    for t in floors:
        un = ans = un_empty = ans_empty = s1 = s5 = 0
        for qid, row in r["per_query"].items():
            q = cat[qid]
            b = key(row)
            kept = row["ranked"] if b is not None and b >= t else []
            if q["category"] == "unanswerable":
                un += 1
                un_empty += not kept
                continue
            ans += 1
            ans_empty += not kept
            if t is not None and kept and key is best:
                kept = [m for m, s in zip(row["ranked"], row["rerank_scores"]) if s is not None and s >= t]
            rel = q["relevant"]
            s1 += any(rel.get(m) == 2 for m in kept[:1])
            s5 += any(rel.get(m) == 2 for m in kept[:5])
        print(f"{t:>6} | {un_empty}/{un} = {un_empty / max(un, 1):.3f} | {ans_empty}/{ans} = "
              f"{ans_empty / ans:.3f} | {s1 / ans:.3f} | {s5 / ans:.3f}")


def per_term(row):
    scores = row.get("scores_raw") or []
    terms = max(len((row.get("sent") or "").split()), 1)
    return scores[0] / terms if scores else None


def locomo_sweep(path, floors):
    r = json.loads(Path(path).read_text())
    cats = {}
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from adapters import ADAPTERS
    b = ADAPTERS["locomo"]().load("/srv/build/bench-data", 20261104, None)
    for q in b.queries:
        cats[q.id] = q.category
    names = ["single-hop", "multi-hop", "temporal", "open-domain", "adversarial"]
    print("\nLoCoMo empty share by category: floor | " + " | ".join(names))
    for t in floors:
        cells = []
        for c in names:
            rows = [row for qid, row in r["per_query"].items() if cats.get(qid) == c]
            empty = sum(1 for row in rows if (best(row) or 0) < t)
            cells.append(f"{empty / len(rows):.3f}")
        print(f"{t:>6} | " + " | ".join(cells))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recall")
    ap.add_argument("--bm25")
    ap.add_argument("--locomo")
    ap.add_argument("--floors", nargs="+", type=float,
                    default=[0.0, 0.001, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5])
    a = ap.parse_args()
    if a.recall:
        recall_sweep(a.recall, a.floors)
    if a.bm25:
        recall_sweep(a.bm25, [0, 0.5, 1, 1.5, 2, 2.5, 3, 4], key=per_term)
    if a.locomo:
        locomo_sweep(a.locomo, a.floors)


if __name__ == "__main__":
    main()
