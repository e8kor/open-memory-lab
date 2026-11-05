#!/usr/bin/env python3
"""The recall eval's data is well formed. Standard library only; needs no server.

Run by .github/workflows/ci.yml (job recall-eval-data). Fails when:

- corpus.json or queries.json does not parse, or an id repeats;
- a memory lacks id, type, topic or text, or its text is empty;
- a query's category is not one of metrics.CATEGORIES, or a category has
  fewer than 5 queries;
- a query labels a memory id the corpus does not hold, labels a grade other
  than 1 or 2, or has no grade-2 memory (success@k would be undefined);
- an `unanswerable` query labels any memory, or there are fewer than 20 of
  them (data version 2; ADR-0053 d6);
- metrics.py scores a perfect ranking below 1.0 or an empty one above 0.0.
"""

import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402


def main():
    memories, queries = metrics.load()
    errors = []
    ids = Counter(m.get("id") for m in memories)
    errors += [f"memory id {i} repeats" for i, n in ids.items() if n > 1]
    for m in memories:
        if not all(m.get(f) for f in ("id", "type", "topic", "text")):
            errors.append(f"memory {m.get('id')} lacks a field")
    qids = Counter(q.get("id") for q in queries)
    errors += [f"query id {i} repeats" for i, n in qids.items() if n > 1]
    cats = Counter()
    for q in queries:
        cats[q["category"]] += 1
        if q["category"] == metrics.UNANSWERABLE:
            if q["relevant"]:
                errors.append(f"{q['id']}: unanswerable, yet labels {sorted(q['relevant'])}")
            if not q["query"].strip():
                errors.append(f"{q['id']}: empty query")
            continue
        if q["category"] not in metrics.CATEGORIES:
            errors.append(f"{q['id']}: unknown category {q['category']}")
        if not q["query"].strip():
            errors.append(f"{q['id']}: empty query")
        for mid, grade in q["relevant"].items():
            if mid not in ids:
                errors.append(f"{q['id']}: labels {mid}, which the corpus does not hold")
            if grade not in (1, 2):
                errors.append(f"{q['id']}: grade {grade} for {mid}")
        if 2 not in q["relevant"].values():
            errors.append(f"{q['id']}: no grade-2 memory")
        ideal = sorted(q["relevant"], key=lambda m: -q["relevant"][m])
        best, none = metrics.score_query(ideal, q["relevant"]), metrics.score_query([], q["relevant"])
        if min(best[k] for k in ("s1", "s5", "mrr", "ndcg10")) < 1.0 or best["r10"] < 1.0 and len(ideal) <= 10:
            errors.append(f"{q['id']}: a perfect ranking scores below 1.0: {best}")
        if max(none[k] for k in ("s1", "s5", "r10", "mrr", "ndcg10")) > 0.0:
            errors.append(f"{q['id']}: an empty ranking scores above 0.0: {none}")
    errors += [f"category {c} has {cats[c]} queries, fewer than 5" for c in metrics.CATEGORIES if cats[c] < 5]
    if cats[metrics.UNANSWERABLE] < 20:
        errors.append(f"{cats[metrics.UNANSWERABLE]} unanswerable queries, fewer than 20")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"recall eval data: {len(memories)} memories, {len(queries)} queries, "
          f"{len(cats)} categories ({cats[metrics.UNANSWERABLE]} unanswerable), every label resolves")
    return 0


if __name__ == "__main__":
    sys.exit(main())
