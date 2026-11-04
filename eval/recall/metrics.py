"""Ranking metrics for the recall eval. Standard library only.

Every function takes `ranked`, the served memory ids in rank order, and
`rel`, the query's labels {memory_id: grade} with grade 2 (answers it) or
1 (related). Unlabelled ids count as grade 0.
"""

import json
import math
import statistics
from collections import OrderedDict
from pathlib import Path

HERE = Path(__file__).resolve().parent
CATEGORIES = [
    "exact", "paraphrase", "inflection", "partial", "typo", "identifier",
    "question", "long", "negation", "temporal", "multihop",
]


def load():
    corpus = json.loads((HERE / "corpus.json").read_text())
    queries = json.loads((HERE / "queries.json").read_text())
    return corpus["memories"], queries["queries"]


def success_at(ranked, rel, n):
    return 1.0 if any(rel.get(m) == 2 for m in ranked[:n]) else 0.0


def recall_at(ranked, rel, n):
    """Share of the labelled (grade >= 1) memories found in the top n."""
    want = [m for m, g in rel.items() if g >= 1]
    return sum(1 for m in ranked[:n] if m in rel) / len(want)


def mrr(ranked, rel):
    """Reciprocal rank of the first grade-2 memory, 0 when none is served."""
    for i, m in enumerate(ranked):
        if rel.get(m) == 2:
            return 1.0 / (i + 1)
    return 0.0


def ndcg_at(ranked, rel, n):
    dcg = sum((2 ** rel.get(m, 0) - 1) / math.log2(i + 2) for i, m in enumerate(ranked[:n]))
    ideal = sorted(rel.values(), reverse=True)[:n]
    idcg = sum((2 ** g - 1) / math.log2(i + 2) for i, g in enumerate(ideal))
    return dcg / idcg


def score_query(ranked, rel):
    return {
        "s1": success_at(ranked, rel, 1),
        "s5": success_at(ranked, rel, 5),
        "r10": recall_at(ranked, rel, 10),
        "mrr": mrr(ranked, rel),
        "ndcg10": ndcg_at(ranked, rel, 10),
        "zero": 1.0 if not ranked else 0.0,
    }


METRICS = ["s1", "s5", "r10", "mrr", "ndcg10", "zero"]


def aggregate(per_query, queries):
    """per_query: {qid: {"ranked": [...], "latency_ms": float?}} -> summary."""
    cats = OrderedDict((c, []) for c in CATEGORIES)
    cats["overall"] = []
    for q in queries:
        row = per_query[q["id"]]
        s = score_query(row["ranked"], q["relevant"])
        row["scores"] = s
        cats[q["category"]].append((s, row.get("latency_ms")))
        cats["overall"].append((s, row.get("latency_ms")))
    out = OrderedDict()
    for c, rows in cats.items():
        if not rows:
            continue
        agg = {m: round(sum(s[m] for s, _ in rows) / len(rows), 3) for m in METRICS}
        lat = [l for _, l in rows if l is not None]
        agg["median_latency_ms"] = round(statistics.median(lat), 1) if lat else None
        agg["n"] = len(rows)
        out[c] = agg
    return out


def markdown_table(summaries, metric):
    """One row per category, one column per method, for one metric."""
    methods = list(summaries)
    head = "| category | n | " + " | ".join(methods) + " |"
    sep = "|---|---:|" + "---:|" * len(methods)
    lines = [head, sep]
    for c in CATEGORIES + ["overall"]:
        n = summaries[methods[0]][c]["n"]
        cells = [f"{summaries[m][c][metric]:.2f}" for m in methods]
        name = f"**{c}**" if c == "overall" else c
        lines.append(f"| {name} | {n} | " + " | ".join(cells) + " |")
    return "\n".join(lines)


def overall_table(summaries):
    lines = [
        "| method | success@1 | success@5 | recall@10 | MRR | nDCG@10 | zero-result | median latency (ms) |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for m, s in summaries.items():
        o = s["overall"]
        lat = "n/a" if o["median_latency_ms"] is None else f"{o['median_latency_ms']:.1f}"
        lines.append(
            f"| {m} | {o['s1']:.2f} | {o['s5']:.2f} | {o['r10']:.2f} | {o['mrr']:.2f} | "
            f"{o['ndcg10']:.2f} | {o['zero']:.2f} | {lat} |"
        )
    return "\n".join(lines)
