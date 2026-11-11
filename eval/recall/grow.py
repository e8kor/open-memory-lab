#!/usr/bin/env python3
"""Builds the grown recall set from the drafter's output (ADR-0075 d8, phase 0a).

    python3 eval/recall/grow.py build eval/recall/grow/pass1.jsonl > eval/recall/queries-grown.json
    python3 eval/recall/grow.py drafts eval/recall/grow/pass1.jsonl

`grow/drafter` asks a local model (Phi-3-mini-4k-instruct, MIT; which model
wrote the corpus is unknown, so whether it is of another family, as ADR-0075
d8 asks, is unverified) for two question-shaped searches (Q1, Q2) and
two paraphrases (P1, P2) per memory, each with the words of the memory that
answer it. Every draft then passes these checks, in this order, or is dropped
with the first reason it fails; `drafts` prints every draft with its verdict:

- `unparsed`: no search line, or no answer line after it;
- `length`: under 3 or over 30 words, or over 200 characters;
- `not_question`: a Q draft that does not end with a question mark;
- `meta`: names the note or the set ("note", "notes", "mentioned", "this memory");
- `span_missing`: the answer span, normalised, is not in the memory's text,
  or is under 3 characters: the label's deterministic check;
- `answer_in_query`: the answer span appears in the search as whole words;
- `not_paraphrase`: a P draft that shares more than half of its content words
  with the memory (5-character stems, run.py's stop words);
- `duplicate`: the normalised search repeats one of the 121 synthetic queries
  or an earlier kept draft, or shares at least 0.8 of its word set with one
  (Jaccard), so nothing leaks from the synthetic set into the grown one.

Of each memory's surviving drafts, at most four are kept, in the order Q1,
P1, Q2, P2 (then a later pass's, in the same order). Each kept query labels
its own memory grade 2 with the answer span. Its fold (0 to 4) is its
memory's: the 200 memories are ordered by SHA-256 of their id and dealt in
turn, so every query of one memory sits in one fold and five-fold
cross-validation never tunes on a memory's query and scores on its sibling.
A seeded 10% sample is listed for a person's label check; `grow/person_check.json`
records that check and the labels it corrected, which `build` applies by query id
(the query keeps the fold of the memory it was drafted from). Standard library only.
"""

import argparse
import hashlib
import json
import random
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import metrics  # noqa: E402
from run import STOP  # noqa: E402

GROW = HERE / "grow"
PROMPT = GROW / "prompt.txt"
PERSON_CHECK = GROW / "person_check.json"
WRITER = {
    "model": "microsoft/Phi-3-mini-4k-instruct",
    "family": "Phi (Microsoft)",
    "licence": "MIT",
    "weights": "microsoft/Phi-3-mini-4k-instruct-gguf@a64113399c2f6b8ad3e11c394733a2ddadaa7f33"
               " Phi-3-mini-4k-instruct-q4.gguf",
    "weights_sha256": "8a83c7fb9049a9b2e92266fa7ad04933bb53aa1e85136b7b30f1b8000ff2edef",
    "tokenizer": "microsoft/Phi-3-mini-4k-instruct@f39ac1d28e925b323eae81227eaba4464caced4e"
                 " tokenizer.json",
    "tokenizer_sha256": "072ab882d6c7192a42f78790945d16c064691321a73251a4b18f6a380f0fbe39",
    "runtime": "eval/recall/grow/drafter (candle-transformers 0.11.1 quantized_phi3, CPU)",
    "decoding": {"temperature": 0.7, "top_p": 0.9, "max_new_tokens": 240,
                 "seed": "pass seed + the memory's index in corpus.json"},
}
PASSES = {"pass1": 20261111, "pass2": 20261112}
SLOTS = {"Q1": "question", "Q2": "question", "P1": "paraphrase", "P2": "paraphrase"}
ORDER = ["Q1", "P1", "Q2", "P2"]
PER_MEMORY = 4
FOLDS = 5
SAMPLE_SEED = 20261111
META = re.compile(r"\b(notes?|mentioned|this memory|the memory)\b", re.I)
LINE = re.compile(r"^\s*\**\s*([QPA])\s*(\d)\s*\**\s*[:.)-]\s*(.*)$")


def norm(s):
    s = s.strip().strip("\"'`").strip()
    s = re.sub(r"\s+", " ", s.lower())
    return s.rstrip(".;:,!").strip()


def words(s):
    return re.findall(r"[a-z0-9][a-z0-9_.\-/]*[a-z0-9]|[a-z0-9]", s.lower())


def content(s):
    return {w[:5] for w in words(s) if len(w) >= 3 and w not in STOP}


def parse(raw):
    """[(slot, query, span)] in the order written; a search keeps the next answer line."""
    out, seen, pending = [], set(), None
    for line in raw.splitlines():
        m = LINE.match(line)
        if not m:
            continue
        kind, num, body = m.group(1), m.group(2), m.group(3).strip()
        if kind in "QP":
            pending = f"{kind}{num}", body
        elif pending:
            slot, query = pending
            pending = None
            if slot in SLOTS and slot not in seen:
                seen.add(slot)
                out.append((slot, query, body))
    return out


def check(slot, query, span, memory, kept):
    q = query.strip().strip("\"'`").strip()
    n = len(q.split())
    if not q or n < 3 or n > 30 or len(q) > 200:
        return "length"
    if slot.startswith("Q") and not q.endswith("?"):
        return "not_question"
    if META.search(q):
        return "meta"
    sp = norm(span)
    if len(sp) < 3 or sp not in norm(memory):
        return "span_missing"
    if re.search(r"(?<![a-z0-9])" + re.escape(sp) + r"(?![a-z0-9])", q.lower()):
        return "answer_in_query"
    if slot.startswith("P"):
        cw = content(q)
        if cw and len(cw & content(memory)) / len(cw) > 0.5:
            return "not_paraphrase"
    nq, wq = norm(q), set(words(q))
    for other in kept:
        wo = set(words(other))
        if nq == norm(other) or (wq and len(wq & wo) / len(wq | wo) >= 0.8):
            return "duplicate"
    return None


def drafts(files):
    """Every draft in pass order: (pass, memory_id, slot, query, span)."""
    rows = []
    for f in files:
        name = Path(f).stem
        if name not in PASSES:
            raise SystemExit(f"{f}: a pass file is named after its pass ({', '.join(PASSES)})")
        for line in Path(f).read_text().splitlines():
            d = json.loads(line)
            for slot, query, span in parse(d["raw"]):
                rows.append((name, d["memory_id"], slot, query, span))
    return rows


def judge(files):
    memories, synthetic = metrics.load()
    text = {m["id"]: m["text"] for m in memories}
    kept_text = [q["query"] for q in synthetic]
    rank = {s: i for i, s in enumerate(ORDER)}
    rows = sorted(drafts(files), key=lambda r: (r[1], list(PASSES).index(r[0]), rank[r[2]]))
    verdicts, per = [], {}
    for p, mid, slot, query, span in rows:
        reason = check(slot, query, span, text[mid], kept_text)
        if reason is None and per.get(mid, 0) >= PER_MEMORY:
            reason = "memory_full"
        if reason is None:
            per[mid] = per.get(mid, 0) + 1
            kept_text.append(query.strip().strip("\"'`").strip())
        verdicts.append({"pass": p, "memory_id": mid, "slot": slot, "query": query,
                         "answer_span": span, "dropped": reason})
    return memories, verdicts


def folds(memories):
    order = sorted((m["id"] for m in memories),
                   key=lambda i: hashlib.sha256(i.encode()).hexdigest())
    return {mid: i % FOLDS for i, mid in enumerate(order)}


def build(files):
    memories, verdicts = judge(files)
    fold = folds(memories)
    queries = []
    for v in verdicts:
        if v["dropped"]:
            continue
        mid = v["memory_id"]
        queries.append({
            "id": f"g{len(queries) + 1:04d}",
            "category": SLOTS[v["slot"]],
            "query": v["query"].strip().strip("\"'`").strip(),
            "relevant": {mid: 2},
            "answer_span": {mid: v["answer_span"].strip()},
            "fold": fold[mid],
            "origin": {"writer": "model", "pass": v["pass"], "slot": v["slot"]},
        })
    sample = sorted(random.Random(SAMPLE_SEED).sample([q["id"] for q in queries],
                                                       -(-len(queries) // 10)))
    check = json.loads(PERSON_CHECK.read_text()) if PERSON_CHECK.exists() else None
    by_id = {q["id"]: q for q in queries}
    for qid, c in ((check or {}).get("corrections") or {}).items():
        q = by_id[qid]
        if q["relevant"] != c["was"]:
            raise SystemExit(f"{PERSON_CHECK}: {qid} no longer labels {c['was']}")
        q["relevant"], q["answer_span"] = c["relevant"], c["answer_span"]
        q["origin"]["corrected_by"] = "person_check"
    dropped = {}
    for v in verdicts:
        if v["dropped"]:
            dropped[v["dropped"]] = dropped.get(v["dropped"], 0) + 1
    return {
        "version": 2 if check else 1,
        "corpus_version": json.loads((HERE / "corpus.json").read_text())["version"],
        "adr": "ADR-0075 d8, phase 0a (#235)",
        "provenance": {
            "writer": WRITER,
            "prompt": {"file": "eval/recall/grow/prompt.txt", "version": "oml-recall-grow/1",
                       "sha256": hashlib.sha256(PROMPT.read_bytes()).hexdigest()},
            "passes": {p: {"seed": s} for p, s in PASSES.items() if any(v["pass"] == p for v in verdicts)},
            "corpus_writer": (check or {}).get("corpus_writer") or (
                "one model wrote corpus.json and queries.json (REPORT.md, Limits); "
                "that it is not of the Phi family is part of the person's check"),
            "drafts": len(verdicts),
            "dropped": dict(sorted(dropped.items())),
            "labels": "each query labels the memory it was drafted from, grade 2, and the answer "
                      "span the model copied from it, checked to be in that memory's text; "
                      "person_check.corrections lists the labels a person replaced",
            "folds": "by memory: corpus ids ordered by SHA-256, dealt into 5 folds in turn",
        },
        "person_check": ({"sample": sample, "seed": SAMPLE_SEED, "checked_by": check["checked_by"],
                          "date": check["date"], "status": "done", "checked": check["checked"],
                          "marks": check["marks"], "not_correct": check["not_correct"],
                          "corrected_labels": check["corrected_labels"],
                          "finding": check["finding"],
                          "corrections": {k: {"was": v["was"], "mark": v["mark"], "note": v["note"],
                                              "decision": v["decision"]}
                                          for k, v in check["corrections"].items()}}
                         if check else {"sample": sample, "seed": SAMPLE_SEED, "checked_by": None,
                                        "status": "pending"}),
        "queries": queries,
    }


def render(files):
    """queries-grown.json's exact text, as `build` prints it."""
    return json.dumps(build(files), indent=1, ensure_ascii=False) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("what", choices=["build", "drafts"])
    ap.add_argument("files", nargs="+")
    a = ap.parse_args()
    if a.what == "build":
        sys.stdout.write(render(a.files))
    else:
        for v in judge(a.files)[1]:
            print(json.dumps(v, ensure_ascii=False))


if __name__ == "__main__":
    main()
