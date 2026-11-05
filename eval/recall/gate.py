#!/usr/bin/env python3
"""The recall eval's gate per served pipeline (ADR-0049 d9). Standard library only.

    python3 eval/recall/gate.py --results <dir> --default <pipeline> \
        [--committed eval/recall/results]

Reads served_<pipeline>.json for each pipeline found in --results and checks
it against its gate, taken from ADR-0048 d1:

- keyword-all: no gate of its own; its ranked lists must equal the committed
  oml.json's, the v0.5.0 recall as it was served, query by query:
  the same scores in the same order and the same memories at each score.
  Ties break by artifact id, which a fresh load draws anew, so the order
  inside a tie is not compared, nor which tied rows a cut at k kept.
- keyword-any: success@5 >= 0.70 and a zero-result rate <= 0.05.
- bm25: success@5 >= 0.86 and success@1 >= 0.67.
- bm25-trigram: success@5 >= 0.92, and partial and typo success@5 >= 0.91.
- bm25-rerank: success@1 >= 0.87 and success@5 >= 0.94, above bm25's
  measured 0.835 and 0.934 (ADR-0053 d9), and no reranked query skipped:
  a skipped rerank serves bm25's order, so its numbers would gate bm25.
- bm25-postings: bm25's gate (ADR-0052 d9).

Every pipeline must also keep each category's success@5 within one query
(1/11) of its committed result, when one is committed.

Exit 1 when the pipeline named by --default misses its gate or falls, or when
keyword-all does not reproduce v0.5.0; any other pipeline's miss is printed
and is not a failure, since only the default must pass (ADR-0049 d9).
"""

import argparse
import json
import sys
from pathlib import Path

GATES = {
    "keyword-any": [("overall", "s5", ">=", 0.70), ("overall", "zero", "<=", 0.05)],
    "bm25": [("overall", "s5", ">=", 0.86), ("overall", "s1", ">=", 0.67)],
    "bm25-postings": [("overall", "s5", ">=", 0.86), ("overall", "s1", ">=", 0.67)],
    "bm25-trigram": [
        ("overall", "s5", ">=", 0.92),
        ("partial", "s5", ">=", 0.91),
        ("typo", "s5", ">=", 0.91),
    ],
    "bm25-rerank": [("overall", "s1", ">=", 0.87), ("overall", "s5", ">=", 0.94)],
}
# One query of a category of 11, plus the 0.001 the summaries' rounding to
# three decimals can take from a difference (metrics.py): 10/11 is stored as
# 0.909, which is below 1.0 - 1/11 by 0.0001, so a fall of exactly one query
# read as two before this allowance.
ONE_QUERY = 1 / 11 + 0.001


def file_of(pipeline):
    return "served_" + pipeline.replace("-", "_") + ".json"


def groups(ranked, scores, k):
    """[(score, sorted ids)] per run of equal scores; the last group of a
    list cut at k is compared by its size alone."""
    out = []
    for memory, score in zip(ranked, scores):
        score = round(float(score), 5)
        if out and out[-1][0] == score:
            out[-1][1].append(memory)
        else:
            out.append((score, [memory]))
    out = [(s, sorted(ids)) for s, ids in out]
    if len(ranked) >= k and out:
        s, ids = out[-1]
        out[-1] = (s, len(ids))
    return out


def check(pipeline, result, committed):
    problems = []
    if pipeline == "bm25-rerank":
        skipped = sum(1 for row in result["per_query"].values() if row.get("rerank") == "skipped")
        if skipped:
            problems.append(f"{skipped} queries skipped the rerank")
    summary = result["summary"]
    for scope, metric, op, bound in GATES.get(pipeline, []):
        value = summary[scope][metric]
        ok = value >= bound if op == ">=" else value <= bound
        if not ok:
            problems.append(f"{scope} {metric} {value} is not {op} {bound}")
    if committed is not None:
        for category, now in summary.items():
            if category in ("overall", "unanswerable") or category not in committed["summary"]:
                continue
            before = committed["summary"][category]["s5"]
            if now["s5"] < before - ONE_QUERY:
                problems.append(f"{category} success@5 fell from {before} to {now['s5']}")
    return problems


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", required=True)
    ap.add_argument("--committed", default=str(Path(__file__).resolve().parent / "results"))
    ap.add_argument("--default", required=True)
    a = ap.parse_args()
    results, committed_dir = Path(a.results), Path(a.committed)
    failed = False
    seen = []
    for pipeline in ["keyword-all", "keyword-any", "bm25", "bm25-trigram", "bm25-postings", "bm25-rerank"]:
        path = results / file_of(pipeline)
        if not path.exists():
            continue
        seen.append(pipeline)
        result = json.loads(path.read_text())
        if pipeline == "keyword-all":
            reference = json.loads((committed_dir / "oml.json").read_text())
            k = result["params"]["k"]
            # Data version 2's unanswerable queries postdate v0.5.0's lists.
            differ = [q for q, row in result["per_query"].items()
                      if q in reference["per_query"] and groups(row["ranked"], row["scores_raw"], k)
                      != groups(reference["per_query"][q]["ranked"],
                                reference["per_query"][q]["scores_raw"], k)]
            if differ:
                failed = True
                print(f"FAIL keyword-all does not reproduce v0.5.0 on {len(differ)} queries: {differ[:5]}")
            else:
                print("ok   keyword-all reproduces v0.5.0's ranked lists on every query")
            continue
        committed_path = committed_dir / file_of(pipeline)
        committed = json.loads(committed_path.read_text()) if committed_path.exists() else None
        problems = check(pipeline, result, committed)
        overall = result["summary"]["overall"]
        line = f"{pipeline}: success@1 {overall['s1']} success@5 {overall['s5']} zero {overall['zero']}"
        if problems:
            mark = "FAIL" if pipeline == a.default else "miss"
            failed = failed or pipeline == a.default
            print(f"{mark} {line}; " + "; ".join(problems))
        else:
            print(f"ok   {line}")
    if a.default not in seen:
        print(f"FAIL the default {a.default} was not measured")
        failed = True
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
