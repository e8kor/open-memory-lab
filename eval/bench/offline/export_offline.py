#!/usr/bin/env python3
"""Write evaluate.py's results in `oml-bench/1` under eval/bench/results/offline/.

    export_offline.py --raw RAW --enc ENC --commit SHA --date YYYY-MM-DD [--host H]

One file per suite and method, eval/bench/results/offline/<suite>/<method>.json,
kind `offline`: no served path ran, so latency_ms is null. Beside the schema's
fields each file carries `cost` (the model's encode throughput and, for dense
models, its query encode p50/p95 from ENC/<model>/cost.json, or the sparse
model's non-zero tokens per document) and, for a convex combination, the
weights tuned on synthetic. Also writes tables.md, the REPORT's tables.
"""

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import export as base  # noqa: E402  eval/bench/export.py

SUITES = {"synthetic": "recall-synthetic", "scifact": "beir-scifact",
          "nfcorpus": "beir-nfcorpus", "locomo": "locomo"}
MODELS = {"sparse-distill": "opensearch-neural-sparse-encoding-doc-v3-distill (Apache-2.0)",
          "sparse-gte": "opensearch-neural-sparse-encoding-doc-v3-gte (Apache-2.0)",
          "bge-small": "BAAI/bge-small-en-v1.5 (MIT)",
          "arctic-s": "Snowflake/snowflake-arctic-embed-s (Apache-2.0)"}
CARD = {  # the model cards' own nDCG@10, fetched 2026-11-05
    "sparse-distill": {"scifact": 0.708, "nfcorpus": 0.345},
    "sparse-gte": {"scifact": 0.725, "nfcorpus": 0.360},
}
CARD_URL = "https://huggingface.co/opensearch-project/opensearch-neural-sparse-encoding-doc-v3-"


def paired_p(a, b, n=10000, seed=20261105):
    """Two-sided paired sign-flip randomisation test on per-query differences."""
    import numpy as np
    d = np.asarray(a) - np.asarray(b)
    obs = abs(d.sum())
    signs = np.random.default_rng(seed).choice([-1.0, 1.0], size=(n, len(d)))
    hits = int((np.abs(signs @ d) >= obs - 1e-12).sum())
    return (hits + 1) / (n + 1)


def models_of(method):
    return [m for m in MODELS if m in method]


def describe(method, r):
    if method == "bm25":
        return ("The served bm25 statement (oml-pg bm25_sql, k1 1.2, b 0.75) over migration "
                "0020's stored lexemes, run in a throwaway Postgres 18 with the scope reduced to "
                "the query's workarea; it reproduces the served run to the third decimal.")
    if method in MODELS:
        kind = ("Inference-free learned sparse: documents encoded by the model, queries by its "
                "tokenizer and idf.json, no model at read." if method.startswith("sparse")
                else "Dense, exact cosine over every row of the scope; query with the card's "
                     "retrieval prompt.")
        return f"{MODELS[method]}. {kind}"
    legs = method.split("-", 1)[1]
    if method.startswith("rrf-"):
        return f"RRF (k 60, equal weights) of {legs} over each leg's top 100."
    w = r.get("weights", {})
    return ("Convex combination of " + " + ".join(f"{v:g} x {k}" for k, v in w.items())
            + ", each leg min-max normalised over this query's own top 100; weights tuned on "
              "recall-synthetic (nDCG@10) and applied unchanged here.")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw", required=True)
    ap.add_argument("--enc", required=True)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--date", required=True)
    ap.add_argument("--host", default="8 vCPU (AMD EPYC Genoa) / 15 GB, shared")
    ap.add_argument("--cache", default="/srv/build/bench-data")
    ap.add_argument("--out", default=str(HERE.parent / "results" / "offline"))
    a = ap.parse_args()
    costs = {m: json.loads((Path(a.enc) / m / "cost.json").read_text())
             for m in MODELS if (Path(a.enc) / m / "cost.json").exists()}
    clean = Path(a.enc) / "cost-clean.json"
    clean = json.loads(clean.read_text()) if clean.exists() else {}
    out = Path(a.out)
    rows = {}
    raws = {}
    for p in sorted(Path(a.raw).glob("*_*.json")):
        r = json.loads(p.read_text())
        if "summary" in r:
            raws[(r["dataset"], r["pipeline"])] = r
    from adapters import ADAPTERS
    headline = {}
    for ds in {k[0] for k in raws}:
        b = ADAPTERS[ds]().load(a.cache, 20261104, None)
        headline[ds] = [q.id for q in b.queries if q.category != "adversarial"]
    for p in sorted(Path(a.raw).glob("*_*.json")):
        r = json.loads(p.read_text())
        if "summary" not in r:
            continue
        ds, method = r["dataset"], r["pipeline"]
        s = r["summary"]
        hk = "headline" if "headline" in s else "all"
        head = s[hk]
        per_category = {c: {base.METRICS[k]: base.r4(row[k]) for k in base.METRICS}
                        | {"n_queries": row["n"]}
                        for c, row in s.items() if c not in ("all", "headline")}
        if hk == "headline":
            per_category["all-categories"] = (
                {base.METRICS[k]: base.r4(s["all"][k]) for k in base.METRICS}
                | {"n_queries": s["all"]["n"]})
        vs = None
        if method != "bm25" and (ds, "bm25") in raws:
            ids = headline[ds]
            x = [r["per_query"][q]["scores"]["nDCG@10"] for q in ids]
            y = [raws[(ds, "bm25")]["per_query"][q]["scores"]["nDCG@10"] for q in ids]
            vs = {"metric": "ndcg@10", "delta": base.r4(sum(x) / len(x) - sum(y) / len(y)),
                  "p_value": base.r4(paired_p(x, y)),
                  "test": "paired sign-flip randomisation, 10,000 draws, two-sided"}
        cost = {}
        for m in models_of(method):
            c = dict(costs.get(m, {}).get(ds, {}))
            c["clean"] = clean.get(m)
            cost[m] = c
        ref = None
        if method in CARD and ds in CARD[method]:
            ref = {"source": "model card", "metric": "ndcg@10", "value": CARD[method][ds],
                   "url": CARD_URL + method.split("-")[1]}
        elif ds in base.PUBLISHED_BM25 and method == "bm25":
            ref = {"source": "BEIR BM25", "metric": "ndcg@10",
                   "value": base.PUBLISHED_BM25[ds]["nDCG@10"], "url": base.BEIR_URL}
        version = (base.suite_version(ds, a.cache) if ds != "synthetic"
                   else "eval/recall data version 1, scored by eval/bench/bench.py (gain = grade)")
        doc = {
            "schema": "oml-bench/1", "suite": SUITES[ds], "suite_version": version,
            "kind": "offline", "method": method, "date": a.date, "commit": a.commit,
            "environment": {"host": a.host, "postgres": "18.6 (bm25 leg only)",
                            "build": "offline python (torch 2.15 cpu, fp32)"},
            "n_queries": head["n"], "n_docs": r["docs"], "sampled": None,
            "metrics": {base.METRICS[k]: base.r4(head[k]) for k in base.METRICS},
            "per_category": per_category,
            "latency_ms": {"p50": None, "p95": None, "p99": None},
            "reference": ref,
            "notes": describe(method, r),
            "cost": cost or None,
            "vs_bm25": vs,
        }
        if "weights" in r:
            doc["weights"] = r["weights"]
            doc["tuned_on"] = r["tuned_on"]
        d = out / SUITES[ds]
        d.mkdir(parents=True, exist_ok=True)
        (d / f"{method}.json").write_text(json.dumps(doc, indent=1) + "\n")
        rows.setdefault(method, {})[ds] = doc
    tables(rows, out / "tables.md")


def tables(rows, path):
    sets = ["synthetic", "scifact", "nfcorpus", "locomo"]
    suite = {k: SUITES[k] for k in sets}
    order = sorted(rows, key=lambda m: (m != "bm25", m.startswith(("rrf", "cc")),
                                        m.startswith("cc"), m))
    L = ["## Headline (nDCG@10 / R@10 / S@1 / S@5; LoCoMo over categories 1-4)\n",
         "| method | " + " | ".join(suite[s] for s in sets) + " |",
         "|---|" + "---|" * len(sets)]
    for m in order:
        cells = []
        for s in sets:
            d = next((x for x in rows[m].values() if x["suite"] == suite[s]), None)
            if d is None:
                cells.append("–")
                continue
            x = d["metrics"]
            cells.append(f"{x['ndcg@10']:.3f} / {x['recall@10']:.3f} / {x['success@1']:.3f} / "
                         f"{x['success@5']:.3f}")
        L.append(f"| {m} | " + " | ".join(cells) + " |")
    L += ["", "## nDCG@10 gain over the served bm25 ((ns): paired randomisation p >= 0.05)\n",
          "| method | " + " | ".join(suite[s] for s in sets) + " |", "|---|" + "---:|" * len(sets)]
    for m in order[1:]:
        cells = []
        for s in sets:
            d = next((x for x in rows[m].values() if x["suite"] == suite[s]), None)
            if d is None:
                cells.append("–")
                continue
            v = d["vs_bm25"]
            mark = "" if v["p_value"] < 0.05 else " (ns)"
            cells.append(f"{v['delta']:+.3f}{mark}")
        L.append(f"| {m} | " + " | ".join(cells) + " |")
    cats = ["multi-hop", "temporal", "open-domain", "single-hop", "adversarial"]
    L += ["", "## LoCoMo nDCG@10 by category\n", "| method | " + " | ".join(cats) + " |",
          "|---|" + "---:|" * len(cats)]
    for m in order:
        d = next((x for x in rows[m].values() if x["suite"] == "locomo"), None)
        if d:
            L.append(f"| {m} | " + " | ".join(f"{d['per_category'][c]['ndcg@10']:.3f}"
                                             for c in cats) + " |")
    syn_cats = None
    for m in order:
        d = next((x for x in rows[m].values() if x["suite"] == "recall-synthetic"), None)
        if d and syn_cats is None:
            syn_cats = sorted(c for c in d["per_category"])
            L += ["", "## recall-synthetic nDCG@10 by category\n",
                  "| method | " + " | ".join(syn_cats) + " |", "|---|" + "---:|" * len(syn_cats)]
        if d:
            L.append(f"| {m} | " + " | ".join(f"{d['per_category'][c]['ndcg@10']:.3f}"
                                             for c in syn_cats) + " |")
    path.write_text("\n".join(L) + "\n")
    print(path)


if __name__ == "__main__":
    main()
