#!/usr/bin/env python3
"""The power computation of ADR-0075 d8 as amended on 2026-11-11.

    power.py > eval/bench/results/phase0/power.md

The amended primary comparison is `bm25-questions` against `bm25-sparse` on
the grown and synthetic sets' question-shaped categories (`question` and
`paraphrase`, n 687), a one-sided exact McNemar test at 0.025 (the primary
and the secondary share alpha 0.05 by Bonferroni, since a go needs either)
with a 1-point floor. Its power depends on the discordance rate pi_d (the
share of queries on which the two pipelines disagree at success@5) and the
gain delta: wins p10 = (pi_d + delta) / 2, losses p01 = (pi_d - delta) / 2.
For each planning pi_d this prints the least delta, in steps of 0.1 point,
whose power by exact enumeration (eval/bench/power.py's mcnemar_power) reaches
80% and 90%, with Connor's formula beside it.

The planning pi_d are the discordances the served pipelines measured on those
687 queries (PR #245's per-query runs and eval/recall/results/served_*.json):
`bm25-sparse` against `bm25-postings`, `bm25-rerank` and `bm25-sparse-rrf`.

The non-inferiority margin on the other categories (n 99) is m = (z_0.975 +
z_power) sqrt(pi_d) / sqrt(n), the normal approximation of the paired
success@5 difference's SD at a true difference of 0, with pi_d the same pair's
discordance there. Standard library only.
"""

import math
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "bench"))
from power import mcnemar_power  # noqa: E402

ND = statistics.NormalDist()
N_QS, N_OTHER = 687, 99
ALPHA, FLOOR = 0.025, 0.01
PLAN = [("`bm25-sparse` vs `bm25-postings`", 34), ("`bm25-sparse` vs `bm25-rerank`", 19),
        ("`bm25-sparse` vs `bm25-sparse-rrf`", 10)]
SPARSE_MISSES = 31  # served bm25-sparse misses the top 5 on 31 of the 687 (29 grown, 2 synthetic)
OTHER_DISCORDANT = 5  # bm25-sparse vs bm25-postings on the 99 other queries


def mde(n, pd, target):
    for g in range(1, int(pd * 1000) + 1):
        d = g / 1000
        if mcnemar_power(n, (pd + d) / 2, max((pd - d) / 2, 1e-12), ALPHA, FLOOR) >= target:
            return d
    return None


def connor(n, pd, target):
    za, zb = ND.inv_cdf(1 - ALPHA), ND.inv_cdf(target)
    lo, hi = 1e-5, pd
    for _ in range(80):
        d = (lo + hi) / 2
        if (za * math.sqrt(pd) + zb * math.sqrt(pd - d * d)) ** 2 / (d * d) > n:
            lo = d
        else:
            hi = d
    return hi


def main():
    out = ["# ADR-0075 d8 as amended on 2026-11-11: the gate's power\n",
           f"Primary and secondary: n {N_QS} question-shaped queries, one-sided exact McNemar at "
           f"{ALPHA}, floor {FLOOR * 100:.0f} point. `bm25-sparse` misses the top 5 on "
           f"{SPARSE_MISSES} of them ({SPARSE_MISSES / N_QS:.1%}), the most any arm can win.\n",
           "| planning discordance | pi_d | least gain for 80% power (exact) | for 90% (exact) | "
           "Connor, 80% |", "|---|---|---|---|---|"]
    for name, k in PLAN:
        pd = k / N_QS
        d80, d90 = mde(N_QS, pd, 0.80), mde(N_QS, pd, 0.90)
        c90 = f"{d90 * 100:.1f} points" if d90 else "not reachable"
        out.append(f"| {name} ({k} of {N_QS}) | {pd:.4f} | {d80 * 100:.1f} points | {c90} | "
                   f"{connor(N_QS, pd, 0.80) * 100:.1f} points |")
    pd = PLAN[0][1] / N_QS
    pw = mcnemar_power(N_QS, (pd + 0.025) / 2, (pd - 0.025) / 2, ALPHA, FLOOR)
    out.append(f"\nAt the planning discordance {pd:.4f}, a 2.5-point gain is detected with power "
               f"{pw:.3f}.\n")
    po = OTHER_DISCORDANT / N_OTHER
    out.append(f"Non-inferiority: n {N_OTHER} other queries, planning pi_d {po:.4f} "
               f"({OTHER_DISCORDANT} of {N_OTHER}).\n")
    out.append("| power at a true difference of 0 | m |")
    out.append("|---|---|")
    for p in (0.80, 0.90, 0.95):
        m = (ND.inv_cdf(0.975) + ND.inv_cdf(p)) * math.sqrt(po) / math.sqrt(N_OTHER)
        out.append(f"| {p:.2f} | {m:.4f} |")
    print("\n".join(out))


if __name__ == "__main__":
    main()
