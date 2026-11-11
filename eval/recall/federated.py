#!/usr/bin/env python3
"""The federated recall eval (ADR-0072 d11, phase 1). Standard library only.

    python3 eval/recall/federated.py --url http://127.0.0.1:8437 --state DIR \
        --out eval/recall/results [--scifact-zip PATH]

federated.sh boots the lead, makes the workareas and the keys, and runs this.
It reads the keys from DIR (writer.key, reviewer.key, reader.key) and calls
`oml` for the admin's acts with the credentials federated.sh stored.

1. load: the 200-memory corpus, one committed workarea `fed-<topic>` per topic
   (11) under `eval-fed`, written by an agent key and approved by a person's.
2. splits, every query through POST /v1/memories/recall with the reader key,
   k 10, bm25-postings:
   - scoped: each answerable query scoped at its first grade-2 memory's topic;
   - cross: the same query scoped at the first topic, in name order after that
     one, holding none of its grade-2 memories;
   - unanswerable: each scoped at a topic in turn, counted by false hits.
   Modes: scope; wide at the default max_sources 8 (10 candidates, so the
   estimate chooses); the union (max_sources 64).
3. skew: the topic `database` padded with SciFact abstracts to at least 100
   times the largest other topic, max_sources 4, the cross split again under
   scope, wide and the union; E(s)'s choice compared with the exact ReDDE
   analogue, the union's top 100 counted per source.

Writes <out>/federated.json and prints the tables.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
import time
import zipfile
from collections import Counter, OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402
from run import Client  # noqa: E402

CLAIM_PREFIX = "fed-eval-"
PARENT = "eval-fed"
SEARCH = "bm25-postings"
K = 10
SKEW_TOPIC = "database"
SKEW_FACTOR = 100
SKEW_MAX_SOURCES = 4


def oml(*args):
    """The admin's acts, through the CLI and the credentials federated.sh stored."""
    out = subprocess.run([os.environ["OML"], "--json", *args], check=True,
                         capture_output=True, text=True)
    return json.loads(out.stdout) if out.stdout.strip() else None


def workarea(topic):
    return f"fed-{topic}"


def write_all(writer, reviewer, rows):
    """rows: [(claim_key, text, workarea)] -> {artifact_id: claim_key}, approved."""
    idmap = {}
    for key, text, wa in rows:
        body = {"body": text, "claim_key": CLAIM_PREFIX + key, "kind": "memory", "workarea": wa}
        status, resp = writer.call("POST", "/v1/memories", body, idempotent=True)
        if status != 202:
            raise SystemExit(f"write {key} refused: {status} {resp}")
        idmap[resp["artifact_id"]] = key
    approved = 0
    while True:
        status, item = reviewer.call("POST", "/v2/review/next", {}, idempotent=True)
        if status == 204:
            break
        if status != 200:
            raise SystemExit(f"review next: {status} {item}")
        status, v = reviewer.call("POST", "/v2/review/verdicts",
                                  {"serve_id": item["serve_id"], "verdict": "approve"},
                                  idempotent=True)
        if status != 200 or v.get("state") != "promoted":
            raise SystemExit(f"approve: {status} {v}")
        approved += 1
    print(f"loaded {len(idmap)}, approved {approved}", file=sys.stderr)
    return idmap


def recall(reader, query, scope, sources):
    body = {"query": query, "k": K, "workarea": scope, "search": SEARCH, "include_text": False}
    if sources is not None:
        body["sources"] = sources
    t0 = time.perf_counter()
    status, resp = reader.call("POST", "/v1/memories/recall", body)
    ms = (time.perf_counter() - t0) * 1000
    if status != 200:
        raise SystemExit(f"recall {query!r} at {scope} under {sources}: {status} {resp}")
    return resp, ms


def p95(values):
    v = sorted(values)
    return round(v[max(0, -(-len(v) * 95 // 100) - 1)], 1) if v else None


def run_split(reader, idmap, topic_of, items, sources):
    """items: [(query, scope topic)] -> per-query rows."""
    rows = []
    for q, scope in items:
        resp, ms = recall(reader, q["query"], workarea(scope), sources)
        ranked = [idmap.get(h["memory"]["artifact_id"], "?") for h in resp["hits"]]
        searched = [s["workarea_path"].rsplit(".", 1)[-1]
                    for s in (resp["search"].get("sources") or {}).get("searched", [])]
        rows.append({
            "id": q["id"], "category": q["category"], "scope": scope, "ranked": ranked,
            "latency_ms": round(ms, 2), "searched": searched,
            "cut": (resp["search"].get("sources") or {}).get("cut"),
        })
    return rows


def summarize(rows, queries_by_id, topic_of):
    s5, nd, lat, sel_p, sel_r, false_hits, n_un = [], [], [], [], [], 0, 0
    for r in rows:
        q = queries_by_id[r["id"]]
        lat.append(r["latency_ms"])
        if q["category"] == metrics.UNANSWERABLE:
            n_un += 1
            false_hits += 1 if r["ranked"] else 0
            continue
        rel = q["relevant"]
        s5.append(metrics.success_at(r["ranked"], rel, 5))
        nd.append(metrics.ndcg_at(r["ranked"], rel, 10))
        if r["searched"]:
            holding = {workarea(topic_of[m]) for m, g in rel.items() if g == 2}
            searched = set(r["searched"])
            sel_p.append(len(searched & holding) / len(searched))
            sel_r.append(len(searched & holding) / len(holding))
    mean = lambda v: round(sum(v) / len(v), 3) if v else None  # noqa: E731
    # success@5 per category, as counts, so d11's one-query rule reads off it.
    by_category = OrderedDict()
    for r in rows:
        q = queries_by_id[r["id"]]
        if q["category"] == metrics.UNANSWERABLE:
            continue
        hit = int(metrics.success_at(r["ranked"], q["relevant"], 5))
        c = by_category.setdefault(q["category"], [0, 0])
        c[0] += hit
        c[1] += 1
    return OrderedDict([
        ("success@5_by_category", {c: f"{h}/{n}" for c, (h, n) in by_category.items()}),
        ("n", len(s5)), ("success@5", mean(s5)), ("ndcg@10", mean(nd)),
        ("selection_precision", mean(sel_p)), ("selection_recall", mean(sel_r)),
        ("unanswerable_false_hits", round(false_hits / n_un, 3) if n_un else None),
        ("p50_ms", round(statistics.median(lat), 1) if lat else None), ("p95_ms", p95(lat)),
    ])


def cross_scope(q, topic_of, topics):
    holding = {topic_of[m] for m, g in q["relevant"].items() if g == 2}
    first = sorted(holding)[0]
    i = topics.index(first)
    for t in topics[i + 1:] + topics[:i]:
        if t not in holding:
            return t
    return None


def scifact(path, n):
    with zipfile.ZipFile(path) as z:
        name = next(x for x in z.namelist() if x.endswith("corpus.jsonl"))
        with z.open(name) as f:
            for i, line in enumerate(f):
                if i >= n:
                    return
                doc = json.loads(line)
                text = (doc.get("title", "") + ". " + doc.get("text", "")).strip()
                yield f"scifact-{doc['_id']}", text[:4000]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", required=True)
    ap.add_argument("--state", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--scifact-zip", default=None)
    a = ap.parse_args()
    state = Path(a.state)
    key = lambda n: (state / n).read_text().strip()  # noqa: E731
    writer, reviewer, reader = (Client(a.url, key(f"{n}.key")) for n in ("writer", "reviewer", "reader"))
    memories, queries = metrics.load()
    queries_by_id = {q["id"]: q for q in queries}
    topic_of = {m["id"]: m["topic"] for m in memories}
    topics = sorted({m["topic"] for m in memories})

    idmap = write_all(writer, reviewer,
                      [(m["id"], m["text"], workarea(m["topic"])) for m in memories])

    answerable = [q for q in queries if q["category"] != metrics.UNANSWERABLE]
    unanswerable = [q for q in queries if q["category"] == metrics.UNANSWERABLE]
    scoped = [(q, topic_of[sorted(m for m, g in q["relevant"].items() if g == 2)[0]])
              for q in answerable]
    cross = [(q, t) for q in answerable if (t := cross_scope(q, topic_of, topics))]
    unans = [(q, topics[i % len(topics)]) for i, q in enumerate(unanswerable)]

    recall(reader, "warm up", workarea(topics[0]), None)
    report = OrderedDict()
    raw = {}
    modes = [("scope", "scope", None), ("wide@8", "wide", 8), ("union", "wide", 64)]
    for split, items in (("scoped", scoped), ("cross", cross), ("unanswerable", unans)):
        report[split] = OrderedDict()
        for label, sources, max_sources in modes:
            if max_sources is not None:
                oml("search", "federation", "--max-sources", str(max_sources))
            rows = run_split(reader, idmap, topic_of, items, sources)
            raw[f"{split}/{label}"] = rows
            report[split][label] = summarize(rows, queries_by_id, topic_of)
    oml("search", "federation", "--max-sources", "8")

    # The skew split (d11): one topic padded to 100 times the largest other.
    if a.scifact_zip:
        sizes = Counter(m["topic"] for m in memories)
        pad = SKEW_FACTOR * max(c for t, c in sizes.items() if t != SKEW_TOPIC)
        idmap.update(write_all(writer, reviewer,
                               [(k, t, workarea(SKEW_TOPIC)) for k, t in scifact(a.scifact_zip, pad)]))
        report["skew"] = OrderedDict([("padded_topic", SKEW_TOPIC), ("padded_rows", pad),
                                      ("max_sources", SKEW_MAX_SOURCES)])
        for label, sources, max_sources in (("scope", "scope", None),
                                            (f"wide@{SKEW_MAX_SOURCES}", "wide", SKEW_MAX_SOURCES),
                                            ("union", "wide", 64)):
            if max_sources is not None:
                oml("search", "federation", "--max-sources", str(max_sources))
            rows = run_split(reader, idmap, topic_of, cross, sources)
            raw[f"skew/{label}"] = rows
            report["skew"][label] = summarize(rows, queries_by_id, topic_of)
        # E(s)'s choice against the exact ReDDE analogue: the sources of the
        # union's top 100, counted, the first max_sources of them.
        oml("search", "federation", "--max-sources", "64")
        agree = []
        for q, scope in cross:
            body = {"query": q["query"], "k": 100, "workarea": workarea(scope), "search": SEARCH,
                    "include_text": False, "sources": "wide"}
            status, resp = reader.call("POST", "/v1/memories/recall", body)
            counts = Counter(h["memory"]["workarea_path"].rsplit(".", 1)[-1]
                             for h in resp["hits"] if h.get("via") == "federated")
            redde = [w for w, _ in sorted(counts.items(), key=lambda x: (-x[1], x[0]))][:SKEW_MAX_SOURCES]
            chosen = [s for s in next(r for r in raw[f"skew/wide@{SKEW_MAX_SOURCES}"]
                                      if r["id"] == q["id"])["searched"] if s != workarea(scope)]
            if redde:
                agree.append(len(set(chosen) & set(redde)) / len(redde))
        report["skew"]["agreement_with_redde_analogue"] = round(sum(agree) / len(agree), 3)
        oml("search", "federation", "--max-sources", "8")

    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "federated.json").write_text(json.dumps(
        {"method": "federated", "pipeline": SEARCH, "k": K, "report": report, "per_query": raw},
        indent=1))
    print(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
