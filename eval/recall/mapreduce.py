#!/usr/bin/env python3
"""Map-reduce recall, offline: can each workarea rank its own rows and a reducer merge the lists
into the answer one search over all of them gives? Standard library only. A research prototype,
not a served path: nothing here runs in the server.

    python3 eval/recall/mapreduce.py encode --encoder BIN --model DIR --scifact-zip PATH --cache DIR
    python3 eval/recall/mapreduce.py run --scifact-zip PATH --cache DIR --out eval/recall/results

`encode` writes the learned sparse postings of every text and the query terms of every query
through `oml-rerank`'s `sparse_encode` example (doc-v3-distill at the pinned revision), the
encoder the server runs, into the cache directory. `run` reads them and needs no model.

What `run` computes, for three pipelines, the shapes `oml-app` serves (search.rs):
- `sparse`: sum over query tokens of the model's idf.json weight times the document weight;
- `bm25`: BM25, k1 1.2 and b 0.75 as `oml-pg` writes it, over an offline analyzer (lower-cased
  words, identifier parts and a light suffix stemmer; no Snowball in the standard library), so its
  absolute numbers are not the served `bm25-postings`', but every variant uses the same analyzer;
- `bm25-sparse`: the two lists read 100 deep (CC_DEPTH), each min-max normalised over its own
  candidates and summed 0.3 / 0.7 (CC_BM25_SPARSE).

Over four partitions of the rows into workareas (shards):
- `topics`: the 200-memory recall corpus, one workarea per topic (11), 121 answerable queries;
- `skew`: the same plus 2,400 SciFact abstracts in `database`, as fed2.py pads it (the shape of
  `universe` beside crews);
- `scifact-N`: BEIR SciFact, 5,183 abstracts and its 300 test queries, split into N uniform random
  workareas, N in 5, 50, 500, and `scifact-zipf-50`, 50 workareas whose sizes follow 1/rank.

And six ways to answer, each compared with `global`, one search over the union:
- `exact`: each shard scores every leg with the union's statistics (BM25's n, avgdl, df: sums of
  per-shard counts, ADR-0073 d9 round 1), returns each leg's top 100 with raw scores; the reducer
  merges each leg by score (ties by id), cuts it at 100, fuses, cuts at k;
- `exact-adaptive`: as `exact`, but each shard first returns d0 = max(10, ceil(2 * 100 / N)) per
  leg; the reducer takes the 100th best score it holds as a threshold and asks again, to 100, only
  the shards whose d0-th score is at or above it (Fagin's threshold idea); still exact;
- `local-stats`: as `exact`, but BM25 on each shard uses that shard's own n, avgdl and df;
- `local-fused-score`: each shard runs the whole pipeline on its own rows and statistics, its own
  CC bounds included, and returns its top k fused scores; the reducer sorts them by score;
- `local-fused-rrf`: the same shard lists merged by reciprocal rank, k 60;
- `local-fused-cori`: the same shard lists merged by CORI's rule (Callan et al. 1995):
  (s + 0.4 * s * C') / 1.4, C' the shard's min-max normalised CORI belief for the query words.

Writes <out>/mapreduce.json and prints the tables.
"""

import argparse
import json
import math
import random
import re
import statistics
import subprocess
import sys
import zipfile
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402

K = 10
DEPTH = 100                 # CC_DEPTH
CC = (0.3, 0.7)             # CC_BM25_SPARSE: bm25, then sparse
RRF_K = 60                  # RRF_K
K1, B = 1.2, 0.75
SEED = 20261111
SKEW_TOPIC, SKEW_ROWS = "database", 2400   # fed2.py's padding
PIPELINES = ["sparse", "bm25", "bm25-sparse"]
VARIANTS = ["exact", "exact-adaptive", "local-stats", "local-fused-score", "local-fused-rrf",
            "local-fused-cori"]


# ---------------------------------------------------------------- data

def scifact(path):
    with zipfile.ZipFile(path) as z:
        docs = []
        with z.open("scifact/corpus.jsonl") as f:
            for line in f:
                d = json.loads(line)
                docs.append((f"scifact-{d['_id']}", (d.get("title", "") + ". " + d.get("text", "")).strip()[:4000]))
        qtext = {}
        with z.open("scifact/queries.jsonl") as f:
            for line in f:
                q = json.loads(line)
                qtext[q["_id"]] = q["text"]
        rel = defaultdict(dict)
        with z.open("scifact/qrels/test.tsv") as f:
            for i, line in enumerate(f.read().decode().splitlines()):
                if i == 0:
                    continue
                qid, did, g = line.split("\t")
                if int(g) > 0:
                    rel[qid][f"scifact-{did}"] = int(g)
    queries = [{"id": f"sf-{q}", "query": qtext[q], "relevant": r} for q, r in sorted(rel.items())]
    return docs, queries


# ---------------------------------------------------------------- encode

def encode(a):
    cache = Path(a.cache)
    cache.mkdir(parents=True, exist_ok=True)
    memories, queries = metrics.load()
    docs, sq = scifact(a.scifact_zip)
    texts = {m["id"]: m["text"] for m in memories}
    texts.update(dict(docs))
    qs = {q["id"]: q["query"] for q in queries if q["category"] != metrics.UNANSWERABLE}
    qs.update({q["id"]: q["query"] for q in sq})
    for mode, items in (("queries", qs), ("docs", texts)):
        path = cache / f"{mode}.jsonl"
        done = set()
        if path.exists():
            with open(path) as f:
                done = {json.loads(line)["id"] for line in f}
        ids = sorted(i for i in items if i not in done)
        for c in range(0, len(ids), 250):
            chunk = ids[c:c + 250]
            inp = "".join(json.dumps({"text": items[i]}) + "\n" for i in chunk)
            out = subprocess.run([a.encoder, a.model, str(a.threads), mode], input=inp, check=True,
                                 capture_output=True, text=True)
            rows = out.stdout.splitlines()
            assert len(rows) == len(chunk), (mode, len(rows), len(chunk))
            with open(path, "a") as f:
                for i, r in zip(chunk, rows):
                    f.write(json.dumps({"id": i, "terms": json.loads(r)}) + "\n")
            print(mode, len(done) + c + len(chunk), "/", len(items), out.stderr.strip(), file=sys.stderr, flush=True)


# ---------------------------------------------------------------- analyzer and scorers

def stem(w):
    """A light English suffix stemmer: the same for every variant, so comparisons hold."""
    for suf, rep in (("ies", "y"), ("sses", "ss"), ("ing", ""), ("edly", ""), ("ed", ""),
                     ("ly", ""), ("es", ""), ("s", "")):
        if w.endswith(suf) and len(w) - len(suf) >= 3 and not w.endswith("ss"):
            return w[: len(w) - len(suf)] + rep
    return w


def forms(text):
    """{word: set of forms}: the word, its identifier parts and its stem (`bm25`'s forms)."""
    out = {}
    for raw in re.findall(r"[A-Za-z0-9_.\-]+", text):
        raw = raw.strip(".-_")
        if not raw:
            continue
        w = raw.lower()
        fs = {w, stem(w)}
        for part in re.split(r"[_.\-]|(?<=[a-z])(?=[A-Z])", raw):
            if part and part.lower() != w:
                fs.add(part.lower())
                fs.add(stem(part.lower()))
        out.setdefault(w, set()).update(fs)
    return out


class Bm25Index:
    def __init__(self, texts):
        self.tf = {}      # doc -> Counter(form)
        self.len = {}
        self.post = defaultdict(list)  # form -> docs
        for d, t in texts.items():
            fsets = forms(t)
            c = Counter()
            for fs in fsets.values():
                c.update(fs)
            self.tf[d] = c
            self.len[d] = max(1, sum(len(fs) for fs in fsets.values()))
            for f in c:
                self.post[f].append(d)

    def score(self, qwords, shard_of, stats):
        """BM25 of every row in `shard_of`, {doc: score}. `stats` maps a shard to the key whose
        statistics score its rows: one key for all (the union's) or each shard its own."""
        key_of = {s: stats(s) for s in set(shard_of.values())}
        n, total = Counter(), Counter()
        for d, s in shard_of.items():
            n[key_of[s]] += 1
            total[key_of[s]] += self.len[d]
        acc = defaultdict(float)
        for _w, fs in qwords.items():
            best = defaultdict(float)
            for f in fs:
                docs = [d for d in self.post.get(f, ()) if d in shard_of]
                if not docs:
                    continue
                df = Counter(key_of[shard_of[d]] for d in docs)
                for d in docs:
                    key = key_of[shard_of[d]]
                    idf = math.log(1 + (n[key] - df[key] + 0.5) / (df[key] + 0.5))
                    avgdl = max(1.0, total[key] / n[key])
                    tf = self.tf[d][f]
                    c = idf * tf * 2.2 / (tf + K1 * (0.25 + 0.75 * self.len[d] / avgdl))
                    if c > best[d]:
                        best[d] = c
            for d, c in best.items():
                acc[d] += c
        return acc


class SparseIndex:
    def __init__(self, enc_docs):
        self.post = defaultdict(list)  # token -> [(doc, weight)]
        for d, terms in enc_docs.items():
            for tok, w, _in_text in terms:
                self.post[tok].append((d, w))

    def score(self, qterms, shard_of):
        acc = defaultdict(float)
        for tok, w, _piece in qterms:
            for d, dw in self.post.get(tok, ()):
                if d in shard_of:
                    acc[d] += w * dw
        return acc


def top(scores, n):
    return sorted(scores.items(), key=lambda x: (-x[1], x[0]))[:n]


def per_shard(scores, shard_of, n):
    """Each shard's own top n of a score dict: what a shard returns when a row's score is a
    function of the row, the query and statistics fixed before the search."""
    out = defaultdict(list)
    for d, s in sorted(scores.items(), key=lambda x: (-x[1], x[0])):
        lst = out[shard_of[d]]
        if len(lst) < n:
            lst.append((d, s))
    return out


def cc(lists):
    """CC over (list, weight) pairs, each list [(doc, score)] normalised over its own members."""
    out = defaultdict(float)
    for lst, wt in lists:
        if not lst:
            continue
        hi, lo = lst[0][1], lst[-1][1]
        for d, s in lst:
            out[d] += wt * ((s - lo) / (hi - lo) if hi > lo else 1.0)
    return out


def merge(lists, n):
    return sorted((x for lst in lists for x in lst), key=lambda x: (-x[1], x[0]))[:n]


# ---------------------------------------------------------------- one query over one partition

class World:
    def __init__(self, texts, enc_docs, enc_q):
        self.bm = Bm25Index(texts)
        self.sp = SparseIndex(enc_docs)
        self.enc_q = enc_q

    @staticmethod
    def answer(pipeline, legs, k):
        if pipeline == "bm25-sparse":
            return top(cc([(legs["bm25"][:DEPTH], CC[0]), (legs["sparse"][:DEPTH], CC[1])]), k)
        return legs[pipeline][:k]

    def cori(self, q, shard_of, shards):
        """CORI's belief per shard for the query's words (Callan, Lu and Croft, SIGIR 1995),
        min-max normalised over the shards."""
        words = forms(q["query"])
        cw = Counter()
        for d, s in shard_of.items():
            cw[s] += self.bm.len[d]
        avg_cw = statistics.mean(cw[s] for s in shards) or 1
        nshards = len(shards)
        bel = Counter()
        for _w, fs in words.items():
            df = Counter()
            for f in fs:
                c = Counter(shard_of[d] for d in self.bm.post.get(f, ()) if d in shard_of)
                for s, v in c.items():
                    df[s] = max(df[s], v)
            cf = sum(1 for s in shards if df[s] > 0)
            for s in shards:
                T = df[s] / (df[s] + 50 + 150 * cw[s] / avg_cw)
                I = math.log((nshards + 0.5) / max(cf, 1)) / math.log(nshards + 1)
                bel[s] += 0.4 + 0.6 * T * I
        vals = [bel[s] for s in shards]
        lo, hi = min(vals), max(vals)
        return {s: ((bel[s] - lo) / (hi - lo) if hi > lo else 1.0) for s in shards}

    def run_query(self, q, shard_of, shards):
        qwords = forms(q["query"])
        g_bm = self.bm.score(qwords, shard_of, lambda s: "union")
        l_bm = self.bm.score(qwords, shard_of, lambda s: s)
        sp = self.sp.score(self.enc_q[q["id"]], shard_of)
        g = {"bm25": top(g_bm, DEPTH), "sparse": top(sp, DEPTH)}
        exact = {"bm25": per_shard(g_bm, shard_of, DEPTH), "sparse": per_shard(sp, shard_of, DEPTH)}
        local = {"bm25": per_shard(l_bm, shard_of, DEPTH), "sparse": exact["sparse"]}
        nshards = len(shards)
        d0 = max(10, math.ceil(2 * DEPTH / nshards))
        cori = self.cori(q, shard_of, shards)
        out = {}
        for p in PIPELINES:
            res = {"global": self.answer(p, g, K)}
            legs_used = ["bm25", "sparse"] if p == "bm25-sparse" else [p]
            merged = {leg: merge([exact[leg][s] for s in shards], DEPTH) for leg in legs_used}
            res["exact"] = self.answer(p, merged, K)
            # adaptive: d0 per leg per shard first, then up to `need` only from the shards whose
            # d0-th score is at or above the `need`-th best score the reducer holds; a
            # single-leg pipeline needs k per leg, a CC pipeline DEPTH (its bounds)
            need = DEPTH if p == "bm25-sparse" else K
            shipped, second, got = 0, 0, {}
            for leg in legs_used:
                first = {s: exact[leg][s][:min(d0, need)] for s in shards}
                shipped += sum(len(v) for v in first.values())
                pool = merge(first.values(), need)
                tau = pool[-1][1] if len(pool) >= need else float("-inf")
                full = {}
                for s, lst in first.items():
                    if lst and len(lst) == min(d0, need) < need and lst[-1][1] >= tau:
                        full[s] = exact[leg][s][:need]
                        shipped += len(full[s]) - len(lst)
                        second += 1
                    else:
                        full[s] = lst
                got[leg] = merge(full.values(), need)
            res["exact-adaptive"] = self.answer(p, got, K)
            res["_adaptive"] = {"rows": shipped, "second_round_shards": second,
                                "full_rows": sum(len(exact[leg][s][:need]) for s in shards for leg in legs_used)}
            depth = 0
            for leg in legs_used:
                c = Counter(shard_of[d] for d, _ in g[leg][:need])
                depth = max(depth, max(c.values(), default=0))
            res["_depth_needed"] = depth
            local_merged = {leg: merge([local[leg][s] for s in shards], DEPTH) for leg in legs_used}
            res["local-stats"] = self.answer(p, local_merged, K)
            shard_answers = {s: self.answer(p, {leg: local[leg][s] for leg in legs_used}, K) for s in shards}
            res["local-fused-score"] = merge(shard_answers.values(), K)
            rrf = defaultdict(float)
            for lst in shard_answers.values():
                for r, (d, _s) in enumerate(lst):
                    rrf[d] += 1 / (RRF_K + r + 1)
            res["local-fused-rrf"] = top(rrf, K)
            cm = {d: (sc + 0.4 * sc * cori[s]) / 1.4 for s, lst in shard_answers.items() for d, sc in lst}
            res["local-fused-cori"] = top(cm, K)
            out[p] = res
        return out


# ---------------------------------------------------------------- partitions

def partitions(memories, queries, sf_docs, sf_queries):
    answerable = [q for q in queries if q["category"] != metrics.UNANSWERABLE]
    topics = sorted({m["topic"] for m in memories})
    by_topic = {t: {m["id"] for m in memories if m["topic"] == t} for t in topics}
    yield "topics", by_topic, answerable
    skew = {t: set(v) for t, v in by_topic.items()}
    skew[SKEW_TOPIC] |= {d for d, _ in sf_docs[:SKEW_ROWS]}
    yield "skew", skew, answerable
    ids = [d for d, _ in sf_docs]
    rng = random.Random(SEED)
    for n in (5, 50, 500):
        shuffled = ids[:]
        rng.shuffle(shuffled)
        yield f"scifact-{n}", {f"s{i:03d}": set(shuffled[i::n]) for i in range(n)}, sf_queries
    # 50 workareas sized 1/rank
    n = 50
    w = [1 / (r + 1) for r in range(n)]
    total = sum(w)
    sizes = [max(1, round(len(ids) * x / total)) for x in w]
    sizes[0] += len(ids) - sum(sizes)
    shuffled = ids[:]
    rng.shuffle(shuffled)
    shards, i = {}, 0
    for s, size in enumerate(sizes):
        shards[f"z{s:03d}"] = set(shuffled[i:i + size])
        i += size
    yield "scifact-zipf-50", shards, sf_queries


# ---------------------------------------------------------------- metrics

def quality(ranked, rel):
    ndcg = metrics.ndcg_at(ranked, rel, 10)
    top_grade = max(rel.values())
    s5 = 1.0 if any(rel.get(m) == top_grade for m in ranked[:5]) else 0.0
    return s5, ndcg


def summarise(name, shard_docs, qs, results):
    sizes = sorted(len(v) for v in shard_docs.values())
    out = {"workareas": len(shard_docs), "rows": sum(sizes), "largest": sizes[-1],
           "smallest": sizes[0], "queries": len(qs), "pipelines": {}}
    for p in PIPELINES:
        rows = {}
        for v in ["global"] + VARIANTS:
            s5, nd, same, ov, same_scores = [], [], 0, [], 0
            for q, r in zip(qs, results):
                lst = r[p][v]
                ids = [d for d, _ in lst]
                gids = [d for d, _ in r[p]["global"]]
                a, b = quality(ids, q["relevant"])
                s5.append(a)
                nd.append(b)
                same += ids == gids
                same_scores += lst == r[p]["global"]
                ov.append(len(set(ids) & set(gids)) / max(1, len(gids)))
            rows[v] = {"success@5": round(statistics.mean(s5), 4), "ndcg@10": round(statistics.mean(nd), 4),
                       "same_top10_as_global": round(same / len(qs), 4),
                       "same_top10_and_scores": round(same_scores / len(qs), 4),
                       "top10_overlap": round(statistics.mean(ov), 4)}
        depth = [r[p]["_depth_needed"] for r in results]
        ad = [r[p]["_adaptive"] for r in results]
        rows["_shipping"] = {
            "need_per_leg": DEPTH if p == "bm25-sparse" else K,
            "depth_needed_mean": round(statistics.mean(depth), 1),
            "depth_needed_p95": sorted(depth)[int(0.95 * (len(depth) - 1))],
            "depth_needed_max": max(depth),
            "rows_fixed_depth": round(statistics.mean(x["full_rows"] for x in ad), 1),
            "rows_adaptive": round(statistics.mean(x["rows"] for x in ad), 1),
            "queries_with_second_round": round(sum(1 for x in ad if x["second_round_shards"]) / len(ad), 4),
            "second_round_shards_mean": round(statistics.mean(x["second_round_shards"] for x in ad), 2),
        }
        out["pipelines"][p] = rows
    return out


def load_cache(cache):
    def rd(name):
        with open(Path(cache) / name) as f:
            return {r["id"]: r["terms"] for r in map(json.loads, f)}
    return rd("docs.jsonl"), rd("queries.jsonl")


def run(a):
    memories, queries = metrics.load()
    sf_docs, sf_queries = scifact(a.scifact_zip)
    enc_docs, enc_q = load_cache(a.cache)
    texts = {m["id"]: m["text"] for m in memories}
    texts.update(dict(sf_docs))
    world = World(texts, enc_docs, enc_q)
    report = {"k": K, "depth": DEPTH, "cc_weights": CC, "rrf_k": RRF_K, "seed": SEED, "partitions": {}}
    for name, shard_docs, qs in partitions(memories, queries, sf_docs, sf_queries):
        if a.only and name not in a.only:
            continue
        shard_of = {d: s for s, ds in shard_docs.items() for d in ds}
        shards = sorted(shard_docs)
        results = [world.run_query(q, shard_of, shards) for q in qs]
        report["partitions"][name] = summarise(name, shard_docs, qs, results)
        print(f"done {name}", file=sys.stderr)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "mapreduce.json").write_text(json.dumps(report, indent=1) + "\n")
    print_tables(report)


def print_tables(report):
    for name, part in report["partitions"].items():
        print(f"\n## {name}: {part['workareas']} workareas, {part['rows']} rows "
              f"(largest {part['largest']}, smallest {part['smallest']}), {part['queries']} queries\n")
        print("| pipeline | answer | success@5 | nDCG@10 | same top 10 as global | same top 10 and scores | top-10 overlap |")
        print("|---|---|---:|---:|---:|---:|---:|")
        for p, rows in part["pipelines"].items():
            for v in ["global"] + VARIANTS:
                r = rows[v]
                print(f"| {p} | {v} | {r['success@5']:.3f} | {r['ndcg@10']:.3f} | {r['same_top10_as_global']:.3f} "
                      f"| {r['same_top10_and_scores']:.3f} | {r['top10_overlap']:.3f} |")
        print("\n| pipeline | rows per leg needed | depth a shard needed mean / p95 / max | rows shipped fixed | rows shipped adaptive | queries with a second round |")
        print("|---|---:|---|---:|---:|---:|")
        for p, rows in part["pipelines"].items():
            s = rows["_shipping"]
            print(f"| {p} | {s['need_per_leg']} | {s['depth_needed_mean']} / {s['depth_needed_p95']} / {s['depth_needed_max']} "
                  f"| {s['rows_fixed_depth']} | {s['rows_adaptive']} | {s['queries_with_second_round']:.3f} |")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("encode")
    e.add_argument("--encoder", required=True)
    e.add_argument("--model", required=True)
    e.add_argument("--threads", type=int, default=4)
    e.add_argument("--scifact-zip", required=True)
    e.add_argument("--cache", required=True)
    r = sub.add_parser("run")
    r.add_argument("--scifact-zip", required=True)
    r.add_argument("--cache", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--only", nargs="*")
    a = ap.parse_args()
    encode(a) if a.cmd == "encode" else run(a)


if __name__ == "__main__":
    main()
