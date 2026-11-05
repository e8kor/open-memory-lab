#!/usr/bin/env python3
"""Write results in the common benchmark schema, `oml-bench/1`, for the docs.

    export.py --raw <bench.py --out dir> --commit <sha> [--date YYYY-MM-DD]
              [--postgres "18.6"] [--host "8 vCPU / 15 GB"] [--recall eval/recall/results]

One file per suite and method: eval/bench/results/<suite>/<method>.json.
Sources:
- bench.py and reference.py results in --raw (`<dataset>_<method>.json`);
  the harness's own synthetic run is not exported, since eval/recall's
  served results are that suite's record and are exported instead;
- eval/recall's served results (`served_<pipeline>.json`, --recall), as the
  suite `recall-synthetic` with its own metric definitions (grade-2 success,
  gain 2^g - 1, k 10 and a 2,000-token budget, so recall@100 is null).

Numbers are kept as measured, rounded to 4 decimals at most. The schema is
documented in README.md (Results schema).
"""

import argparse
import hashlib
import json
import math
import statistics
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from adapters import ADAPTERS, PUBLISHED_BM25  # noqa: E402

SUITES = {"scifact": "beir-scifact", "nfcorpus": "beir-nfcorpus", "arguana": "beir-arguana",
          "locomo": "locomo", "scifact-x10": "beir-scifact-x10"}
METHOD_NAMES = {"ref_bm25_anserini": "reference-bm25s-k1-0.9-b-0.4",
                "ref_bm25_oml_params": "reference-bm25s-k1-1.2-b-0.75"}
BEIR_URL = "https://arxiv.org/abs/2104.08663"
METRICS = {"nDCG@10": "ndcg@10", "R@10": "recall@10", "R@100": "recall@100",
           "RR@10": "mrr@10", "Success@1": "success@1", "Success@5": "success@5",
           "zero": "zero_result_rate"}


def r4(x):
    return None if x is None else round(float(x), 4)


def pct(values, p):
    """Nearest-rank percentile; None when there is nothing to rank."""
    v = sorted(x for x in values if x is not None)
    if not v:
        return None
    return r4(v[min(len(v), max(1, math.ceil(p / 100 * len(v)))) - 1])


def latency(per_query, offline=False):
    if offline:
        return {"p50": None, "p95": None, "p99": None}
    lat = [row.get("latency_ms") for row in per_query.values()]
    return {"p50": r4(statistics.median(lat)), "p95": pct(lat, 95), "p99": pct(lat, 99)}


def suite_version(dataset, cache):
    if dataset == "locomo":
        p = Path(cache) / "locomo" / "locomo10.json"
        digest = hashlib.sha256(p.read_bytes()).hexdigest()[:16] if p.exists() else "unknown"
        return f"locomo10.json sha256:{digest}"
    ir = {"scifact": "beir/scifact/test", "nfcorpus": "beir/nfcorpus/test",
          "arguana": "beir/arguana", "scifact-x10": "beir/scifact/test x10"}[dataset]
    try:
        import importlib.metadata as m
        v = m.version("ir_datasets")
    except Exception:  # noqa: BLE001 - only the label depends on it
        v = "unknown"
    return f"{ir} (ir_datasets {v})"


def from_bench(r, a):
    ds, method = r["dataset"], r["pipeline"]
    if r.get("k", 100) != 100:
        method = f"{method}-k{r['k']}"
    offline = method in METHOD_NAMES
    s = r["summary"]
    head_key = "headline" if "headline" in s else "all"
    head = s[head_key]
    per_category = {}
    for c, row in s.items():
        if c in ("all", "headline"):
            continue
        per_category[c] = {METRICS[k]: r4(row[k]) for k in METRICS} | {"n_queries": row["n"]}
    if head_key == "headline":
        per_category["all-categories"] = ({METRICS[k]: r4(s["all"][k]) for k in METRICS}
                                          | {"n_queries": s["all"]["n"]})
    pub = PUBLISHED_BM25.get(ds)
    reference = ({"source": "BEIR BM25 (Anserini, k1 0.9, b 0.4, title and text fields)",
                  "metric": "ndcg@10", "value": pub["nDCG@10"], "url": BEIR_URL}
                 if pub else None)
    if ds == "scifact-x10":
        note = ("SciFact's abstracts and nine copies with 10% of words replaced, 51,830 memories "
                "written to one workarea, for latency at scale; qrels name the original copy only, "
                "so quality is not comparable to beir-scifact. k 100, no token budget.")
    elif ds == "locomo":
        note = ("One dialogue turn per memory, each conversation its own workarea; metrics over "
                "categories 1-4, adversarial in per_category; English only.")
    else:
        note = ("One abstract per memory as '<title>. <text>' (BEIR indexes two fields); "
                "k 100, no token budget; English only.")
    if ds == "nfcorpus" and not offline:
        note += (" Known gap: served bm25 measures below BEIR's published 0.325 and Pyserini's "
                 "single-field 0.322; the offline textbook BM25 over the same memories measures "
                 "0.318, so most of the gap is the one-body mapping and the analyser (Snowball "
                 "against Lucene's Porter, hyphenated words kept whole), not the ranking "
                 "(ADR-0049 d12).")
    if r.get("rows_in_scope") and r["rows_in_scope"] < r["docs"]:
        note += (f" {r['rows_in_scope']:,} of the {r['docs']:,} rows written were promoted when "
                 "it ran, so the scope held those: approval through review next slowed with the "
                 "queue (FR-87).")
    if r.get("k", 100) != 100:
        note += (f" Asked with k {r['k']}, the size an agent asks, to measure latency at that k; "
                 "recall@100 reads the shorter list.")
    if not offline and r.get("workers", 1) > 1:
        note += (f" Queries ran {r['workers']} at a time to fit the time box, so latency is "
                 "under that many concurrent clients.")
    if offline:
        note = ("Offline bm25s over the same memories (Lucene BM25, English stop words, "
                "Snowball stems), not a served path; " + note)
    limit = r.get("limit")
    total = a.full_queries.get(ds) if limit else None
    if limit and not total:
        total = len(ADAPTERS[ds]().load(a.cache, r["seed"], None).queries)
    return SUITES[ds], METHOD_NAMES.get(method, method), {
        "schema": "oml-bench/1",
        "suite": SUITES[ds],
        "suite_version": suite_version(ds, a.cache),
        "kind": "recall",
        "method": METHOD_NAMES.get(method, method),
        "date": a.date,
        "commit": a.commit,
        "environment": {"host": a.host, "postgres": a.postgres, "build": "release"},
        "n_queries": head["n"],
        "n_docs": r.get("rows_in_scope") or r["docs"],
        "sampled": ({"seed": r["seed"], "fraction": r4(r["queries"] / total)}
                    if limit and total else None),
        "metrics": {METRICS[k]: r4(head[k]) for k in METRICS},
        "per_category": per_category,
        "latency_ms": latency(r["per_query"], offline),
        "reference": reference,
        "notes": note,
    }


def from_recall(r, a, version, commit, date):
    s = r["summary"]
    o = s["overall"]
    names = {"s1": "success@1", "s5": "success@5", "r10": "recall@10", "mrr": "mrr@10",
             "ndcg10": "ndcg@10", "zero": "zero_result_rate"}
    metrics = {names[k]: r4(o[k]) for k in names}
    metrics["recall@100"] = None
    metrics = {k: metrics[k] for k in ["ndcg@10", "recall@10", "recall@100", "mrr@10",
                                       "success@1", "success@5", "zero_result_rate"]}
    per_category = {c: {names[k]: r4(row[k]) for k in names} | {"n_queries": row["n"]}
                    for c, row in s.items() if c not in ("overall", "unanswerable")}
    # Data version 2's unanswerable queries are scored apart (ADR-0053 d6).
    unanswerable = s.get("unanswerable")
    return "recall-synthetic", r["pipeline"], {
        "schema": "oml-bench/1",
        "suite": "recall-synthetic",
        "suite_version": f"eval/recall data version {version}",
        "kind": "recall",
        "method": r["pipeline"],
        "date": date,
        "commit": commit,
        "environment": {"host": a.host, "postgres": a.postgres, "build": "release"},
        "n_queries": o["n"],
        "n_docs": 200,
        "sampled": None,
        "metrics": metrics,
        "per_category": per_category,
        "latency_ms": latency(r["per_query"]),
        "reference": None,
        "notes": ("One model wrote corpus, queries and labels; success counts grade 2, nDCG gains "
                  "2^g-1, k 10 under a 2,000-token budget, so recall@100 is not measured."
                  + (f" Of {unanswerable['n']} unanswerable queries, kept out of these metrics, "
                     f"{unanswerable['empty']:.3f} were answered with no hit."
                     if unanswerable else "")),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw", required=True)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--date", required=True)
    ap.add_argument("--postgres", required=True)
    ap.add_argument("--host", required=True)
    ap.add_argument("--cache", default="/srv/build/bench-data")
    ap.add_argument("--recall", default=str(HERE.parent / "recall" / "results"))
    ap.add_argument("--out", default=str(HERE / "results"))
    a = ap.parse_args()
    out = Path(a.out)
    written = []
    # A run asked with --limit is a seeded subset; its fraction is of the
    # queries the unlimited runs of the same dataset asked.
    a.full_queries = {}
    for p in sorted(Path(a.raw).glob("*.json")):
        r = json.loads(p.read_text())
        if "summary" in r and not r.get("limit"):
            a.full_queries[r["dataset"]] = max(a.full_queries.get(r["dataset"], 0), r["queries"])
    for p in sorted(Path(a.raw).glob("*.json")):
        r = json.loads(p.read_text())
        if "summary" not in r or r.get("dataset") not in SUITES:
            continue
        suite, method, doc = from_bench(r, a)
        written.append((suite, method, doc))
    recall = Path(a.recall)
    version = json.loads((recall.parent / "queries.json").read_text())["version"]
    for p in sorted(recall.glob("served_*.json")):
        rel = p.relative_to(HERE.parent.parent)
        log = subprocess.run(["git", "log", "-1", "--format=%h %ad", "--date=short", "--",
                              str(rel)], capture_output=True, text=True,
                             cwd=HERE.parent.parent).stdout.split()
        commit, date = (log + ["unknown", "unknown"])[:2]
        written.append(from_recall(json.loads(p.read_text()), a, version, commit, date))
    for suite, method, doc in written:
        d = out / suite
        d.mkdir(parents=True, exist_ok=True)
        (d / f"{method}.json").write_text(json.dumps(doc, indent=1) + "\n")
        print(f"{suite}/{method}.json")


if __name__ == "__main__":
    main()
