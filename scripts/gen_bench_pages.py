#!/usr/bin/env python3
"""Generate the benchmark pages of the API docs from the committed results.

    python scripts/gen_bench_pages.py          write docs/benchmarks/
    python scripts/gen_bench_pages.py --check  exit 1 when a page differs

Reads every `oml-bench/1` JSON file under eval/bench/results/ and, when the
scale lane has written any, bench/scale/results/. A file under an `archive/`
directory is an earlier run: its numbers are shown beside the current ones,
never in place of them. A file under `offline/` is an experiment no served
path ran (eval/bench/offline/) and is not rendered. Writes docs/benchmarks/index.md (the default
pipeline, one headline table per suite, the runs' dates, commits and
environments), one page per suite, and scale.md when scale results exist.
The pages are generated; the docs CI fails when they differ from what this
script writes (docs-checks.yml). Standard library only.
"""

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import docgen  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SOURCES = [ROOT / "eval" / "bench" / "results", ROOT / "bench" / "scale" / "results"]
OUT = ROOT / "docs" / "benchmarks"
DEFAULT_SOURCE = ROOT / "crates" / "bins" / "oml-server-lead" / "src" / "lib.rs"
SCHEMA = "oml-bench/1"
SERVED = ["keyword-all", "keyword-any", "bm25", "bm25-trigram", "bm25-postings", "bm25-rerank",
          "sparse", "bm25-sparse", "bm25-sparse-rrf"]
SUITE_TITLES = {
    "recall-synthetic": "Synthetic recall set",
    "beir-scifact": "BEIR SciFact",
    "beir-nfcorpus": "BEIR NFCorpus",
    "beir-scifact-x10": "SciFact ×10 (latency at 50k rows)",
    "locomo": "LoCoMo",
}
SUITE_ORDER = ["recall-synthetic", "beir-scifact", "beir-nfcorpus", "locomo", "beir-scifact-x10"]
QUALITY = [("ndcg@10", "nDCG@10"), ("recall@10", "R@10"), ("recall@20", "R@20"),
           ("recall@100", "R@100"),
           ("mrr@10", "RR@10"), ("success@1", "S@1"), ("success@5", "S@5"),
           ("zero_result_rate", "zero")]
LATENCY_LINE_MS = 50


CEILING = ("**Why latency grows with the rows.** Under row security no text index serves a recall, "
           "so every pipeline except `bm25-postings` reads the stored text vectors of every row the key "
           "may read, and its cost grows with those rows. Stored vectors made a row several times "
           "cheaper, not free: an every-word match crossed 50 ms near 70,000 readable rows "
           "(`bench/scale/REPORT-baseline.md`), and `bm25` reads more per row, so on abstract-length "
           "text it crosses sooner, as the tables show. `bm25-postings` reads integer postings, which "
           "row security lets an index serve, so its cost follows the postings it reads: 25.5 ms p95 "
           "at 99,721 readable memories, universe scope, against `bm25`'s 485 ms. That figure was "
           "measured at the database, not through the client, on 2026-11-05 on a host shared with "
           "other jobs, so treat it as an upper bound (`bench/scale/REPORT.md`).\n")


def load_results():
    """(current, earlier): {suite: {method: doc}} and {suite: [doc]}."""
    current, earlier = {}, {}
    for base in SOURCES:
        if not base.exists():
            continue
        for p in sorted(base.rglob("*.json")):
            try:
                doc = json.loads(p.read_text())
            except (json.JSONDecodeError, UnicodeDecodeError):
                continue
            if not isinstance(doc, dict) or doc.get("schema") != SCHEMA:
                continue
            if "offline" in p.relative_to(base).parts:
                continue
            doc["_path"] = p.relative_to(ROOT).as_posix()
            if "archive" in p.relative_to(base).parts:
                earlier.setdefault(doc["suite"], []).append(doc)
            else:
                current.setdefault(doc["suite"], {})[doc["method"]] = doc
    return current, earlier


def default_pipeline():
    m = re.search(r"DEFAULT_SEARCH_PIPELINE: Pipeline = Pipeline::(\w+);",
                  DEFAULT_SOURCE.read_text())
    names = {"KeywordAll": "keyword-all", "KeywordAny": "keyword-any", "Bm25": "bm25",
             "Bm25Trigram": "bm25-trigram", "Bm25Postings": "bm25-postings",
             "Bm25Rerank": "bm25-rerank"}
    return names.get(m.group(1), m.group(1)) if m else "unknown"


def num(v, digits=3):
    return "–" if v is None else f"{v:.{digits}f}"


def ms(v):
    if v is None:
        return "offline"
    return f"{v:,.0f}" if v >= 100 else f"{v:.1f}"


def method_order(methods):
    served = [m for m in SERVED if m in methods]
    return served + sorted(m for m in methods if m not in SERVED)


def suites_in_order(current):
    return [s for s in SUITE_ORDER if s in current] + sorted(
        s for s in current if s not in SUITE_ORDER)


def header(page_id, answers):
    return docgen.front_matter(answers).replace("status: Generated", "status: Measured")


GENERATED = ("*These are measurements by the OML maintainers on their own hardware, not "
             "independent results. Generated from the committed `oml-bench/1` results.*\n")

REPRODUCE = ("**How to reproduce.** Build `oml` and `oml-server-lead` in release mode, install "
             "`eval/bench/requirements.txt` in a scratch virtual environment, then run "
             "`eval/bench/run.sh <out-dir> {dataset}` against a throwaway PostgreSQL superuser "
             "cluster. The harness loads the data over HTTP, promotes it with reviewer keys, "
             "asks every query through `oml_read` and writes the result files listed below.\n")

DEFINITIONS = ("## Metric definitions\n\n"
               "- **nDCG@10**: ranking quality over the top 10 hits, 0 to 1; higher is better.\n"
               "- **R@k**: the share of the relevant memories found in the top k.\n"
               "- **RR@10**: the reciprocal rank of the first relevant hit within 10, averaged over queries.\n"
               "- **S@1, S@5** (success at k): the share of queries with a relevant hit in the top 1 or top 5.\n"
               "- **zero**: the share of queries that returned no hit.\n"
               "- **p50, p95, p99**: the client's wall time per recall in milliseconds, nearest-rank percentiles.\n"
               "\nThese are retrieval metrics. No language model judges any answer, so there is no judge noise.\n")

WHAT_NOT = {
    "recall-synthetic": "One model wrote the corpus, the queries and the labels, and its name was not recorded. "
                        "The scores compare OML pipelines with each other; do not compare them with BEIR or LoCoMo.",
    "beir-scifact": "SciFact is scientific abstracts and claims. It does not predict recall over short chat notes.",
    "beir-nfcorpus": "NFCorpus is nutrition and medical text. It does not predict recall over short chat notes.",
    "locomo": "Dialogue turns are short and English only. The result does not predict recall over long documents.",
    "beir-scifact-x10": "Ten copies of the corpus measure latency, not quality.",
}


def bar_chart(rows, title, reference=None, unit=""):
    """An inline SVG of horizontal bars; currentColor keeps it legible in both themes."""
    rows = [(label, v) for label, v in rows if v is not None]
    if not rows:
        return ""
    top = max([v for _, v in rows] + ([reference[1]] if reference else []))
    top = top if top > 0 else 1
    width, label_w, bar_h, gap = 680, 240, 18, 8
    plot_w = width - label_w - 70
    height = 30 + len(rows) * (bar_h + gap) + (20 if reference else 0)
    out = [f'<svg role="img" aria-label="{title}" viewBox="0 0 {width} {height}" width="100%" '
           f'style="max-width:{width}px" xmlns="http://www.w3.org/2000/svg">',
           f'<title>{title}</title>',
           f'<text x="0" y="16" font-size="13" fill="currentColor">{title}</text>']
    for i, (label, v) in enumerate(rows):
        y = 28 + i * (bar_h + gap)
        w = max(1.0, plot_w * v / top)
        out.append(f'<text x="{label_w - 8}" y="{y + 13}" font-size="11" text-anchor="end" '
                   f'fill="currentColor">{label}</text>')
        out.append(f'<rect x="{label_w}" y="{y}" width="{w:.1f}" height="{bar_h}" '
                   f'fill="currentColor" fill-opacity="0.55"/>')
        value = f"{v:.3f}" if unit == "" else f"{ms(v)} {unit}"
        out.append(f'<text x="{label_w + w + 6:.1f}" y="{y + 13}" font-size="12" '
                   f'fill="currentColor">{value}</text>')
    if reference:
        x = label_w + plot_w * reference[1] / top
        y2 = 28 + len(rows) * (bar_h + gap)
        out.append(f'<line x1="{x:.1f}" y1="24" x2="{x:.1f}" y2="{y2}" stroke="currentColor" '
                   f'stroke-dasharray="4 3"/>')
        out.append(f'<text x="{x:.1f}" y="{y2 + 14}" font-size="11" text-anchor="middle" '
                   f'fill="currentColor">{reference[0]}</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


def fold(title, lines, kind="note"):
    """A collapsed Material admonition: long detail a reader opens on purpose."""
    body = "\n".join(lines).strip("\n").split("\n")
    return "\n".join([f'??? {kind} "{title}"', ""] + [("    " + l) if l else "" for l in body]) + "\n"


def run_line(docs):
    dates = sorted({d["date"].replace("unknown", "date not recorded") for d in docs})
    commits = sorted({d["commit"].replace("unknown", "not recorded") for d in docs})
    envs = sorted({json.dumps(d.get("environment", {}), sort_keys=True) for d in docs})
    env_text = "; ".join(", ".join(f"{k} {v}" for k, v in json.loads(e).items()) for e in envs)
    return (f"Measured {', '.join(dates)} on commits {', '.join(f'`{c}`' for c in commits)}; "
            f"{env_text}.")


def full_doc(docs):
    """The run that asked the most queries, for a suite's counts."""
    return max(docs.values(), key=lambda d: (d["n_queries"], d["method"]))


def asked(doc):
    return f"{doc['n_queries']:,}" + (" (sampled)" if doc.get("sampled") else "")


def headline_row(doc):
    m, lat = doc["metrics"], doc["latency_ms"]
    return (f"| {doc['method']} | {asked(doc)} | {num(m.get('ndcg@10'))} | {num(m.get('success@5'))} | "
            f"{num(m.get('recall@100'))} | {num(m.get('zero_result_rate'))} | "
            f"{ms(lat.get('p50'))} | {ms(lat.get('p95'))} |")


def index_page(current, earlier, has_scale):
    default = default_pipeline()
    lines = [header("benchmarks", "which search pipeline is the default and what each "
                    "pipeline measured on every benchmark suite: quality, latency, the date, "
                    "commit and environment of each run"),
             "# Benchmarks\n",
             f"The default search pipeline is `{default}` (the deployment's `search.default` "
             "when unset; [settings](../operations/configuration.md)). It scores within 0.003 nDCG@10 "
             "of `bm25` on the quality suites and is the one pipeline an index serves under row "
             "security, so its latency grows slowest with the rows; the pipelines that score "
             "higher read every readable row's stored text vectors or add a model stage. The "
             "[benchmark report](https://github.com/e8kor/open-memory-lab/blob/main/eval/bench/REPORT.md) "
             "gives the reasoning.\n",
             GENERATED,
             "## Results at a glance\n",
             f"The default pipeline, `{default}`, on each suite. nDCG@10 and S@5 measure "
             "quality; p95 is the client's wall time per recall.\n",
             "| suite | memories | queries | nDCG@10 | S@5 | p95 ms | run |",
             "|---|---:|---:|---:|---:|---:|---|"]
    for suite in suites_in_order(current):
        docs = current[suite]
        any_doc = full_doc(docs)
        d = docs.get(default)
        title = SUITE_TITLES.get(suite, suite)
        dates = ", ".join(sorted({x["date"].replace("unknown", "not recorded") for x in docs.values()}))
        if d:
            lines.append(f"| [{title}]({'scale' if suite == 'beir-scifact-x10' else suite}.md) | {any_doc['n_docs']:,} | {asked(d)} | "
                         f"{num(d['metrics'].get('ndcg@10'))} | {num(d['metrics'].get('success@5'))} | "
                         f"{ms(d['latency_ms'].get('p95'))} | {dates} |")
        else:
            lines.append(f"| [{title}]({'scale' if suite == 'beir-scifact-x10' else suite}.md) | {any_doc['n_docs']:,} | "
                         f"{any_doc['n_queries']:,} | Not measured yet | Not measured yet | Not measured yet | {dates} |")
    lines.append("")
    lines.append(DEFINITIONS)
    lines.append("## Methodology\n")
    lines.append("Every number went through the served path: memories written over HTTP by an "
                 "agent key, promoted by reviewer keys of other principals, and recalled through "
                 "`oml_read` over `/mcp`, one query at a time. Latency is the client's wall time "
                 "per call, in milliseconds. Reference rows are offline and have no latency. "
                 "Every suite page lists all pipelines, the run's commit and host, and what the result does not show. "
                 "Latency was measured on a host shared with other jobs, so every latency is an upper bound for one client.\n")
    lines.append(fold("The latency ceiling", [CEILING]))
    lines.append("## Suites\n")
    for suite in suites_in_order(current):
        docs = current[suite]
        any_doc = full_doc(docs)
        title = SUITE_TITLES.get(suite, suite)
        lines.append(f"- [{title}]({'scale' if suite == 'beir-scifact-x10' else suite}.md): {any_doc['n_docs']:,} memories, "
                     f"{any_doc['n_queries']:,} queries, {len(docs)} pipelines.")
    lines.append("")
    lines.append(fold("Run details: commits and environments", [
        f"- **{SUITE_TITLES.get(suite, suite)}**: " + run_line(list(current[suite].values()))
        for suite in suites_in_order(current)]))
    if has_scale:
        lines.append("## [Latency at scale](scale.md)\n")
        lines.append(f"Latency against the rows in a recall's scope, with the "
                     f"{LATENCY_LINE_MS} ms line.\n")
    return "\n".join(lines).rstrip() + "\n"


def suite_page(suite, docs, earlier_docs):
    title = SUITE_TITLES.get(suite, suite)
    any_doc = full_doc(docs)
    methods = method_order(docs)
    lines = [header(f"benchmarks/{suite}", f"what each search pipeline measured on "
                    f"{title}: quality, per-category results, latency, the reference numbers "
                    "and the limits of the run"),
             f"# {title}\n", GENERATED,
             f"Suite `{suite}`, data `{any_doc['suite_version']}`: {any_doc['n_docs']:,} "
             f"memories, {any_doc['n_queries']:,} queries; a method marked sampled asked a "
             "seeded subset, named in its row.\n",
             fold("Run details", [run_line(list(docs.values()))]),
             REPRODUCE.format(dataset={"recall-synthetic": "synthetic", "beir-scifact": "scifact",
                                       "beir-nfcorpus": "nfcorpus", "locomo": "locomo"}.get(suite, suite)),
             "[All suites](index.md)\n", "## Quality\n",
             "| method | queries | " + " | ".join(h for _, h in QUALITY) + " |",
             "|---|---:|" + "---:|" * len(QUALITY)]
    for method in methods:
        m = docs[method]["metrics"]
        lines.append(f"| {method} | {asked(docs[method])} | "
                     + " | ".join(num(m.get(k)) for k, _ in QUALITY) + " |")
    ref = any_doc.get("reference")
    if ref:
        lines.append("")
        lines.append(f"Reference: {ref['source']}, {ref['metric']} {num(ref['value'])} "
                     f"([source]({ref['url']})).")
    lines.append("")
    chart = bar_chart([(m, docs[m]["metrics"].get("ndcg@10")) for m in methods],
                      f"nDCG@10 on {title}",
                      (f"published {num(ref['value'])}", ref["value"])
                      if ref and ref.get("metric") == "ndcg@10" else None)
    if chart:
        lines.append(fold("Chart: nDCG@10 by pipeline", [chart]))
    cats = sorted({c for d in docs.values() for c in d.get("per_category", {})})
    if cats:
        lines.append("## By category\n")
        cat = ["nDCG@10 / S@5 for each category, with its query count.\n",
               "| category | n | " + " | ".join(methods) + " |",
               "|---|---:|" + "---:|" * len(methods)]
        for c in cats:
            n = next((d["per_category"][c].get("n_queries") for d in docs.values()
                      if c in d.get("per_category", {})), None)
            cells = []
            for method in methods:
                row = docs[method].get("per_category", {}).get(c)
                cells.append("–" if row is None else
                             f"{num(row.get('ndcg@10'), 2)} / {num(row.get('success@5'), 2)}")
            cat.append(f"| {c} | {n if n is not None else '–'} | " + " | ".join(cells) + " |")
        lines.append(fold("Results by category", cat))
    lines.append("## Latency\n")
    lines.append("Client wall time per recall, in milliseconds, nearest-rank percentiles.\n")
    lines.append("| method | p50 | p95 | p99 |")
    lines.append("|---|---:|---:|---:|")
    served = [m for m in methods if docs[m]["latency_ms"].get("p50") is not None]
    for method in methods:
        lat = docs[method]["latency_ms"]
        lines.append(f"| {method} | {ms(lat.get('p50'))} | {ms(lat.get('p95'))} | "
                     f"{ms(lat.get('p99'))} |")
    lines.append("")
    chart = bar_chart([(m, docs[m]["latency_ms"].get("p95")) for m in served],
                      f"p95 latency on {title}", None, "ms")
    if chart:
        lines.append(fold("Chart: p95 latency by pipeline", [chart]))
    lines.append("## What this result does not show\n")
    lines.append(WHAT_NOT.get(suite, "Not measured yet.") + "\n")
    lines.append("## Limits of this run\n")
    notes = sorted({docgen.strip_handles(d.get("notes", "")) for d in docs.values()} - {""})
    split = [re.split(r"(?<=[.;])\s+(?=[A-Z])", n) for n in notes]
    count = {}
    for s in split:
        for sentence in set(s):
            count[sentence] = count.get(sentence, 0) + 1
    shared = [x for x in dict.fromkeys(x for s in split for x in s) if count[x] > 1]
    if shared:
        lines.append(" ".join(shared) + "\n")
    for s in split:
        rest = " ".join(x for x in s if count[x] == 1)
        if rest:
            lines.append(f"- {rest}")
    lines.append("")
    lines.append("Sources: " + ", ".join(f"`{docs[m]['_path']}`" for m in methods) + ".")
    return "\n".join(lines).rstrip() + "\n"


def scale_page(current):
    """Latency against scope rows, from the suites whose rows differ."""
    points = {}
    for suite, docs in current.items():
        for method, d in docs.items():
            if d["latency_ms"].get("p50") is None:
                continue
            rows = d.get("rows", d["n_docs"])
            points.setdefault(method, []).append((rows, d["latency_ms"]["p50"],
                                                  d["latency_ms"]["p95"], suite))
    lines = [header("benchmarks/scale", "how recall latency grows with the rows in a "
                    "recall's scope, per search pipeline, against the 50 ms line"),
             "# Latency at scale\n", GENERATED,
             "[All suites](index.md)\n",
             "The ten-fold SciFact corpus (about 50,000 memories) is the largest suite. "
             "The default pipeline, `bm25-postings`, has no row at that size yet: Not measured yet.\n",
             "Each point is one suite's run: the rows in the recall's scope (for LoCoMo, all "
             "conversations together, though each query reads one) and the client's p50 and "
             f"p95 in milliseconds. The line is {LATENCY_LINE_MS} ms.\n", CEILING,
             "| method | rows | suite | p50 ms | p95 ms | p95 under 50 ms |",
             "|---|---:|---|---:|---:|---|"]
    for method in method_order(points):
        for rows, p50, p95, suite in sorted(points[method]):
            lines.append(f"| {method} | {rows:,} | {suite} | {ms(p50)} | {ms(p95)} | "
                         f"{'yes' if p95 is not None and p95 <= LATENCY_LINE_MS else 'no'} |")
    lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def pages():
    current, earlier = load_results()
    scale_suites = {s for s, docs in current.items()
                    if any(d.get("kind") == "scale" for d in docs.values())}
    has_scale = bool(scale_suites) or "beir-scifact-x10" in current
    out = {"index.md": index_page(current, earlier, has_scale)}
    for suite in suites_in_order(current):
        if suite == "beir-scifact-x10":
            continue
        out[f"{suite}.md"] = suite_page(suite, current[suite], earlier.get(suite, []))
    if has_scale:
        out["scale.md"] = scale_page(current)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true", help="fail when a page differs")
    a = ap.parse_args()
    want = pages()
    if a.check:
        have = {p.name: p.read_text() for p in OUT.glob("*.md")} if OUT.exists() else {}
        stale = sorted(n for n in set(want) | set(have) if want.get(n) != have.get(n))
        if stale:
            print("benchmark pages differ from scripts/gen_bench_pages.py's output: "
                  + ", ".join(stale) + "; run python scripts/gen_bench_pages.py")
            return 1
        print(f"benchmark pages: {len(want)} pages agree with the results")
        return 0
    OUT.mkdir(parents=True, exist_ok=True)
    for p in OUT.glob("*.md"):
        if p.name not in want:
            p.unlink()
    for name, text in want.items():
        (OUT / name).write_text(text)
    print(f"wrote {len(want)} pages to {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
