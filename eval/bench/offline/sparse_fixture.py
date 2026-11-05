#!/usr/bin/env python3
"""The parity fixture for oml-rerank's doc-v3-distill encoder (ADR-0052 d13).

    sparse_fixture.py --model-dir DIR --out FILE

Encodes a fixed sample with the reference this lane's offline experiment used
(encode.py's `encode_sparse`, the model card's method: MLM logits, max over
the tokens under the attention mask, log(1 + log(1 + relu(x))), special
tokens zeroed, truncation at 512) and the query side of evaluate.py (the
tokenizer's ids, special tokens dropped, each once, weighted by idf.json).
Writes, per document, its input ids and every non-zero (token id, weight),
and per query its (token id, idf) pairs. The Rust encoder's test reads the
file and holds its output to it: identical ids, weights within 1e-4.

The sample: the first 12 memories of eval/recall/corpus.json, its first 12
queries, two edge cases (an empty claim, one past 512 tokens) and one
non-English line, all one at a time, so no padding is involved.
"""

import argparse
import json
from pathlib import Path

import torch
from transformers import AutoModelForMaskedLM, AutoTokenizer

ROOT = Path(__file__).resolve().parents[3]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model-dir", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    torch.set_num_threads(4)
    tok = AutoTokenizer.from_pretrained(a.model_dir)
    model = AutoModelForMaskedLM.from_pretrained(a.model_dir).eval()
    special = sorted(set(tok.all_special_ids))
    idf_raw = json.loads((Path(a.model_dir) / "idf.json").read_text())
    idf = {tok.convert_tokens_to_ids(t): w for t, w in idf_raw.items()}

    corpus = json.loads((ROOT / "eval/recall/corpus.json").read_text())["memories"]
    queries = json.loads((ROOT / "eval/recall/queries.json").read_text())["queries"]
    docs = [m["text"] for m in corpus[:12]]
    docs += ["", " ".join(m["text"] for m in corpus[:40]),
             "Wdrażamy usługę API z wdrożeniem kanarkowym."]
    out = {"model": "opensearch-neural-sparse-encoding-doc-v3-distill",
           "revision": "babf71f3c48695e2e53a978208e8aba48335e3c0",
           "docs": [], "queries": []}
    with torch.inference_mode():
        for text in docs:
            f = tok([text], truncation=True, max_length=512, return_tensors="pt",
                    return_token_type_ids=False)
            logits = model(**f)[0]
            v, _ = torch.max(logits * f["attention_mask"].unsqueeze(-1), dim=1)
            v = torch.log(1 + torch.log(1 + torch.relu(v)))[0]
            v[special] = 0
            nz = torch.nonzero(v).flatten().tolist()
            out["docs"].append({"text": text, "input_ids": f["input_ids"][0].tolist(),
                                "weights": [[i, round(float(v[i]), 7)] for i in nz]})
    for q in queries[:12]:
        ids = sorted({i for i in tok(q["query"], truncation=True, max_length=512)["input_ids"]
                      if i not in special})
        out["queries"].append({"text": q["query"],
                               "weights": [[i, round(float(idf.get(i, 0.0)), 7)] for i in ids]})
    Path(a.out).write_text(json.dumps(out) + "\n")


if __name__ == "__main__":
    main()
