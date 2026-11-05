#!/usr/bin/env python3
"""Score learned sparse, dense and fused rankings beside the served bm25, offline.

    evaluate.py --enc ENC --runs RUNS --out RAW <dataset>...

Legs, each a top-100 list with scores per query, inside the query's own group
(a LoCoMo question sees only its conversation, as the served scope does):
- `bm25`: served_bm25.py's output, the served statement over the served lexemes;
- `sparse-<m>`: inference-free learned sparse. A document is its encode.py
  vector; a query is its tokenizer's ids (special tokens dropped, each once)
  weighted by the model's `idf.json`, so no model runs at read and no corpus
  statistic is read. Score = sum over query tokens of idf(t) * w_doc(t);
- `<dense m>`: exact cosine over encode.py's unit vectors.

Fusions over the legs' top-100 lists (a document missing from a leg's list
gets nothing from that leg):
- `rrf`: sum of 1 / (60 + rank), equal weights, no tuning;
- `cc`: convex combination of each leg's scores min-max normalised over this
  query's own list (never a corpus-wide statistic). The weights are tuned on
  the synthetic set alone (nDCG@10, grid step 0.05 for two legs, 0.1 for
  three) and applied unchanged to SciFact, NFCorpus and LoCoMo.

Scores every list with bench.py's scorer (ir_measures) and writes
RAW/<dataset>_<method>.json in bench.py's result shape, plus RAW/weights.json,
RAW/curves.json (the synthetic tuning curve) and RAW/oracle.json (each set's own
curve for two-leg fusions, reported to show the transfer loss, never used to
choose a weight).
"""

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import scipy.sparse as sp

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import bench  # noqa: E402
from adapters import ADAPTERS  # noqa: E402

CACHE = "/srv/build/bench-data"
K = 100
RRF_K = 60
SPARSE = {"sparse-distill": "opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill",
          "sparse-gte": "opensearch-project/opensearch-neural-sparse-encoding-doc-v3-gte"}
DENSE = ["bge-small", "arctic-s"]


def sparse_query_encoder(model):
    from huggingface_hub import hf_hub_download
    from transformers import AutoTokenizer

    tok = AutoTokenizer.from_pretrained(SPARSE[model], trust_remote_code=True)
    idf_raw = json.loads(Path(hf_hub_download(SPARSE[model], "idf.json")).read_text())
    idf = np.zeros(tok.vocab_size, dtype=np.float32)
    for t, w in idf_raw.items():
        i = tok.convert_tokens_to_ids(t)
        if i is not None and i < len(idf):
            idf[i] = w
    special = set(tok.all_special_ids)

    def encode(text):
        ids = sorted({i for i in tok(text, truncation=True, max_length=512)["input_ids"]
                      if i not in special})
        return np.array(ids, dtype=np.int64), idf[ids]
    return encode


def top(scores, ids, k=K, positive=True):
    """[(doc_id, score)] best first, ties by doc id; with `positive`, a zero
    score (no shared token) is no hit."""
    k = min(k, len(scores))
    idx = np.argpartition(-scores, k - 1)[:k]
    idx = sorted(idx, key=lambda i: (-scores[i], ids[i]))
    return [(ids[i], float(scores[i])) for i in idx if scores[i] > 0 or not positive]


def legs_for(ds, b, enc, runs_dir):
    """{leg: {qid: [(doc_id, score)]}} for every leg whose encoding exists."""
    legs = {"bm25": {q: [tuple(x) for x in v] for q, v in
                     json.loads((Path(runs_dir) / f"{ds}_bm25.json").read_text()).items()}}
    groups = sorted({d.group for d in b.docs})
    rows = {g: [i for i, d in enumerate(b.docs) if d.group == g] for g in groups}
    for m in SPARSE:
        p = Path(enc) / m / f"{ds}.npz"
        if not p.exists():
            continue
        X = sp.load_npz(p).tocsc()
        qenc = sparse_query_encoder(m)
        run = {}
        for g in groups:
            sub = X[rows[g]] if len(groups) > 1 else X
            ids = [b.docs[i].id for i in rows[g]]
            for q in (q for q in b.queries if q.group == g):
                tid, w = qenc(q.text)
                s = np.asarray(sub[:, tid] @ w).ravel() if len(tid) else np.zeros(len(ids))
                run[q.id] = top(s, ids)
        legs[m] = run
    for m in DENSE:
        p = Path(enc) / m / f"{ds}.npz"
        if not p.exists():
            continue
        z = np.load(p)
        D, Q = z["docs"], z["queries"]
        qrow = {qid: i for i, qid in enumerate(z["qids"].tolist())}
        run = {}
        for g in groups:
            sub = D[rows[g]]
            ids = [b.docs[i].id for i in rows[g]]
            for q in (q for q in b.queries if q.group == g):
                s = sub @ Q[qrow[q.id]]
                run[q.id] = top(s, ids, positive=False)
        legs[m] = run
    return legs


def rrf(lists, weights=None):
    out = {}
    for j, lst in enumerate(lists):
        w = 1.0 if weights is None else weights[j]
        for r, (d, _) in enumerate(lst, 1):
            out[d] = out.get(d, 0.0) + w / (RRF_K + r)
    return sorted(out.items(), key=lambda x: (-x[1], x[0]))[:K]


def cc(lists, weights):
    out = {}
    for w, lst in zip(weights, lists):
        if not lst or w == 0:
            continue
        s = [x for _, x in lst]
        lo, hi = min(s), max(s)
        for d, x in lst:
            out[d] = out.get(d, 0.0) + w * ((x - lo) / (hi - lo) if hi > lo else 1.0)
    return sorted(out.items(), key=lambda x: (-x[1], x[0]))[:K]


def as_runs(run):
    return {q: {"ranked": [d for d, _ in lst], "latency_ms": 0.0} for q, lst in run.items()}


def ndcg(b, run):
    """Mean nDCG@10 over the headline queries (LoCoMo's adversarial left out)."""
    per = bench.score(b, as_runs(run))
    return float(np.mean([per[q.id]["nDCG@10"] for q in b.queries
                          if q.category != "adversarial"]))


def fuse(legs, names, kind, weights=None):
    qids = legs[names[0]].keys()
    if kind == "rrf":
        return {q: rrf([legs[n][q] for n in names]) for q in qids}
    return {q: cc([legs[n][q] for n in names], weights) for q in qids}


def grid(n):
    if n == 2:
        return [(round(float(w), 2), round(1 - float(w), 2)) for w in np.arange(0, 1.0001, 0.05)]
    pts = []
    for a in range(11):
        for c in range(11 - a):
            pts.append((a / 10, c / 10, round((10 - a - c) / 10, 1)))
    return pts


def combos(legs):
    sparse = [m for m in SPARSE if m in legs]
    dense = [m for m in DENSE if m in legs]
    out = [("bm25", s) for s in sparse] + [("bm25", d) for d in dense]
    out += [("bm25", s, d) for s in sparse for d in dense]
    return out


def write(out, ds, b, method, run, extra=None):
    runs = as_runs(run)
    per = bench.score(b, runs)
    summary = bench.summarize(b, runs, per)
    for row in summary.values():
        row["p50_ms"] = row["p95_ms"] = None
    result = {"dataset": ds, "pipeline": method, "k": K, "queries": len(b.queries),
              "docs": len(b.docs), "success_rel": b.success_rel, "notes": b.notes,
              "summary": summary, **(extra or {}),
              "per_query": {q: {**runs[q], "scores": per[q]} for q in runs}}
    (out / f"{ds}_{method}.json").write_text(json.dumps(result))
    head = summary.get("headline", summary["all"])
    print(f"{ds:9} {method:40} " + " ".join(f"{m} {head[m]:.4f}" for m in
          ["nDCG@10", "R@10", "Success@1", "Success@5"]), flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--enc", required=True)
    ap.add_argument("--runs", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", type=int, default=20261104)
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    benches = {ds: ADAPTERS[ds]().load(CACHE, a.seed, None) for ds in a.datasets}
    legs = {ds: legs_for(ds, b, a.enc, a.runs) for ds, b in benches.items()}

    # Tune CC weights on the synthetic set alone.
    wpath = out / "weights.json"
    weights = json.loads(wpath.read_text()) if wpath.exists() else {}
    cpath = out / "curves.json"
    curves = json.loads(cpath.read_text()) if cpath.exists() else {}
    syn = benches.get("synthetic")
    for c in combos(legs.get("synthetic", {})):
        key = "+".join(c)
        if key in weights or syn is None:
            continue
        curve = {w: round(ndcg(syn, fuse(legs["synthetic"], c, "cc", w)), 4)
                 for w in grid(len(c))}
        best = max(curve, key=lambda w: (curve[w], w[0]))
        weights[key] = list(best)
        curves[key] = [[*w, v] for w, v in curve.items()]
        print("tuned", key, best, flush=True)
    wpath.write_text(json.dumps(weights, indent=1))
    cpath.write_text(json.dumps(curves))

    # For the report only: each set's own nDCG@10 curve over the two-leg weights,
    # to show how far the synthetic-tuned weight sits from that set's best.
    # Never used to choose a weight.
    opath = out / "oracle.json"
    oracle = json.loads(opath.read_text()) if opath.exists() else {}
    for ds, b in benches.items():
        for c in (c for c in combos(legs[ds]) if len(c) == 2):
            key = f"{ds}:{'+'.join(c)}"
            if key not in oracle:
                oracle[key] = [[*w, round(ndcg(b, fuse(legs[ds], c, "cc", w)), 4)]
                               for w in grid(2)]
    opath.write_text(json.dumps(oracle))

    for ds, b in benches.items():
        L = legs[ds]
        for name in L:
            write(out, ds, b, name, L[name])
        for c in combos(L):
            key = "+".join(c)
            write(out, ds, b, f"rrf-{key}", fuse(L, c, "rrf"), {"rrf_k": RRF_K})
            if key in weights:
                write(out, ds, b, f"cc-{key}", fuse(L, c, "cc", weights[key]),
                      {"weights": dict(zip(c, weights[key])), "tuned_on": "synthetic"})


if __name__ == "__main__":
    main()
