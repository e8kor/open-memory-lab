#!/usr/bin/env python3
"""Renders phase 0's report from score.py's results (ADR-0075 d12).

    report.py DIR > DIR/report.md

Standard library only.
"""

import json
import sys
from pathlib import Path

MAIN = ["bm25-postings", "sparse", "bm25-sparse", "bm25-questions", "bm25-sparse-questions"]
VARIANTS = ["bm25-questions.strict", "bm25-sparse-questions.strict", "bm25-questions.ten",
            "bm25-sparse-questions.ten", "bm25-questions.unfiltered",
            "bm25-sparse-questions.unfiltered"]


def cell(r):
    return f"{r['success@5']:.3f} ({r['hits']}/{r['n']}; {r['ci95'][0]:.3f} to {r['ci95'][1]:.3f})"


def pct(x):
    return f"{x * 100:+.1f}"


def test_row(name, t):
    p = t["mcnemar_p"]
    pv = "< 0.0001" if p < 0.0001 else f"{min(p, 1.0):.4f}"
    return (f"| {name} | `{t['pipeline']}` vs `{t['against']}` | {t['n']} | {t['wins']} / {t['losses']} | "
            f"{pct(t['gain'])} points ({pct(t['boot95'][0])} to {pct(t['boot95'][1])}) | {pv} | "
            f"{pct(t['ndcg_gain'])} |")


def main():
    d = Path(sys.argv[1])
    r = json.loads((d / "results.json").read_text())
    g, P, c = r["gate"], r["pipelines"], r["cost"]
    gen = r["generator"]
    grown = json.loads((Path(__file__).resolve().parents[1] / "queries-grown.json").read_text())
    pc = grown["person_check"]
    if pc["status"] == "done":
        fixed = ", ".join(sorted(pc.get("corrections", {})))
        checked = (f"**Labels checked.** {pc['checked_by'].capitalize()} checked a seeded tenth of the "
                   f"grown set's labels on {pc['date']}: {pc['bad_labels']} of {pc['checked']} wrong "
                   f"({fixed}, corrected in version {grown['version']}, which this run uses).")
        caveat = ""
    else:
        checked = "**Provisional: pending the 10% label check** of the grown set."
        caveat = "Provisional until the label check passes."
    out = ["# ADR-0075 phase 0: generated questions against `bm25-sparse`\n",
           checked + " (`person_check` in "
           "`eval/recall/queries-grown.json`.) Offline: no server was built or run; the lexemes are "
           "migration 0020's, computed in a throwaway PostgreSQL, and the gate is ADR-0075 d12 "
           "(the operator's decision of 2026-11-11).\n",
           f"Generator: `{gen['model']}` at `{gen['revision'][:12]}` (weights SHA-256 "
           f"`{gen['weights_sha256'][:16]}…`, {gen['licence']}), top-k {gen['decoding']['top_k']}, "
           f"at most {gen['decoding']['max_new_tokens']} new tokens, ten drawn per memory, the first "
           "five the arm. Sparse leg: opensearch-neural-sparse-encoding-doc-v3-distill at `babf71f3`, "
           "as served.\n",
           "## Parity with the served runs\n",
           "| pipeline | queries whose success@5 matches the served run | top 1 matches |",
           "|---|---|---|"]
    for p, x in r["parity"].items():
        out.append(f"| `{p}` | {x['success@5_agree']} of {x['n']} | {x['top1_agree']} of {x['n']} |")
    out += ["", "## Success@5\n",
            "| pipeline | all (786) | question-shaped (687) | other (99) | nDCG@10, question-shaped |",
            "|---|---|---|---|---|"]
    for p in MAIN + VARIANTS:
        x = P[p]
        out.append(f"| `{p}` | {cell(x['all'])} | {cell(x['question-shaped'])} | {cell(x['other'])} | "
                   f"{x['question-shaped']['ndcg@10']:.3f} |")
    cats = sorted(k for k in P["bm25-postings"] if k.startswith("category:"))
    out += ["", "Per category (success@5):\n",
            "| category | n | " + " | ".join(f"`{p}`" for p in MAIN) + " |",
            "|---|---|" + "---|" * len(MAIN)]
    for k in cats:
        out.append(f"| {k.split(':')[1]} | {P['bm25-postings'][k]['n']} | "
                   + " | ".join(f"{P[p][k]['success@5']:.3f}" for p in MAIN) + " |")
    out += ["", "## The gate (d12)\n",
            "One-sided exact McNemar on success@5 at 0.025 per comparison, a 1-point floor; the gain "
            "with its 95% paired bootstrap interval (10,000 resamples).\n",
            "| arm | comparison | n | wins / losses | gain (95% interval) | p | nDCG@10 gain |",
            "|---|---|---|---|---|---|---|",
            test_row("primary", g["primary"]), test_row("secondary", g["secondary"])]
    for k in ("bm25-questions", "bm25-sparse-questions"):
        out.append(test_row(f"non-inferiority, other, m {g['margin']:.3f}", g["non_inferiority"][k]))
    out.append(test_row("reported", g["non_inferiority"]["bm25-questions vs bm25-sparse (reported)"]))
    for k, t in g["reported"].items():
        out.append(test_row("reported", t))
    pr, se = g["primary"], g["secondary"]
    mde = pr["mde80_at_observed_discordance"]
    out += ["",
            f"Primary: {'passes' if pr['passes'] else 'fails'}. Observed discordance "
            f"{pr['observed_discordance']:.4f}; at it the set detects a gain of "
            f"{mde * 100:.1f} points with 80% power, and the observed difference is "
            f"{pct(pr['gain'])} points." if mde else "",
            f"Secondary: {'passes' if se['passes'] else 'fails'}. Observed discordance "
            f"{se['observed_discordance']:.4f} ({se['wins']} wins, {se['losses']} losses): the 95% "
            f"interval rules out an added gain above {se['boot95'][1] * 100:.1f} points.",
            "",
            f"**Verdict: {'go' if g['go'] else 'no go'}.** "
            + ("" if g["go"] else "Neither comparison passes, so by d12 v0.7.1 recommends `bm25-sparse` "
               "for question-style use and #236 to #238 are parked with this evidence. ")
            + caveat, "",
            "## What the filters dropped (of 1,000 questions, five per memory)\n",
            "| variant | " + " | ".join(["shape", "generic", "grounding", "near_duplicate",
                                         "novel_cap", "scan"]) + " |",
            "|---|---|---|---|---|---|---|"]
    for v in ("arm", "strict", "ten"):
        dr = r["drops"][v]
        out.append(f"| {v} | " + " | ".join(str(dr.get(k, 0)) for k in
                                           ("shape", "generic", "grounding", "near_duplicate",
                                            "novel_cap", "scan")) + " |")
    out += ["", "`ten` draws ten per memory (2,000 questions). oml-scan (`oml-scan/1`, the `fp_rate` "
            "example) found nothing in any generated question or in the 200 memories, so no version "
            "drew a `source_finding`.", "",
            "## Cost and size\n",
            "| | measured |", "|---|---|",
            f"| memories | {c['memories']} |",
            f"| generation, wall | {c['wall_seconds']:.0f} s for ten drawn per memory, "
            f"{gen['threads']} CPU threads (median {c['generation_seconds_median']:.2f} s per memory) |",
            f"| tokens in / out | {c['tokens_in']:,} / {c['tokens_out_10_drawn']:,} (ten drawn) |",
            f"| questions kept (arm) | {c['questions_kept']} on {c['memories_with_a_set']} memories |",
            f"| question text | {c['question_bytes']:,} bytes |",
            f"| kind 3 lexemes / kind 1 lexemes | {c['kind3_postings']:,} / {c['kind1_postings']:,} "
            f"= {c['kind3_over_kind1']:.3f} (budget: at most 1) |",
            "",
            "w_q by five-fold cross-validation (per fold; the synthetic queries take the whole grown "
            "set's): " + "; ".join(f"`{p}` {x['per_fold']} / {x['synthetic']}"
                                   for p, x in r["w_q"].items() if "." not in p) + ".", ""]
    print("\n".join(out))


if __name__ == "__main__":
    main()
