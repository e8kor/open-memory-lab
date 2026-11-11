#!/usr/bin/env python3
"""Generates questions for the recall corpus offline (ADR-0075 d5, phase 0).

    generate.py --model-dir DIR --out eval/recall/questions/doc2query-t5-base.jsonl

The `local` adapter of d5, run offline: a doc2query T5 checkpoint
(doc2query/msmarco-t5-base-v1, Apache-2.0, a T5 fine-tuned on MS MARCO), not
a model of the Phi family, which drafted the grown set's queries (d8: no
generator may win by echoing its own family's phrasing). One call per
memory, the memory's own text alone, cut at 512 tokens; top-k sampling, k 10,
at most 64 new tokens; the seed is the first eight bytes of SHA-256 over the
memory id and the generator id, as d5 sets it. Ten sequences are drawn per
memory: the first five are the pre-registered arm (five questions), all ten
the reported variant. Raw output only: the filters of d6 run in score.py, so
what each one drops is recomputable from this file.

Writes one JSON line per memory, `{id, questions: [...], tokens_in,
tokens_out, seconds}`, then a last line `{"generator": {...}}` with the model,
its digest, the decoding and the wall time. Needs torch and transformers.
"""

import argparse
import hashlib
import json
import time
from pathlib import Path

import torch
from transformers import AutoTokenizer, T5ForConditionalGeneration

ROOT = Path(__file__).resolve().parents[3]
MODEL = "doc2query/msmarco-t5-base-v1"
REVISION = "e673dca0dff1f19fda73ac62420eedf0219e692b"
DRAWN = 10
DECODING = {"do_sample": True, "top_k": 10, "max_new_tokens": 64, "input_max_tokens": 512,
            "num_return_sequences": DRAWN}


def generator_id(digest):
    return f"local/{MODEL}@{REVISION}/{digest[:16]}/top_k10/max5"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model-dir", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--threads", type=int, default=4)
    a = ap.parse_args()
    torch.set_num_threads(a.threads)
    weights = Path(a.model_dir) / "pytorch_model.bin"
    digest = hashlib.sha256(weights.read_bytes()).hexdigest()
    gid = generator_id(digest)
    tok = AutoTokenizer.from_pretrained(a.model_dir)
    model = T5ForConditionalGeneration.from_pretrained(a.model_dir).eval()
    corpus = json.loads((ROOT / "eval/recall/corpus.json").read_text())["memories"]
    start = time.perf_counter()
    with open(a.out, "w") as out, torch.inference_mode():
        for m in corpus:
            seed = int.from_bytes(hashlib.sha256(f"{m['id']}\n{gid}".encode()).digest()[:8], "big")
            torch.manual_seed(seed % (2**63))
            t0 = time.perf_counter()
            enc = tok(m["text"], max_length=512, truncation=True, return_tensors="pt")
            seqs = model.generate(**enc, do_sample=True, top_k=10, max_new_tokens=64,
                                  num_return_sequences=DRAWN)
            qs = [tok.decode(s, skip_special_tokens=True).strip() for s in seqs]
            out_tokens = int(sum(int((s != tok.pad_token_id).sum()) for s in seqs))
            out.write(json.dumps({"id": m["id"], "questions": qs,
                                  "tokens_in": int(enc["input_ids"].shape[1]),
                                  "tokens_out": out_tokens,
                                  "seconds": round(time.perf_counter() - t0, 3)}) + "\n")
        wall = time.perf_counter() - start
        out.write(json.dumps({"generator": {
            "id": gid, "model": MODEL, "revision": REVISION, "weights_sha256": digest,
            "licence": "Apache-2.0 (weights); trained on MS MARCO (terms unchecked, d5's register)",
            "family": "T5 (Google), fine-tuned by doc2query",
            "decoding": DECODING, "seed": "first 8 bytes of SHA-256(memory id, newline, generator id)",
            "threads": a.threads, "torch": torch.__version__, "wall_seconds": round(wall, 1),
            "memories": len(corpus)}}) + "\n")


if __name__ == "__main__":
    main()
