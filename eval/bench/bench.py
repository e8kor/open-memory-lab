#!/usr/bin/env python3
"""The recall benchmark harness: established benchmarks through OML's served recall.

    bench.py workareas <dataset>...          the workareas boot.sh must create
    bench.py load --state S <dataset>         write the corpus, approve it
    bench.py run  --state S <dataset> [--search P]...
    bench.py report <results-dir>             REPORT tables from results/*.json

`--state` is boot.sh's state directory: it holds the server's URL, the writer
key and the reviewer keys. Keys are read from files and never printed.

load: every memory is written with the writer (agent) key through POST
/v1/memories, then judged by a crew of reviewer keys of other principals
through POST /v2/review/next and /v2/review/verdicts, one row per call, as a
reviewer would, so the reviewer is never the author (R2). The routes take one
row each, so the benchmark's only shortcut is concurrency: --workers writers
and one thread per reviewer key, each request the same request an agent or a
reviewer makes. Nothing is written to a table directly. The claim_key ->
artifact_id map goes to <state>/idmap-<dataset>.json; a second load writes
only what is missing (it lists the writer's own rows first).

run: every query goes through oml_read in recall mode over POST /mcp, the tool
the oml plugin calls, scoped to its group's workarea, with `search` naming the
pipeline, k 100 and no token budget, so nDCG@10 and Recall@100 come from one
ranked list. Metrics are ir_measures' (pytrec_eval underneath): nDCG@10,
R@10, R@100, RR@10, Success@1, Success@5; a query whose recall served nothing
scores 0 on each, rather than dropping out of the mean as pytrec_eval would
drop it. Latency is the client-side wall time of the /mcp call.
"""

import argparse
import concurrent.futures as cf
import json
import statistics
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "recall"))

from adapters import ADAPTERS, PUBLISHED_BM25, safe, workarea  # noqa: E402
from run import Client  # noqa: E402  eval/recall/run.py's HTTP client

PIPELINES = ["keyword-all", "keyword-any", "bm25", "bm25-trigram", "bm25-postings", "bm25-rerank",
             "sparse", "bm25-sparse", "bm25-sparse-rrf"]
K = 100
MEASURES = ["nDCG@10", "R@10", "R@20", "R@100", "RR@10", "Success@1", "Success@5"]
DEFAULT_CACHE = "/srv/build/bench-data"


def claim_key(dataset, doc_id):
    return f"bench-{dataset}-{safe(doc_id)}"


def adapter(name):
    if name not in ADAPTERS:
        raise SystemExit(f"unknown dataset {name!r}; known: {', '.join(ADAPTERS)}")
    return ADAPTERS[name]()


def clients(state):
    state = Path(state)
    url = (state / "url").read_text().strip()
    writer = Client(url, (state / "writer.key").read_text().strip())
    reviewers = [Client(url, p.read_text().strip())
                 for p in sorted(state.glob("reviewer-*.key"))]
    if not reviewers:
        raise SystemExit(f"no reviewer-*.key in {state}")
    return writer, reviewers


RETRIES = {"count": 0}


def retrying(client, method, path, body):
    """One request, sent again (with a fresh Idempotency-Key) while the server
    says it is retryable: under concurrent writes a verdict can meet "chain
    head contention", a 503 marked retryable, which a client is told to retry.
    The retries are counted and logged with the load."""
    for attempt in range(10):
        status, resp = client.call(method, path, body, idempotent=True)
        retryable = status == 503 or (isinstance(resp, dict)
                                      and resp.get("error", {}).get("retryable"))
        if not (status >= 500 or status == 429) or not retryable:
            return status, resp
        RETRIES["count"] += 1
        time.sleep(0.05 * 2 ** min(attempt, 5))
    return status, resp


def own_rows(writer, prefix):
    """claim_key -> artifact_id for every row of this dataset the writer wrote."""
    found, cursor = {}, None
    while True:
        path = "/v1/memories?own=true&limit=200" + (f"&cursor={cursor}" if cursor else "")
        status, resp = writer.call("GET", path)
        if status != 200:
            raise SystemExit(f"own listing: {status} {resp}")
        for row in resp["items"]:
            if row["claim_key"].startswith(prefix):
                found[row["claim_key"]] = row["artifact_id"]
        cursor = resp.get("next_cursor")
        if not cursor:
            return found


def cmd_workareas(a):
    names = []
    for name in a.datasets:
        ad = adapter(name)
        if ad.groups is None:
            ad.load(a.cache, a.seed, None)
        names += [workarea(name, g) for g in ad.groups]
    print(" ".join(names))


def cmd_load(a):
    ad = adapter(a.dataset)
    bench = ad.load(a.cache, a.seed, None)
    writer, reviewers = clients(a.state)
    prefix = f"bench-{a.dataset}-"
    have = own_rows(writer, prefix)
    todo = [d for d in bench.docs if claim_key(a.dataset, d.id) not in have]
    refused = {}
    lock = threading.Lock()
    t0 = time.perf_counter()

    def write(d):
        body = {"body": d.text, "claim_key": claim_key(a.dataset, d.id), "kind": "memory",
                "workarea": workarea(a.dataset, d.group)}
        status, resp = retrying(writer, "POST", "/v1/memories", body)
        if status == 202:
            with lock:
                have[body["claim_key"]] = resp["artifact_id"]
            return
        with lock:
            refused[d.id] = {"status": status, "response": resp}

    with cf.ThreadPoolExecutor(a.workers) as pool:
        for i, _ in enumerate(pool.map(write, todo), 1):
            if i % 1000 == 0:
                print(f"  wrote {i}/{len(todo)}", file=sys.stderr)
    t_write = time.perf_counter() - t0

    approved = [0]

    def review(reviewer):
        while True:
            status, item = retrying(reviewer, "POST", "/v2/review/next", {})
            if status == 204:
                return
            if status != 200:
                raise SystemExit(f"review next: {status} {item}")
            status, v = retrying(reviewer, "POST", "/v2/review/verdicts",
                                 {"serve_id": item["serve_id"], "verdict": "approve"})
            if status != 200 or v.get("state") != "promoted":
                raise SystemExit(f"approve: {status} {v}")
            with lock:
                approved[0] += 1
                if approved[0] % 1000 == 0:
                    print(f"  approved {approved[0]}", file=sys.stderr)

    t1 = time.perf_counter()
    with cf.ThreadPoolExecutor(len(reviewers)) as pool:
        list(pool.map(review, reviewers))
    t_review = time.perf_counter() - t1

    state = Path(a.state)
    (state / f"idmap-{a.dataset}.json").write_text(json.dumps(have))
    log = {
        "dataset": a.dataset, "docs": len(bench.docs), "written": len(todo) - len(refused),
        "refused": len(refused), "refusals": dict(list(refused.items())[:20]),
        "approved": approved[0], "write_seconds": round(t_write, 1),
        "review_seconds": round(t_review, 1), "writers": a.workers, "reviewers": len(reviewers),
        "retried_503": RETRIES["count"],
    }
    (state / f"load-{a.dataset}.json").write_text(json.dumps(log, indent=1))
    print(json.dumps({k: v for k, v in log.items() if k != "refusals"}))


def score(bench, runs):
    """ir_measures over the run; a query with no hits scores 0 on every measure."""
    import ir_measures
    from ir_measures import Qrel, ScoredDoc

    rel = bench.success_rel
    measures = {
        "nDCG@10": ir_measures.nDCG @ 10,
        "R@10": ir_measures.R @ 10,
        "R@20": ir_measures.R @ 20,
        "R@100": ir_measures.R @ 100,
        "RR@10": ir_measures.RR @ 10,
        "Success@1": ir_measures.Success(rel=rel) @ 1,
        "Success@5": ir_measures.Success(rel=rel) @ 5,
    }
    qids = [q.id for q in bench.queries]
    qrels = [Qrel(q, d, g) for q in qids for d, g in bench.qrels[q].items()]
    # Scores as ranks: the served order is the ranking, whatever the
    # pipeline's score scale, and pytrec_eval sorts by score.
    run = [ScoredDoc(q, d, float(K - i)) for q in qids for i, d in enumerate(runs[q]["ranked"])
           if d is not None]
    per = {q: {m: 0.0 for m in measures} for q in qids}
    by_obj = {v: k for k, v in measures.items()}
    for r in ir_measures.iter_calc(list(measures.values()), qrels, run):
        per[r.query_id][by_obj[r.measure]] = r.value
    return per


def summarize(bench, runs, per):
    cats = {}
    for q in bench.queries:
        cats.setdefault(q.category, []).append(q.id)
    groups = {"all": [q.id for q in bench.queries]}
    if len(cats) > 1:
        groups.update(sorted(cats.items()))
    if bench.name == "locomo":
        groups["headline"] = [q.id for q in bench.queries if q.category != "adversarial"]
    out = {}
    for g, ids in groups.items():
        lat = sorted(runs[i]["latency_ms"] for i in ids)
        row = {m: round(statistics.fmean(per[i][m] for i in ids), 4) for m in MEASURES}
        row["zero"] = round(sum(1 for i in ids if not runs[i]["ranked"]) / len(ids), 4)
        row["p50_ms"] = round(statistics.median(lat), 1)
        row["p95_ms"] = round(lat[min(len(lat) - 1, int(0.95 * len(lat)))], 1)
        row["n"] = len(ids)
        out[g] = row
    return out


def cmd_run(a):
    ad = adapter(a.dataset)
    bench = ad.load(a.cache, a.seed, a.limit)
    writer, _ = clients(a.state)
    idmap = json.loads((Path(a.state) / f"idmap-{a.dataset}.json").read_text())
    back = {aid: ck for ck, aid in idmap.items()}
    doc_of = {claim_key(a.dataset, d.id): d.id for d in bench.docs}
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    for pipeline in a.search or PIPELINES:
        writer.recall("warm up", a.k, None, workarea(a.dataset, bench.queries[0].group), pipeline)
        runs, served_by = {}, set()

        def ask(q, pipeline=pipeline):
            res, ms = writer.recall(q.text, a.k, None, workarea(a.dataset, q.group), pipeline)
            hits = res.get("hits", [])
            ranked = [doc_of.get(back.get((h.get("memory", h))["artifact_id"])) for h in hits]
            search = res.get("search", {})
            return q.id, search.get("pipeline"), {
                "ranked": ranked, "latency_ms": round(ms, 2),
                "fallback": search.get("fallback"),
                # The reranking stage (ADR-0053): its outcome, each hit's
                # relevance, and what a floor withheld, so a floor can be
                # swept offline over a run made with none.
                "rerank": search.get("rerank"),
                "rerank_reason": search.get("rerank_reason"),
                "rerank_scores": [(h.get("matched") or {}).get("rerank") for h in hits],
                "below_floor": search.get("below_floor"),
            }

        t0 = time.perf_counter()
        with cf.ThreadPoolExecutor(a.workers) as pool:
            for qid, served, row in pool.map(ask, bench.queries):
                runs[qid] = row
                served_by.add(served)
        wall = time.perf_counter() - t0
        if served_by != {pipeline}:
            raise SystemExit(f"asked for {pipeline}, served {sorted(map(str, served_by))}")
        foreign = sum(1 for r in runs.values() for d in r["ranked"] if d is None)
        per = score(bench, runs)
        summary = summarize(bench, runs, per)
        result = {
            "dataset": a.dataset, "pipeline": pipeline, "k": a.k, "max_tokens": None,
            "queries": len(bench.queries), "docs": len(bench.docs), "seed": a.seed,
            "limit": a.limit, "workers": a.workers, "wall_seconds": round(wall, 1),
            "rows_in_scope": a.rows,
            "foreign_hits": foreign, "success_rel": bench.success_rel, "notes": bench.notes,
            "summary": summary,
            "per_query": {q: {**runs[q], "scores": per[q]} for q in runs},
        }
        name = f"{a.dataset}_{pipeline.replace('-', '_')}" + (f"_k{a.k}" if a.k != K else "")
        (out / f"{name}.json").write_text(json.dumps(result, indent=1))
        head = summary.get("headline", summary["all"])
        print(f"{a.dataset} {pipeline}: " + " ".join(f"{m} {head[m]}" for m in
              MEASURES + ["zero", "p50_ms", "p95_ms"]) + f" n {head['n']} ({wall:.0f}s)")


REFERENCES = ["ref_bm25_anserini", "ref_bm25_oml_params"]  # reference.py's settings


def cmd_report(a):
    results = {}
    for p in sorted(Path(a.results).glob("*.json")):
        r = json.loads(p.read_text())
        if "summary" in r and "dataset" in r:
            results.setdefault(r["dataset"], {})[r["pipeline"]] = r
    lines = []
    for ds, by in results.items():
        any_r = next(by[p] for p in PIPELINES + REFERENCES if p in by)
        key = "headline" if "headline" in any_r["summary"] else "all"
        lines.append(f"### {ds} ({any_r['docs']} memories, {any_r['summary'][key]['n']} queries"
                     + (", headline: categories 1-4" if key == "headline" else "") + ")\n")
        lines.append("| pipeline | " + " | ".join(MEASURES) +
                     " | zero-result | p50 ms | p95 ms |")
        lines.append("|---|" + "---:|" * (len(MEASURES) + 3))
        for p in PIPELINES + REFERENCES:
            if p not in by:
                continue
            s = by[p]["summary"][key]
            lat = (f" | {s['p50_ms']:.1f} | {s['p95_ms']:.1f} |" if s["p50_ms"] is not None
                   else " | offline | offline |")
            label = p if p in PIPELINES else f"{p} (offline bm25s)"
            lines.append(f"| {label} | " + " | ".join(f"{s[m]:.3f}" for m in MEASURES) +
                         f" | {s['zero']:.3f}" + lat)
        if ds in PUBLISHED_BM25:
            pub = PUBLISHED_BM25[ds]
            lines.append("| BEIR BM25 (published) | " + " | ".join(
                f"{pub[m]:.3f}" if m in pub else "" for m in MEASURES) + " | | | |")
        cats = [c for c in any_r["summary"] if c not in ("all", "headline")]
        if cats:
            lines.append(f"\n{ds} nDCG@10 / R@10 by category:\n")
            lines.append("| category | n | " + " | ".join(p for p in PIPELINES if p in by) + " |")
            lines.append("|---|---:|" + "---:|" * sum(1 for p in PIPELINES if p in by))
            for c in cats + (["all"] if key == "headline" else []):
                n = any_r["summary"][c]["n"]
                cells = [f"{by[p]['summary'][c]['nDCG@10']:.2f} / {by[p]['summary'][c]['R@10']:.2f}"
                         for p in PIPELINES if p in by]
                lines.append(f"| {c} | {n} | " + " | ".join(cells) + " |")
        lines.append("")
    print("\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--cache", default=DEFAULT_CACHE, help="where datasets are cached")
    ap.add_argument("--seed", type=int, default=20261104)
    sub = ap.add_subparsers(dest="cmd", required=True)
    w = sub.add_parser("workareas")
    w.add_argument("datasets", nargs="+")
    ld = sub.add_parser("load")
    ld.add_argument("--state", required=True)
    ld.add_argument("--workers", type=int, default=8)
    ld.add_argument("dataset")
    r = sub.add_parser("run")
    r.add_argument("--state", required=True)
    r.add_argument("--out", required=True, help="raw results; export.py writes the committed ones")
    r.add_argument("--search", action="append", choices=PIPELINES)
    r.add_argument("--limit", type=int, default=None, help="at most this many queries, seeded")
    r.add_argument("--k", type=int, default=K,
                   help="hits asked per recall; below 100 the run is for latency at that k "
                        "and R@100 reads the shorter list")
    r.add_argument("--workers", type=int, default=1,
                   help="concurrent queries; latency is measured at 1")
    r.add_argument("--rows", type=int, default=None,
                   help="the promoted rows in the scope, when not every loaded row was approved")
    r.add_argument("dataset")
    rp = sub.add_parser("report")
    rp.add_argument("results")
    a = ap.parse_args()
    {"workareas": cmd_workareas, "load": cmd_load, "run": cmd_run, "report": cmd_report}[a.cmd](a)


if __name__ == "__main__":
    main()
