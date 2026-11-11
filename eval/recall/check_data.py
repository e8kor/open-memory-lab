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
- metrics.py scores a perfect ranking below 1.0 or an empty one above 0.0;
- the grown set (queries-grown.json, ADR-0075 d8, phase 0a) holds fewer than
  550 queries, a category other than question or paraphrase, an id the
  synthetic set uses, a label whose answer span is not in that memory's text,
  a query that repeats a synthetic query or another grown one, a fold outside
  0 to 4 or one memory's queries in two folds (a query a person relabelled is
  held to the fold of the memory it was drafted from), a query without its
  origin, a person-check sample that is not a tenth of the set, or a person
  check that is done but names no checker, date or corrected-label count, or
  corrects a query outside its sample.
"""

import json
import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402


GROWN_MIN = 550
GROWN_CATEGORIES = ("question", "paraphrase")


def norm(s):
    s = s.strip().strip("\"'`").strip()
    return re.sub(r"\s+", " ", s.lower()).rstrip(".;:,!").strip()


def check_grown(memories, synthetic, errors):
    """The grown set (ADR-0075 d8, phase 0a): its labels are checked, not trusted."""
    path = metrics.HERE / "queries-grown.json"
    if not path.exists():
        return None
    data = json.loads(path.read_text())
    grown = data["queries"]
    text = {m["id"]: m["text"] for m in memories}
    if len(grown) < GROWN_MIN:
        errors.append(f"grown set: {len(grown)} queries, fewer than {GROWN_MIN}")
    if data.get("corpus_version") != json.loads((metrics.HERE / "corpus.json").read_text())["version"]:
        errors.append("grown set: corpus_version is not corpus.json's version")
    taken = {q["id"] for q in synthetic}
    seen = {norm(q["query"]): q["id"] for q in synthetic}
    fold_of = {}
    check = data.get("person_check") or {}
    corrected = check.get("corrections") or {}
    ids = Counter(q.get("id") for q in grown)
    errors += [f"grown {i}: id repeats" for i, n in ids.items() if n > 1]
    for q in grown:
        qid = q.get("id")
        if qid in taken:
            errors.append(f"grown {qid}: the synthetic set uses this id")
        if q.get("category") not in GROWN_CATEGORIES:
            errors.append(f"grown {qid}: category {q.get('category')} is not question or paraphrase")
        if not q.get("query", "").strip():
            errors.append(f"grown {qid}: empty query")
            continue
        key = norm(q["query"])
        if key in seen:
            errors.append(f"grown {qid}: repeats {seen[key]}")
        seen[key] = qid
        rel, spans = q.get("relevant", {}), q.get("answer_span", {})
        if 2 not in rel.values():
            errors.append(f"grown {qid}: no grade-2 memory")
        for mid, grade in rel.items():
            if mid not in text:
                errors.append(f"grown {qid}: labels {mid}, which the corpus does not hold")
                continue
            if grade not in (1, 2):
                errors.append(f"grown {qid}: grade {grade} for {mid}")
            if grade == 2:
                span = norm(spans.get(mid, ""))
                if len(span) < 3 or span not in norm(text[mid]):
                    errors.append(f"grown {qid}: the answer span for {mid} is not in its text")
            drafted_from = corrected[qid]["was"] if qid in corrected else rel
            if drafted_from.get(mid) == 2:
                f = q.get("fold")
                if f not in range(5):
                    errors.append(f"grown {qid}: fold {f} is not 0 to 4")
                elif fold_of.setdefault(mid, f) != f:
                    errors.append(f"grown {qid}: {mid}'s queries sit in two folds")
        if not (q.get("origin") or {}).get("writer"):
            errors.append(f"grown {qid}: no origin")
    passes = sorted((metrics.HERE / "grow").glob("pass*.jsonl"))
    if passes:
        import grow  # noqa: PLC0415 - only the grown set's check needs it

        if grow.render([str(p) for p in passes]) != path.read_text():
            errors.append("grown set: grow.py does not rebuild queries-grown.json byte for byte "
                          "from grow/pass*.jsonl")
    else:
        errors.append("grown set: no grow/pass*.jsonl to rebuild it from")
    sample = (data.get("person_check") or {}).get("sample", [])
    if len(sample) != -(-len(grown) // 10) or not set(sample) <= set(ids):
        errors.append(f"grown set: the person-check sample holds {len(sample)} known ids, "
                      f"not a tenth of {len(grown)}")
    if check.get("status") == "done":
        if not (check.get("checked_by") and check.get("date") and "corrected_labels" in check):
            errors.append("grown set: the person check is done but names no checker, date or "
                          "corrected-label count")
        errors += [f"grown set: the person check corrects {k}, which is not in its sample"
                   for k in corrected if k not in sample]
    return grown


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
    grown = check_grown(memories, queries, errors)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"recall eval data: {len(memories)} memories, {len(queries)} queries, "
          f"{len(cats)} categories ({cats[metrics.UNANSWERABLE]} unanswerable), every label resolves")
    if grown:
        print(f"grown set: {len(grown)} queries ("
              + ", ".join(f"{c} {n}" for c, n in sorted(Counter(q['category'] for q in grown).items()))
              + "), every answer span is in its memory, one fold per memory")
    return 0


if __name__ == "__main__":
    sys.exit(main())
