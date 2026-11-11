#!/usr/bin/env python3
"""Phase 0 offline: d6's filters, BM25F with a question field, and the 2026-11-11 gate.

    score.py --cache CACHE.json --questions FILE --out DIR

Reads prepare.py's cache (the served lexemes and sparse vectors) and
generate.py's raw questions, and writes DIR/results.json (every figure),
DIR/sets.json (each memory's kept questions) and DIR/per-query/<pipeline>.json
(the top 10 of every query); report.py renders DIR/report.md from them. A
variant's pipeline is named `<pipeline>.<variant>`: `strict` (d6 as worded),
`ten` (ten questions drawn) and `unfiltered`. Standard library only.

**Filters** (ADR-0075 d6), per memory, over the questions in the generator's
order, each dropped with the first reason it fails: `shape` (over 200
characters, more than one line, no closing `?`, `://`, a backtick fence, a
control or format character, or a repeat after NFC, case and whitespace
folding); `generic` (under two content lexemes, or all of them in the
generic lexicon of `oml-questions/1`); `grounding` (over 8 content lexemes,
or fewer than half of them among the memory's `english` forms); `near_duplicate`
(a Jaccard index of 0.5 or more over content lexemes with a kept question);
`novel_cap` (the set would hold more than 10 content lexemes absent from the
memory); `scan` (the texts `--scan-findings` lists; oml-scan found none);
then the first five survive. Content lexemes are the `english` lexemes of
the question. The arm the 2026-11-11 gate pre-registers runs the `?` clause
after the adapter closes an unpunctuated question with `?` (a doc2query T5
writes MS MARCO-style queries without one); `--strict` is d6 as worded.

**Pipelines**, each over the 200 memories, top 100:
- `bm25-postings`: the served statement's per-word maximum over forms of
  idf(f) g(tf/B), k1 1.2, b 0.75, with ADR-0052 d6's pruning (a form held by
  more than `FREQUENT_SHARE` 0.1 of N is pruned) and step 5's completion of the
  best 4k candidates by partial score;
- `sparse`: sum over the query's tokens of idf(t) w_doc(t);
- `bm25-sparse`: ADR-0052 d13's convex combination, 0.3 `bm25-postings` and
  0.7 `sparse`, each list min-max normalised over its own candidates;
- `bm25-questions`: ADR-0075 d4's S(d) = T(d) + sum_t idf_q(t) [g(y_t + w_q
  q_t) - g(y_t)], idf_q over n_U = min(N, df_1 + novel_rows), the question
  field pruned on its own by n_U, the completion window the union of the best
  4k by partial T and by partial S; per word, y_t is the form
  `bm25-postings` scores it by and idf_q the maximum over forms;
- `bm25-sparse-questions`: `bm25-sparse` with `bm25-questions` in place of
  `bm25-postings`.

**w_q** is chosen per pipeline by five-fold cross-validation on the grown set
(its folds are by memory), over 0.1 to 0.9 by 0.1, by mean nDCG@10 on the
four training folds, and applied to the held-out fold; the 121 synthetic
queries take the w_q tuned on the whole grown set. Every figure is
out-of-fold.

**The gate** (ADR-0075 d8 as amended on 2026-11-11): primary,
`bm25-questions` against `bm25-sparse` on the question-shaped categories
(`question` and `paraphrase`); secondary, `bm25-sparse-questions` against
`bm25-sparse` there; each a one-sided exact McNemar test at 0.025 with a
1-point floor; non-inferiority on the other categories against the pipeline
each extends, the lower end of the 95% paired bootstrap interval (10,000
resamples) of the success@5 difference above -m, m computed from the
planning discordance.
"""

import argparse
import json
import math
import random
import statistics
import sys
import unicodedata
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parents[1] / "bench"))
import metrics  # noqa: E402
from power import mcnemar_power, tail, wilson  # noqa: E402

K1, B = 1.2, 0.75
FREQUENT_SHARE = 0.1
DEPTH = 100
WINDOW = 40  # step 5 completes the best 4k candidates, k 10
CC = (0.3, 0.7)
GRID = [round(0.1 * i, 1) for i in range(1, 10)]
MAX_Q = 5
GENERIC = {"configur", "recommend", "way", "use", "work", "best"}  # oml-questions/1, d6
QS = ("question", "paraphrase")
ALPHA = 0.025
FLOOR = 0.01
PLAN_QS = 34 / 687  # bm25-sparse vs bm25-postings discordance on the question-shaped queries
PLAN_OTHER = 5 / 99  # the same pair on the other categories
NI_Z = 1.959964 + 1.281552  # two-sided 95% interval, 90% power at a true difference of 0
BOOT = 10_000
SEED = 20261111


def g(x):
    return x * (K1 + 1) / (K1 + x)


def idf(n, df):
    return math.log(1 + (n - df + 0.5) / (df + 0.5))


def fold_text(q):
    return " ".join(unicodedata.normalize("NFC", q).casefold().split())


def bad_char(q):
    return any(unicodedata.category(c) in ("Cc", "Cf") for c in q)


def filter_set(qs, memory_english, lex, strict, scan_hits, limit):
    """d6's filters over one memory's questions, in order; returns (kept, drops)."""
    kept, drops, seen, novel = [], [], set(), set()
    for raw in qs[:limit]:
        q = raw.strip()
        if not strict and q and q[-1] not in "?.!":
            q = q + "?"
        content = set(lex[raw]["content"])
        reason = None
        if (len(q) > 200 or "\n" in q or not q.endswith("?") or "://" in q or "```" in q
                or bad_char(q) or fold_text(q) in seen):
            reason = "shape"
        elif len(content) < 2 or content <= GENERIC:
            reason = "generic"
        elif len(content) > 8 or 2 * len(content & memory_english) < len(content):
            reason = "grounding"
        elif any(len(content & k[1]) / len(content | k[1]) >= 0.5 for k in kept):
            reason = "near_duplicate"
        elif len(novel | (content - memory_english)) > 10:
            reason = "novel_cap"
        elif raw in scan_hits:
            reason = "scan"
        seen.add(fold_text(q))
        if reason:
            drops.append(reason)
            continue
        kept.append((raw, content))
        novel |= content - memory_english
        if len(kept) == MAX_Q:
            break
    return [k[0] for k in kept], drops


class Index:
    def __init__(self, docs, fields):
        self.ids = sorted(docs)
        self.n = len(self.ids)
        self.tf = {d: docs[d]["tf"] for d in self.ids}
        self.avg = max(statistics.fmean(docs[d]["len"] for d in self.ids), 1)
        self.bnorm = {d: 1 - B + B * docs[d]["len"] / self.avg for d in self.ids}
        self.df1 = {}
        for d in self.ids:
            for f in self.tf[d]:
                self.df1[f] = self.df1.get(f, 0) + 1
        self.field = fields
        self.novel = {}
        for d, lexs in fields.items():
            for f in lexs:
                if f not in self.tf[d]:
                    self.novel[f] = self.novel.get(f, 0) + 1

    def text_terms(self, kept):
        """Per row, per kept word: (best text contribution, its y)."""
        out = {}
        pruned = {f for _, forms in kept for f in forms
                  if self.df1.get(f, 0) > FREQUENT_SHARE * self.n}
        partial, full = {}, {}
        for w, forms in kept:
            for f in forms:
                if f not in self.df1:
                    continue
                i = idf(self.n, self.df1[f])
                for d in self.ids:
                    tf = self.tf[d].get(f)
                    if not tf:
                        continue
                    y = tf / self.bnorm[d]
                    c = i * g(y)
                    cur = out.setdefault(d, {}).get(w)
                    if cur is None or c > cur[0]:
                        out[d][w] = (c, y)
                    if f not in pruned:
                        p = partial.setdefault(d, {})
                        p[w] = max(p.get(w, 0.0), c)
        for d, words in out.items():
            full[d] = sum(c for c, _ in words.values())
        return out, partial, full, pruned

    def bm25(self, kept):
        _, partial, full, _ = self.text_terms(kept)
        return self._complete({d: sum(p.values()) for d, p in partial.items()}, full, None)

    def _complete(self, part, full, extra):
        """Step 5: rows in the window(s) get their full score, others keep the partial one."""
        cands = set(part) | (set(extra) if extra else set())
        if not cands:
            return {}
        if extra is None:
            window = set(sorted(part, key=lambda d: (-part[d], d))[:WINDOW])
            return {d: (full[d] if d in window else part[d]) for d in part}
        t_part = {d: part.get(d, 0.0) for d in cands}
        s_part = {d: t_part[d] + extra[d][1] for d in cands}
        window = set(sorted(t_part, key=lambda d: (-t_part[d], d))[:WINDOW])
        window |= set(sorted(s_part, key=lambda d: (-s_part[d], d))[:WINDOW])
        out = {}
        for d in cands:
            if d in window:
                out[d] = extra[d][0]
            else:
                out[d] = s_part[d]
        return out

    def bm25f(self, kept, wq):
        terms, partial, full, pruned = self.text_terms(kept)
        inc_full, inc_part = {}, {}
        for w, forms in kept:
            fforms = [f for f in forms]
            holders = [d for d in self.ids if any(f in self.field.get(d, ()) for f in fforms)]
            if not holders:
                continue
            iq = 0.0
            for f in fforms:
                nu = min(self.n, self.df1.get(f, 0) + self.novel.get(f, 0))
                if nu == 0 or nu > FREQUENT_SHARE * self.n:
                    continue
                iq = max(iq, idf(self.n, nu))
            if iq == 0.0:
                continue
            for d in holders:
                c, y = terms.get(d, {}).get(w, (0.0, 0.0))
                add = iq * (g(y + wq) - g(y))
                inc_full[d] = inc_full.get(d, 0.0) + add
                # the partial S uses the partial text term for this word
                py = y if (w in partial.get(d, {})) else 0.0
                inc_part[d] = inc_part.get(d, 0.0) + iq * (g(py + wq) - g(py))
        part = {d: sum(p.values()) for d, p in partial.items()}
        extra = {}
        for d in set(part) | set(inc_full):
            extra[d] = (full.get(d, 0.0) + inc_full.get(d, 0.0), inc_part.get(d, 0.0))
        return self._complete(part, full, extra)


def top(scores, k=DEPTH):
    return sorted(scores.items(), key=lambda x: (-x[1], x[0]))[:k]


def cc(lists, weights):
    fused = {}
    for lst, w in zip(lists, weights):
        if not lst:
            continue
        hi, lo = lst[0][1], lst[-1][1]
        for d, s in lst:
            v = 1.0 if hi == lo else (s - lo) / (hi - lo)
            fused[d] = fused.get(d, 0.0) + w * v
    return [d for d, _ in top(fused)]


def sparse_scores(dv, qv):
    out = {}
    for d, vec in dv.items():
        s = sum(w * vec.get(t, 0.0) for t, w in qv.items())
        if s > 0:
            out[d] = s
    return out


def evaluate(ranked, rel):
    return {"s5": metrics.success_at(ranked, rel, 5), "ndcg10": metrics.ndcg_at(ranked, rel, 10)}


def paired_boot(a, b, seed=SEED):
    rng = random.Random(seed)
    d = [x - y for x, y in zip(a, b)]
    n = len(d)
    means = sorted(sum(d[rng.randrange(n)] for _ in range(n)) / n for _ in range(BOOT))
    return statistics.fmean(d), means[int(0.025 * BOOT)], means[int(0.975 * BOOT) - 1]


def mcnemar(a, b):
    w = sum(1 for x, y in zip(a, b) if x > y)
    lo = sum(1 for x, y in zip(a, b) if x < y)
    return w, lo, (tail(w + lo, w) if w + lo else 1.0)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", required=True)
    ap.add_argument("--questions", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--scan-findings", help="a JSON list of question texts the scan found something in")
    a = ap.parse_args()
    c = json.loads(Path(a.cache).read_text())
    docs, queries, lex = c["docs"], c["queries"], c["questions"]
    gen, meta = {}, None
    for line in open(a.questions):
        r = json.loads(line)
        if "generator" in r:
            meta = r["generator"]
        else:
            gen[r["id"]] = r
    scan_hits = set(json.loads(Path(a.scan_findings).read_text())) if a.scan_findings else set()
    english = {d: set(docs[d]["english"]) for d in docs}

    variants = {"arm": (False, 5), "strict": (True, 5), "ten": (False, 10), "unfiltered": (None, 5)}
    sets, drops = {}, {}
    for v, (strict, limit) in variants.items():
        sets[v], drops[v] = {}, {}
        for d in docs:
            qs = gen[d]["questions"]
            if strict is None:
                kept, dr = qs[:limit], []
            else:
                kept, dr = filter_set(qs, english[d], lex, strict, scan_hits, limit)
            sets[v][d] = kept
            for r in dr:
                drops[v][r] = drops[v].get(r, 0) + 1
    fields = {v: {d: {f for q in s[d] for f in lex[q]["lexemes"]} for d in s} for v, s in sets.items()}

    base = Index(docs, {})
    idx = {v: Index(docs, fields[v]) for v in variants}
    sp = {q["id"]: top(sparse_scores(c["sparse_docs"], c["sparse_queries"][q["id"]])) for q in queries}
    bm = {q["id"]: top(base.bm25(q["kept"])) for q in queries}

    runs = {"bm25-postings": {q["id"]: [d for d, _ in bm[q["id"]]] for q in queries},
            "sparse": {q["id"]: [d for d, _ in sp[q["id"]]] for q in queries},
            "bm25-sparse": {q["id"]: cc([bm[q["id"]], sp[q["id"]]], CC) for q in queries}}

    cache = {}

    def bmf(v, wq, q):
        key = (v, wq, q["id"])
        if key not in cache:
            cache[key] = top(idx[v].bm25f(q["kept"], wq))
        return cache[key]

    def ranked(v, kind, wq, q):
        lst = bmf(v, wq, q)
        if kind == "bm25-questions":
            return [d for d, _ in lst]
        return cc([lst, sp[q["id"]]], CC)

    grown = [q for q in queries if q["set"] == "grown"]
    chosen = {}
    for v in variants:
        for kind in ("bm25-questions", "bm25-sparse-questions"):
            name = kind if v == "arm" else f"{kind}.{v}"
            per_fold = {}
            for f in range(5):
                train = [q for q in grown if q["fold"] != f]
                best = max(GRID, key=lambda w: (statistics.fmean(
                    evaluate(ranked(v, kind, w, q), q["relevant"])["ndcg10"] for q in train), -w))
                per_fold[f] = best
            whole = max(GRID, key=lambda w: (statistics.fmean(
                evaluate(ranked(v, kind, w, q), q["relevant"])["ndcg10"] for q in grown), -w))
            chosen[name] = {"per_fold": per_fold, "synthetic": whole}
            runs[name] = {q["id"]: ranked(v, kind, per_fold[q["fold"]] if q["fold"] is not None
                                          else whole, q) for q in queries}

    scores = {p: {q["id"]: evaluate(r[q["id"]], q["relevant"]) for q in queries} for p, r in runs.items()}
    cat = {q["id"]: q["category"] for q in queries}
    groups = {"all": [q["id"] for q in queries],
              "question-shaped": [q["id"] for q in queries if q["category"] in QS],
              "other": [q["id"] for q in queries if q["category"] not in QS],
              "grown": [q["id"] for q in grown],
              "synthetic": [q["id"] for q in queries if q["set"] == "synthetic"]}
    for k in sorted({q["category"] for q in queries}):
        groups[f"category:{k}"] = [q["id"] for q in queries if q["category"] == k]

    res = {"generator": meta, "variants": {v: {"strict": s, "limit": lim} for v, (s, lim) in variants.items()},
           "drops": drops, "w_q": chosen, "pipelines": {}}
    for p in runs:
        res["pipelines"][p] = {}
        for gname, ids in groups.items():
            s5 = [scores[p][i]["s5"] for i in ids]
            k = int(sum(s5))
            lo, hi = wilson(k, len(ids))
            res["pipelines"][p][gname] = {"n": len(ids), "hits": k, "success@5": round(k / len(ids), 4),
                                          "ci95": [round(lo, 4), round(hi, 4)],
                                          "ndcg@10": round(statistics.fmean(scores[p][i]["ndcg10"] for i in ids), 4)}

    # parity with the served runs
    served = {}
    root = HERE.parents[1]
    for p in ("bm25-postings", "bm25-sparse"):
        g_ = json.loads((root / f"bench/results/per-query/recall-grown/{p}.json").read_text())["queries"]
        s_ = json.loads((root / f"recall/results/served_{p.replace('-', '_')}.json").read_text())["per_query"]
        agree = top1 = 0
        for q in queries:
            if q["set"] == "grown":
                srv5, srv1 = g_[q["id"]]["scores"]["success@5"], g_[q["id"]]["ranked"][:1]
            else:
                srv5, srv1 = s_[q["id"]]["scores"]["s5"], s_[q["id"]]["ranked"][:1]
            agree += srv5 == scores[p][q["id"]]["s5"]
            top1 += srv1 == runs[p][q["id"]][:1]
        served[p] = {"success@5_agree": agree, "top1_agree": top1, "n": len(queries),
                     "served_success@5_all": round(sum(
                         (g_[q["id"]]["scores"]["success@5"] if q["set"] == "grown"
                          else s_[q["id"]]["scores"]["s5"]) for q in queries) / len(queries), 4)}
    res["parity"] = served

    def compare(p, base_p, ids):
        a_, b_ = [scores[p][i]["s5"] for i in ids], [scores[base_p][i]["s5"] for i in ids]
        w, lo, pv = mcnemar(a_, b_)
        mean, l95, h95 = paired_boot(a_, b_)
        return {"pipeline": p, "against": base_p, "n": len(ids), "wins": w, "losses": lo,
                "gain": round(mean, 4), "mcnemar_p": pv, "boot95": [round(l95, 4), round(h95, 4)],
                "ndcg_gain": round(statistics.fmean(scores[p][i]["ndcg10"] - scores[base_p][i]["ndcg10"]
                                                    for i in ids), 4)}

    qs_ids, other = groups["question-shaped"], groups["other"]
    m = NI_Z * math.sqrt(PLAN_OTHER) / math.sqrt(len(other))
    gate = {"alpha": ALPHA, "floor": FLOOR, "margin": round(m, 4)}
    gate["primary"] = compare("bm25-questions", "bm25-sparse", qs_ids)
    gate["secondary"] = compare("bm25-sparse-questions", "bm25-sparse", qs_ids)
    for k in ("primary", "secondary"):
        t = gate[k]
        t["passes"] = t["mcnemar_p"] < ALPHA and t["gain"] >= FLOOR
        dpi = (t["wins"] + t["losses"]) / t["n"]
        t["observed_discordance"] = round(dpi, 4)
    gate["non_inferiority"] = {
        "bm25-questions": compare("bm25-questions", "bm25-postings", other),
        "bm25-sparse-questions": compare("bm25-sparse-questions", "bm25-sparse", other),
        "bm25-questions vs bm25-sparse (reported)": compare("bm25-questions", "bm25-sparse", other)}
    for k in ("bm25-questions", "bm25-sparse-questions"):
        t = gate["non_inferiority"][k]
        t["passes"] = t["boot95"][0] > -m
    gate["go"] = ((gate["primary"]["passes"] and gate["non_inferiority"]["bm25-questions"]["passes"])
                  or (gate["secondary"]["passes"]
                      and gate["non_inferiority"]["bm25-sparse-questions"]["passes"]))
    gate["reported"] = {
        "bm25-questions vs bm25-postings, question-shaped (d8's original arm)":
            compare("bm25-questions", "bm25-postings", qs_ids),
        "bm25-questions vs bm25-postings, all": compare("bm25-questions", "bm25-postings", groups["all"]),
        "bm25-sparse-questions vs bm25-sparse, all": compare("bm25-sparse-questions", "bm25-sparse",
                                                             groups["all"])}
    for v in ("strict", "ten", "unfiltered"):
        for kind in ("bm25-questions", "bm25-sparse-questions"):
            gate["reported"][f"{kind}.{v} vs bm25-sparse, question-shaped"] = compare(
                f"{kind}.{v}", "bm25-sparse", qs_ids)
    # power at the observed discordance and at the planned effect
    for k in ("primary", "secondary"):
        t = gate[k]
        pd = t["observed_discordance"]
        t["mde80_at_observed_discordance"] = None
        if pd > 0:
            for g10 in range(1, int(pd * 1000) + 1):
                dd = g10 / 1000
                if mcnemar_power(t["n"], (pd + dd) / 2, max((pd - dd) / 2, 1e-12), ALPHA, FLOOR) >= 0.8:
                    t["mde80_at_observed_discordance"] = dd
                    break
    res["gate"] = gate

    # costs
    rows = list(gen.values())
    kept = sets["arm"]
    kind3 = {d: len(fields["arm"][d]) for d in docs}
    kind1 = {d: docs[d]["len"] for d in docs}
    res["cost"] = {
        "memories": len(rows),
        "generation_seconds_sum": round(sum(r["seconds"] for r in rows), 1),
        "generation_seconds_median": round(statistics.median(r["seconds"] for r in rows), 3),
        "wall_seconds": meta["wall_seconds"] if meta else None,
        "tokens_in": sum(r["tokens_in"] for r in rows),
        "tokens_out_10_drawn": sum(r["tokens_out"] for r in rows),
        "questions_kept": sum(len(v) for v in kept.values()),
        "memories_with_a_set": sum(1 for v in kept.values() if v),
        "question_bytes": sum(len(q.encode()) for v in kept.values() for q in v),
        "kind3_postings": sum(kind3.values()), "kind1_postings": sum(kind1.values()),
        "kind3_over_kind1": round(sum(kind3.values()) / sum(kind1.values()), 3)}

    out = Path(a.out)
    (out / "per-query").mkdir(parents=True, exist_ok=True)
    for p, r in runs.items():
        (out / "per-query" / f"{p}.json").write_text(json.dumps(
            {"pipeline": p, "queries": {i: {"category": cat[i], "ranked": r[i][:10], **scores[p][i]}
                                        for i in r}}, sort_keys=True) + "\n")
    (out / "sets.json").write_text(json.dumps(sets["arm"], indent=0, sort_keys=True) + "\n")
    (out / "results.json").write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")
    print(json.dumps({k: res[k] for k in ("parity", "drops", "w_q", "cost")}, indent=1))
    for p in runs:
        r = res["pipelines"][p]
        print(f"{p:42s} all {r['all']['success@5']:.3f} qs {r['question-shaped']['success@5']:.3f} "
              f"other {r['other']['success@5']:.3f} nDCG qs {r['question-shaped']['ndcg@10']:.3f}")
    print(json.dumps(gate, indent=1))


if __name__ == "__main__":
    main()
