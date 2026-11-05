#!/usr/bin/env python3
"""Encode each dataset's memories (and, for dense models, its queries) offline.

    encode.py --enc DIR <model> <dataset>...

Models (all licences allow commercial use):
- `sparse-distill`: opensearch-neural-sparse-encoding-doc-v3-distill (Apache-2.0, 67M);
- `sparse-gte`: opensearch-neural-sparse-encoding-doc-v3-gte (Apache-2.0, 133M);
- `bge-small`: BAAI/bge-small-en-v1.5 (MIT, 33M);
- `arctic-s`: Snowflake/snowflake-arctic-embed-s (Apache-2.0, 33M).

Sparse documents follow the model card: the MLM logits, max over tokens under
the attention mask, `log(1 + log(1 + relu(x)))`, special tokens zeroed. A
sparse query never meets the model: its tokenizer's ids, each set to 1 and
weighted by the card's `idf.json` (retrieve.py does that). Dense models embed
with CLS pooling and unit length, queries with the retrieval prompt the card
names. Writes DIR/<model>/<dataset>.npz and a timing line per dataset to
DIR/<model>/cost.json: document encode throughput and, for sparse models, the
non-zero tokens per document. Queries are encoded in batches here; cost.py
measures one-query latency.
"""

import argparse
import json
import statistics
import sys
import time
from pathlib import Path

import numpy as np
import scipy.sparse as sp
import torch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from adapters import ADAPTERS  # noqa: E402

MODELS = {
    "sparse-distill": "opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill",
    "sparse-gte": "opensearch-project/opensearch-neural-sparse-encoding-doc-v3-gte",
    "bge-small": "BAAI/bge-small-en-v1.5",
    "arctic-s": "Snowflake/snowflake-arctic-embed-s",
}
QUERY_PROMPT = "Represent this sentence for searching relevant passages: "
CACHE = "/srv/build/bench-data"
BATCH = 32


def batches(texts):
    order = sorted(range(len(texts)), key=lambda i: len(texts[i]))
    for s in range(0, len(order), BATCH):
        idx = order[s:s + BATCH]
        yield idx, [texts[i] for i in idx]


def encode_sparse(name, texts):
    from transformers import AutoModelForMaskedLM, AutoTokenizer

    tok = AutoTokenizer.from_pretrained(MODELS[name], trust_remote_code=True)
    model = AutoModelForMaskedLM.from_pretrained(MODELS[name], trust_remote_code=True).eval()
    if name == "sparse-gte":
        # The checkpoint stores no lm_head.decoder.weight: it is tied to the
        # word embeddings, and transformers 5 does not tie it for the remote
        # NewForMaskedLM (its config has no tie_word_embeddings), leaving it
        # randomly initialised. Tie it as the checkpoint intends.
        model.get_output_embeddings().weight = model.get_input_embeddings().weight
        # Its non-persistent buffers (the rotary inv_freq and cos/sin cache, the
        # embeddings' position_ids) are not in the checkpoint and come back
        # uninitialised under transformers 5's meta-device load: rebuild them
        # as the remote code's own constructors do.
        for parent in model.modules():
            for cname, child in parent.named_children():
                if type(child).__name__ == "RotaryEmbedding":
                    setattr(parent, cname, type(child)(child.dim, child.max_position_embeddings,
                                                       child.base))
            if isinstance(getattr(parent, "position_ids", None), torch.Tensor):
                parent.position_ids = torch.arange(parent.position_ids.size(0))
        # transformers 5 dropped get_extended_attention_mask, which the remote
        # code calls; this is transformers 4's body for a 2-D padding mask.

        def extended(self, mask, input_shape, dtype=torch.float32):
            ext = mask[:, None, None, :].to(dtype)
            return (1.0 - ext) * torch.finfo(dtype).min
        type(model.base_model).get_extended_attention_mask = extended
    special = torch.tensor(sorted(set(tok.all_special_ids)))
    rows = [None] * len(texts)
    t0 = time.perf_counter()
    with torch.inference_mode():
        for idx, chunk in batches(texts):
            f = tok(chunk, padding=True, truncation=True, max_length=512,
                    return_tensors="pt", return_token_type_ids=False)
            logits = model(**f)[0]
            v, _ = torch.max(logits * f["attention_mask"].unsqueeze(-1), dim=1)
            v = torch.log(1 + torch.log(1 + torch.relu(v)))
            v[:, special] = 0
            m = sp.csr_matrix(v.numpy())
            for j, i in enumerate(idx):
                rows[i] = m[j]
    secs = time.perf_counter() - t0
    X = sp.vstack(rows).tocsr()
    X.eliminate_zeros()
    return {"docs": X}, secs, None


def encode_dense(name, texts, queries, time_queries=False):
    from sentence_transformers import SentenceTransformer

    model = SentenceTransformer(MODELS[name], device="cpu")
    model.max_seq_length = 512
    t0 = time.perf_counter()
    D = model.encode(texts, batch_size=BATCH, normalize_embeddings=True,
                     convert_to_numpy=True, show_progress_bar=False)
    secs = time.perf_counter() - t0
    qtexts = [QUERY_PROMPT + q for q in queries]
    lat = []
    if not qtexts:
        return {"docs": D.astype(np.float32)}, secs, lat
    if not time_queries:
        Q = model.encode(qtexts, batch_size=BATCH, normalize_embeddings=True,
                         convert_to_numpy=True, show_progress_bar=False)
        return {"docs": D.astype(np.float32), "queries": Q.astype(np.float32)}, secs, lat
    model.encode(qtexts[:4], normalize_embeddings=True)  # warm up
    Q = []
    for q in qtexts:
        t = time.perf_counter()
        Q.append(model.encode([q], normalize_embeddings=True, convert_to_numpy=True,
                              show_progress_bar=False)[0])
        lat.append((time.perf_counter() - t) * 1000)
    return {"docs": D.astype(np.float32), "queries": np.stack(Q).astype(np.float32)}, secs, lat


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--enc", required=True)
    ap.add_argument("--seed", type=int, default=20261104)
    ap.add_argument("--threads", type=int, default=8)
    ap.add_argument("model", choices=MODELS)
    ap.add_argument("datasets", nargs="+")
    a = ap.parse_args()
    torch.set_num_threads(a.threads)
    out = Path(a.enc) / a.model
    out.mkdir(parents=True, exist_ok=True)
    cost_p = out / "cost.json"
    cost = json.loads(cost_p.read_text()) if cost_p.exists() else {}
    for ds in a.datasets:
        b = ADAPTERS[ds]().load(CACHE, a.seed, None)
        texts = [d.text for d in b.docs]
        if a.model.startswith("sparse"):
            arrs, secs, lat = encode_sparse(a.model, texts)
            X = arrs["docs"]
            sp.save_npz(out / f"{ds}.npz", X)
            nnz = np.diff(X.indptr)
            extra = {"nnz_mean": round(float(nnz.mean()), 1),
                     "nnz_p50": int(np.median(nnz)), "nnz_p95": int(np.percentile(nnz, 95)),
                     "nnz_max": int(nnz.max())}
        else:
            arrs, secs, lat = encode_dense(a.model, texts, [q.text for q in b.queries])
            np.savez(out / f"{ds}.npz", docs=arrs["docs"], queries=arrs["queries"],
                     qids=np.array([q.id for q in b.queries]))
            extra = {"dims": int(arrs["docs"].shape[1])}
        cost[ds] = {"docs": len(texts), "encode_seconds": round(secs, 1),
                    "docs_per_second": round(len(texts) / secs, 1),
                    "mean_chars": round(statistics.fmean(len(t) for t in texts)),
                    "threads": torch.get_num_threads(), "batch": BATCH, **extra}
        cost_p.write_text(json.dumps(cost, indent=1))
        print(a.model, ds, json.dumps(cost[ds]), flush=True)


if __name__ == "__main__":
    main()
