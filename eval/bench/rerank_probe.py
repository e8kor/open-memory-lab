#!/usr/bin/env python3
"""Offline cross-encoder probe: which reranker, how deep, before any is served.

    rerank_probe.py --raw <bench.py --out dir> --models <dir>... --depth 10 20 50 \
        --out <json> <dataset>...

It reads the first-stage lists a served run already wrote (`bench.py run`,
`<dataset>_bm25.json`, k 100), reorders the top `depth` of each list with
each cross-encoder through onnxruntime on the CPU, leaves the rest of the
list in its served order, and scores the result with bench.py's own scorer
(ir_measures), so a number here sits beside the served one it starts from.
Each model directory holds `model.onnx`, `tokenizer.json` and
`config.json`, as the model's repository publishes them.

Latency is the model's alone: one query's `depth` pairs as one batch, on
`--threads` intra-op threads, the median and p95 over the queries asked; it
ranks the models against each other, not the served path, which runs its
own runtime (ADR-0053).

Writes one JSON with, per dataset, model and depth, the metrics over every
query and per category, and keeps each query's top-1 cross-encoder score so
a relevance floor can be swept without running a model again.
"""

import argparse
import json
import statistics
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import bench  # noqa: E402
from adapters import ADAPTERS  # noqa: E402

MAX_LEN = 512


class CrossEncoder:
    def __init__(self, path, threads, max_len):
        import onnxruntime as ort
        from tokenizers import Tokenizer

        opts = ort.SessionOptions()
        opts.intra_op_num_threads = threads
        opts.inter_op_num_threads = 1
        self.session = ort.InferenceSession(str(path / "model.onnx"), opts,
                                            providers=["CPUExecutionProvider"])
        self.inputs = {i.name for i in self.session.get_inputs()}
        self.tok = Tokenizer.from_file(str(path / "tokenizer.json"))
        self.tok.enable_truncation(max_length=max_len, strategy="only_second")
        self.tok.enable_padding()
        self.name = path.name

    def scores(self, query, passages):
        import numpy as np

        enc = self.tok.encode_batch([(query, p) for p in passages])
        feed = {"input_ids": np.array([e.ids for e in enc], dtype=np.int64),
                "attention_mask": np.array([e.attention_mask for e in enc], dtype=np.int64)}
        if "token_type_ids" in self.inputs:
            feed["token_type_ids"] = np.array([e.type_ids for e in enc], dtype=np.int64)
        logits = self.session.run(None, feed)[0]
        return [float(x) for x in logits.reshape(len(passages), -1)[:, 0]]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=bench.DEFAULT_CACHE)
    ap.add_argument("--seed", type=int, default=20261104)
    ap.add_argument("--raw", required=True, help="bench.py run's --out directory")
    ap.add_argument("--first-stage", default="bm25")
    ap.add_argument("--models", nargs="+", required=True)
    ap.add_argument("--depth", nargs="+", type=int, default=[10, 20, 50])
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--max-len", type=int, default=MAX_LEN)
    ap.add_argument("--limit", type=int, default=None, help="at most this many queries, seeded")
    ap.add_argument("--out", required=True)
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()

    out = json.loads(Path(a.out).read_text()) if Path(a.out).exists() else {}
    for name in a.datasets:
        b = ADAPTERS[name]().load(a.cache, a.seed, a.limit)
        text = {d.id: d.text for d in b.docs}
        raw = json.loads((Path(a.raw) / f"{name}_{a.first_stage.replace('-', '_')}.json")
                         .read_text())["per_query"]
        queries = [q for q in b.queries if q.id in raw]
        first = {q.id: {"ranked": raw[q.id]["ranked"], "latency_ms": raw[q.id]["latency_ms"]}
                 for q in queries}
        b.queries = queries
        base = bench.summarize(b, first, bench.score(b, first))
        res = out.setdefault(name, {})
        res[a.first_stage] = {"summary": base}
        print(f"{name} {a.first_stage}: {json.dumps(base.get('headline', base['all']))}")
        for mdir in a.models:
            model = CrossEncoder(Path(mdir), a.threads, a.max_len)
            deepest = max(a.depth)
            scored, lat = {}, {d: [] for d in a.depth}
            for q in queries:
                cands = [d for d in first[q.id]["ranked"][:deepest] if d is not None]
                s_all = {}
                for depth in sorted(a.depth):
                    t0 = time.perf_counter()
                    sc = model.scores(q.text, [text[d] for d in cands[:depth]]) if cands else []
                    lat[depth].append((time.perf_counter() - t0) * 1000)
                    for d, s in zip(cands[:depth], sc):
                        s_all.setdefault(d, s)
                scored[q.id] = s_all
            for depth in a.depth:
                runs, top = {}, {}
                for q in queries:
                    ranked = first[q.id]["ranked"]
                    head = sorted(ranked[:depth], key=lambda d: -scored[q.id].get(d, -1e9))
                    runs[q.id] = {"ranked": head + ranked[depth:], "latency_ms": 0.0}
                    top[q.id] = scored[q.id].get(head[0]) if head else None
                summ = bench.summarize(b, runs, bench.score(b, runs))
                ms = sorted(lat[depth])
                key = f"{model.name}@{depth}"
                res[key] = {
                    "summary": summ,
                    "model_ms": {"p50": round(statistics.median(ms), 1),
                                 "p95": round(ms[min(len(ms) - 1, int(0.95 * len(ms)))], 1)},
                    "top1_score": top,
                    "category": {q.id: q.category for q in queries},
                }
                h = summ.get("headline", summ["all"])
                print(f"{name} {key}: nDCG@10 {h['nDCG@10']} S@1 {h['Success@1']} "
                      f"S@5 {h['Success@5']} model p50 {res[key]['model_ms']['p50']} ms "
                      f"p95 {res[key]['model_ms']['p95']} ms", flush=True)
            Path(a.out).write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
