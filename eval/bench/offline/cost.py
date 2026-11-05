#!/usr/bin/env python3
"""Encode cost of each model on this CPU, on fixed samples, one model at a time.

    cost.py --enc ENC <model>...

For each model: document throughput (docs/s, batch 32, 8 threads, fp32) on the
first 256 SciFact abstracts and the first 512 LoCoMo turns; for dense models,
the p50/p95 of encoding one query at a time over 200 SciFact and LoCoMo
questions at 1, 4 and 8 threads (on a shared host 8 threads oversubscribe);
for sparse models, the p50/p95 of the read-side query encoding
(tokenizer + idf lookup, no model). The 1-minute load average before and after
is recorded, since the host is shared. Writes ENC/cost-clean.json.
"""

import argparse
import json
import os
import statistics
import sys
import time
from pathlib import Path

import torch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import encode  # noqa: E402
from adapters import ADAPTERS  # noqa: E402


def pcts(lat):
    lat = sorted(lat)
    return {"p50_ms": round(statistics.median(lat), 2),
            "p95_ms": round(lat[min(len(lat) - 1, int(0.95 * len(lat)))], 2)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--enc", required=True)
    ap.add_argument("models", nargs="+")
    a = ap.parse_args()
    torch.set_num_threads(8)
    sci = ADAPTERS["scifact"]().load(encode.CACHE, 20261104, None)
    loc = ADAPTERS["locomo"]().load(encode.CACHE, 20261104, None)
    samples = {"scifact-256": [d.text for d in sci.docs[:256]],
               "locomo-512": [d.text for d in loc.docs[:512]]}
    queries = [q.text for q in sci.queries[:100]] + [q.text for q in loc.queries[:100]]
    p = Path(a.enc) / "cost-clean.json"
    res = json.loads(p.read_text()) if p.exists() else {}
    for m in a.models:
        row = {"load_before": round(os.getloadavg()[0], 2)}
        for name, texts in samples.items():
            if m.startswith("sparse"):
                _, secs, _ = encode.encode_sparse(m, texts)
            else:
                _, secs, _ = encode.encode_dense(m, texts, [])
            row[name] = {"docs": len(texts), "seconds": round(secs, 2),
                         "docs_per_second": round(len(texts) / secs, 1)}
        if m.startswith("sparse"):
            import evaluate
            qenc = evaluate.sparse_query_encoder(m)
            lat = []
            for q in queries:
                t = time.perf_counter()
                qenc(q)
                lat.append((time.perf_counter() - t) * 1000)
            row["query_encode"] = {"what": "tokenizer + idf lookup, no model", **pcts(lat)}
        else:
            for n in (1, 4, 8):
                torch.set_num_threads(n)
                _, _, lat = encode.encode_dense(m, ["warm"], queries, time_queries=True)
                row[f"query_encode_{n}_threads"] = {"what": f"one query per call, {n} threads",
                                                    **pcts(lat)}
            torch.set_num_threads(8)
        row["load_after"] = round(os.getloadavg()[0], 2)
        res[m] = row
        p.write_text(json.dumps(res, indent=1))
        print(m, json.dumps(row), flush=True)


if __name__ == "__main__":
    main()
