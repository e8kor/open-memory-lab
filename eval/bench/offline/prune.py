#!/usr/bin/env python3
"""What a weight floor and a per-version token cap (ADR-0052 Q6) cost in quality.

    prune.py --enc ENC --runs RUNS --weights RAW/weights.json --model sparse-distill <dataset>...

For each setting, every document vector keeps only its weights at or above the
floor and, of those, its `max_tokens` largest; then the sparse leg alone and
its synthetic-tuned convex combination with bm25 are scored (nDCG@10 over the
headline queries) beside the kept non-zero tokens per document. Prints a
markdown table and writes it to stdout only.
"""

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import scipy.sparse as sp

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import evaluate  # noqa: E402
from adapters import ADAPTERS  # noqa: E402

SETTINGS = [(0.0, None), (0.05, None), (0.1, None), (0.2, None), (0.3, None),
            (0.0, 128), (0.0, 64), (0.0, 32), (0.1, 128), (0.1, 64)]


def prune(X, floor, cap):
    X = X.tocsr(copy=True)
    X.data[X.data < floor] = 0
    X.eliminate_zeros()
    if cap:
        for i in range(X.shape[0]):
            a, b = X.indptr[i], X.indptr[i + 1]
            if b - a > cap:
                row = X.data[a:b]
                cut = np.partition(row, b - a - cap)[b - a - cap]
                row[row < cut] = 0
        X.eliminate_zeros()
    return X


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--enc", required=True)
    ap.add_argument("--runs", required=True)
    ap.add_argument("--weights", required=True)
    ap.add_argument("--model", default="sparse-distill")
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()
    w = json.loads(Path(a.weights).read_text())[f"bm25+{a.model}"]
    benches = {ds: ADAPTERS[ds]().load(evaluate.CACHE, 20261104, None) for ds in a.datasets}
    full = {ds: sp.load_npz(Path(a.enc) / a.model / f"{ds}.npz") for ds in a.datasets}
    tmp = Path(a.enc) / "_prune"
    print("| floor | max tokens | " + " | ".join(
        f"{ds} nnz / sparse / cc" for ds in a.datasets) + " |")
    print("|---:|---:|" + "---|" * len(a.datasets))
    for floor, cap in SETTINGS:
        cells = []
        for ds, b in benches.items():
            X = prune(full[ds], floor, cap)
            (tmp / a.model).mkdir(parents=True, exist_ok=True)
            sp.save_npz(tmp / a.model / f"{ds}.npz", X)
            legs = evaluate.legs_for(ds, b, tmp, a.runs)
            s = evaluate.ndcg(b, legs[a.model])
            c = evaluate.ndcg(b, evaluate.fuse(legs, ("bm25", a.model), "cc", w))
            cells.append(f"{np.diff(X.indptr).mean():.0f} / {s:.3f} / {c:.3f}")
        print(f"| {floor} | {cap or '–'} | " + " | ".join(cells) + " |", flush=True)


if __name__ == "__main__":
    main()
