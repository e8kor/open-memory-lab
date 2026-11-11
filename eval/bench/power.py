#!/usr/bin/env python3
"""Phase 0a's rerun of ADR-0075 d8's power computation on measured runs.

    power.py --runs RAW [--json OUT.json] > OUT.md

RAW holds bench.py run files (`<dataset>_<pipeline>.json`, per-query scores
included) for the grown set (`grown_*`), SciFact, NFCorpus and LoCoMo, each
with `bm25-postings` and `bm25-sparse`. It reports, in d8's order:

1. **Baselines on the grown set**: success@5 and nDCG@10 per pipeline, overall
   and per category, success@5 with a 95% Wilson interval; and each pipeline's
   wins and losses against `bm25-postings` with the one-sided exact McNemar p.
2. **The superiority arm's power**: exact enumeration of the one-sided
   conditional McNemar test (every count of wins W and losses L, weighted by
   its multinomial probability, passing when P(Bin(W + L, 0.5) >= W) < 0.05
   and the gain is at least the 1.5-point floor), at the grown set's n, at
   d8's planning effect (wins 5.0%, losses 1.7%) and at the wins and losses
   `bm25-sparse` measured against `bm25-postings` there, the expansion OML has
   measured; Connor's formula beside it; and the n each needs for 80% and 90%.
   The baseline's misses bound the wins any arm can have.
3. **Each non-inferiority margin**: sigma_i, the SD of the per-query nDCG@10
   difference `bm25-sparse` minus `bm25-postings`, m_i = max(0.01, (1.960 +
   2.326) sigma_i / sqrt(n_i)) capped at 0.02, and the arm's power at a true
   difference of 0, Phi(m_i sqrt(n_i) / sigma_i - 1.960). LoCoMo uses the SD
   of its ten per-conversation means and Student's t with 9 degrees of
   freedom, m = (2.262 + 2.821) sigma / sqrt(10), and its power is simulated
   with the exact 1,024-assignment sign-flip test (4,000 runs, seeded).
4. **The joint power**: 1 minus the sum of every gate's type II error (the
   union bound), with the displacement arm's 0.033 from d8.

Standard library only.
"""

import argparse
import itertools
import json
import math
import random
import statistics
import sys
from pathlib import Path

ND = statistics.NormalDist()
ALPHA = 0.05
FLOOR = 0.015
PLAN = (0.05, 0.017)
DISPLACEMENT_BETA = 0.033  # d8: 0.967 at a true 1% displacement
BASE = "bm25-postings"
SPARSE = "bm25-sparse"
NI_SETS = [("scifact", "SciFact"), ("nfcorpus", "NFCorpus")]
SEED = 20261111


def load(raw, dataset, pipeline):
    p = Path(raw) / f"{dataset}_{pipeline.replace('-', '_')}.json"
    return json.loads(p.read_text()) if p.exists() else None


def wilson(k, n, z=1.959964):
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return c - h, c + h


def lbin(n, k):
    return math.lgamma(n + 1) - math.lgamma(k + 1) - math.lgamma(n - k + 1)


def tail(m, w):
    """P(Bin(m, 0.5) >= w)."""
    return sum(math.exp(lbin(m, k) - m * math.log(2)) for k in range(w, m + 1))


def mcnemar_power(n, p10, p01, alpha=ALPHA, floor=FLOOR):
    p0 = 1 - p10 - p01
    span = lambda p: min(n, int(n * p + 10 * math.sqrt(n * p + 1)) + 5) if p > 0 else 0  # noqa: E731
    total = 0.0
    for w in range(span(p10) + 1):
        for lo in range(span(p01) + 1):
            if w + lo > n:
                break
            if (w - lo) / n < floor or tail(w + lo, w) >= alpha:
                continue
            lp = math.lgamma(n + 1) - math.lgamma(w + 1) - math.lgamma(lo + 1) - math.lgamma(n - w - lo + 1)
            lp += (w * math.log(p10) if w else 0) + (lo * math.log(p01) if lo else 0)
            lp += (n - w - lo) * math.log(p0) if n - w - lo else 0
            total += math.exp(lp)
    return total


def n_for(target, p10, p01, start=50, stop=5000):
    """The least n whose power reaches target, as d8 counts it (the power saw-tooths above it)."""
    for n in range(start, stop):
        if mcnemar_power(n, p10, p01) >= target:
            return n
    return None


def connor(p10, p01, beta_z):
    d = p10 - p01
    s = p10 + p01
    return (ND.inv_cdf(1 - ALPHA) * math.sqrt(s) + beta_z * math.sqrt(s - d * d)) ** 2 / (d * d)


def per_query(run, measure):
    return {q: row["scores"][measure] for q, row in run["per_query"].items()}


def margin(sigma, n, za=1.960, zb=2.326):
    m = max(0.01, (za + zb) * sigma / math.sqrt(n))
    return min(m, 0.02), m > 0.02


def sign_flip_power(sigma, m, runs=4000, seed=SEED):
    """Power of the one-sided exact sign-flip test on ten conversations at a true difference of 0."""
    rng = random.Random(seed)
    signs = list(itertools.product((1, -1), repeat=10))
    passed = 0
    for _ in range(runs):
        d = [rng.gauss(0.0, sigma) + m for _ in range(10)]
        obs = sum(d)
        p = sum(1 for s in signs if sum(x * y for x, y in zip(s, d)) >= obs - 1e-12) / len(signs)
        passed += p < 0.025
    return passed / runs


def grown_section(raw, out, lines):
    runs = {p: load(raw, "grown", p) for p in (BASE, "bm25-rerank", SPARSE, "bm25-sparse-rrf")}
    runs = {p: r for p, r in runs.items() if r}
    base = runs[BASE]
    lines.append("## 1. Baselines on the grown set\n")
    lines.append("| pipeline | n | success@5 | 95% interval (Wilson) | nDCG@10 | wins / losses vs "
                 "`bm25-postings` | one-sided exact McNemar p |")
    lines.append("|---|---|---|---|---|---|---|")
    b5 = per_query(base, "Success@5")
    out["grown"] = {}
    for p, r in runs.items():
        s5 = per_query(r, "Success@5")
        nd = per_query(r, "nDCG@10")
        n = len(s5)
        k = int(round(sum(s5.values())))
        lo, hi = wilson(k, n)
        w = sum(1 for q in s5 if s5[q] > b5[q])
        lo_ = sum(1 for q in s5 if s5[q] < b5[q])
        pval = tail(w + lo_, w) if w + lo_ else 1.0
        out["grown"][p] = {"n": n, "success@5": round(k / n, 4), "hits": k,
                           "ci95": [round(lo, 4), round(hi, 4)],
                           "ndcg@10": round(statistics.fmean(nd.values()), 4),
                           "wins": w, "losses": lo_, "mcnemar_p": round(pval, 4)}
        vs = "—" if p == BASE else f"{w} / {lo_}"
        pv = "—" if p == BASE else (f"{pval:.4f}" if pval >= 0.0001 else "< 0.0001")
        lines.append(f"| `{p}` | {n} | {k / n:.3f} ({k}/{n}) | {lo:.3f} to {hi:.3f} | "
                     f"{statistics.fmean(nd.values()):.3f} | {vs} | {pv} |")
    lines.append("")
    by_cat = {}
    for q, row in base["per_query"].items():
        by_cat.setdefault(row["category"], []).append(q)
    lines.append("| pipeline | " + " | ".join(f"{c} success@5 (95% interval)" for c in sorted(by_cat)) + " |")
    lines.append("|---|" + "---|" * len(by_cat))
    for p, r in runs.items():
        s5 = per_query(r, "Success@5")
        cells = []
        for c in sorted(by_cat):
            ids = by_cat[c]
            k = int(round(sum(s5[q] for q in ids)))
            lo, hi = wilson(k, len(ids))
            out["grown"][p].setdefault("per_category", {})[c] = {
                "n": len(ids), "success@5": round(k / len(ids), 4), "ci95": [round(lo, 4), round(hi, 4)]}
            cells.append(f"{k / len(ids):.3f} ({k}/{len(ids)}; {lo:.3f} to {hi:.3f})")
        lines.append(f"| `{p}` | " + " | ".join(cells) + " |")
    lines.append("")
    return runs


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--runs", required=True)
    ap.add_argument("--json")
    a = ap.parse_args()
    out, lines = {}, ["# ADR-0075 phase 0a: baselines and power on the grown set\n"]
    grown_section(a.runs, out, lines)

    g = out["grown"]
    n = g[BASE]["n"]
    misses = n - g[BASE]["hits"]
    sp = g.get(SPARSE)
    lines.append("## 2. The superiority arm's power\n")
    rows = [("d8's planning effect", PLAN[0], PLAN[1])]
    if sp:
        rows.append((f"`{SPARSE}` vs `{BASE}`, measured here", sp["wins"] / n, sp["losses"] / n))
    lines.append(f"n = {n}; `{BASE}` misses the top 5 on {misses} queries ({misses / n:.1%}), "
                 f"so no arm can win more than {misses / n:.1%} of queries.\n")
    lines.append("| effect | wins p10 | losses p01 | gain | power at n (exact, 1.5-point floor) | "
                 "n for 80% / 90% (exact) | Connor n for 80% / 90% |")
    lines.append("|---|---|---|---|---|---|---|")
    out["superiority"] = []
    for name, p10, p01 in rows:
        if p10 <= p01:
            lines.append(f"| {name} | {p10:.4f} | {p01:.4f} | {p10 - p01:+.4f} | no gain to detect | — | — |")
            out["superiority"].append({"effect": name, "p10": p10, "p01": p01, "power": None})
            continue
        pw = mcnemar_power(n, p10, p01)
        n80, n90 = n_for(0.80, p10, p01), n_for(0.90, p10, p01)
        c80, c90 = connor(p10, p01, ND.inv_cdf(0.80)), connor(p10, p01, ND.inv_cdf(0.90))
        out["superiority"].append({"effect": name, "p10": round(p10, 4), "p01": round(p01, 4),
                                   "power": round(pw, 4), "n80": n80, "n90": n90,
                                   "connor80": round(c80), "connor90": round(c90)})
        lines.append(f"| {name} | {p10:.4f} | {p01:.4f} | {p10 - p01:+.4f} | {pw:.3f} | "
                     f"{n80} / {n90} | {c80:.0f} / {c90:.0f} |")
    lines.append("")
    beta_sup = 1 - out["superiority"][0]["power"]

    lines.append("## 3. Non-inferiority margins\n")
    lines.append("| set | n | sigma of the nDCG@10 difference | m (uncapped) | m used | capped | "
                 "power at a true difference of 0 |")
    lines.append("|---|---|---|---|---|---|---|")
    out["margins"] = {}
    betas = []
    for ds, title in NI_SETS:
        b, s = load(a.runs, ds, BASE), load(a.runs, ds, SPARSE)
        if not (b and s):
            lines.append(f"| {title} | — | not run | | | | |")
            continue
        nb, ns = per_query(b, "nDCG@10"), per_query(s, "nDCG@10")
        d = [ns[q] - nb[q] for q in nb]
        sigma = statistics.stdev(d)
        raw_m = max(0.01, (1.960 + 2.326) * sigma / math.sqrt(len(d)))
        m, capped = margin(sigma, len(d))
        pw = ND.cdf(m * math.sqrt(len(d)) / sigma - 1.960)
        betas.append(1 - pw)
        out["margins"][ds] = {"n": len(d), "sigma": round(sigma, 4), "m_uncapped": round(raw_m, 4),
                              "m": round(m, 4), "capped": capped, "power": round(pw, 4),
                              "mean_difference": round(statistics.fmean(d), 4)}
        lines.append(f"| {title} | {len(d)} | {sigma:.4f} | {raw_m:.4f} | {m:.4f} | "
                     f"{'yes' if capped else 'no'} | {pw:.3f} |")
    b, s = load(a.runs, "locomo", BASE), load(a.runs, "locomo", SPARSE)
    if b and s:
        nb, ns = per_query(b, "nDCG@10"), per_query(s, "nDCG@10")
        conv = {}
        for q, row in b["per_query"].items():
            if row.get("category") == "adversarial":
                continue
            conv.setdefault(q.split(".")[0], []).append(ns[q] - nb[q])
        means = [statistics.fmean(v) for v in conv.values()]
        sigma = statistics.stdev(means)
        raw_m = (2.262 + 2.821) * sigma / math.sqrt(len(means))
        m = min(raw_m, 0.02)
        pw = sign_flip_power(sigma, m)
        betas.append(1 - pw)
        out["margins"]["locomo"] = {"conversations": len(means),
                                    "queries": sum(len(v) for v in conv.values()),
                                    "sigma": round(sigma, 4), "m_uncapped": round(raw_m, 4),
                                    "m": round(m, 4), "capped": raw_m > 0.02, "power": round(pw, 4),
                                    "mean_difference": round(statistics.fmean(means), 4)}
        lines.append(f"| LoCoMo ({len(means)} conversations, "
                     f"{sum(len(v) for v in conv.values())} queries) | {len(means)} | {sigma:.4f} "
                     f"(of the per-conversation means) | {raw_m:.4f} | {m:.4f} | "
                     f"{'yes' if raw_m > 0.02 else 'no'} | {pw:.3f} (sign-flip, 4,000 runs) |")
    else:
        lines.append("| LoCoMo | — | not run | | | | |")
    lines.append("")

    joint = 1 - (beta_sup + sum(betas) + DISPLACEMENT_BETA)
    out["joint"] = {"beta_superiority": round(beta_sup, 4),
                    "beta_non_inferiority": [round(x, 4) for x in betas],
                    "beta_displacement": DISPLACEMENT_BETA, "power": round(joint, 4),
                    "arms_measured": 1 + len(betas)}
    lines.append("## 4. Joint power\n")
    verdict = ("at least 0.80: phase 0 may run on this set" if joint >= 0.80 and len(betas) == 3
               else "below 0.80, so by d8 phase 0 does not run yet. Growing the grown set raises "
               "only the superiority term; the non-inferiority terms are fixed by SciFact's, "
               "NFCorpus's and LoCoMo's own sizes and by the 0.02 cap on m"
               if len(betas) == 3 else "an arm is unmeasured")
    lines.append(f"1 − ({beta_sup:.3f} + " + " + ".join(f"{x:.3f}" for x in betas)
                 + f" + {DISPLACEMENT_BETA}) = **{joint:.3f}** at the planned effect, by the union "
                 "bound over the superiority arm at d8's planning effect, the "
                 f"{len(betas)} non-inferiority arms measured here and d8's displacement arm"
                 + (" (a negative bound says nothing: the arms' type II errors sum past 1)"
                    if joint < 0 else "") + f"; {verdict}.")
    print("\n".join(lines))
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=1) + "\n")


if __name__ == "__main__":
    sys.exit(main())
