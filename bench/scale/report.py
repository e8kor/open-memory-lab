"""Tables, a chart and EXPLAIN excerpts from results/*.json.

    python report.py            # writes results/tables.md and results/p95.svg

REPORT.md quotes these; regenerate after a run.
"""

from __future__ import annotations

import json
import math
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
RES = HERE / "results"
BUDGET = 50.0

RUNS = [("10k", "s10k.json"), ("100k", "s100k.json"), ("1M", "s1m_text.json")]
VEC_RUNS = [("10k", "s10k.json"), ("100k", "s100k.json"), ("1M", "s1m_vector.json")]
MT = "mt10x100k.json"
EXTRA = {"10k": ["s10k_stored.json"], "100k": ["s100k_stored.json", "s100k_halfvec.json"], "1M": []}
NS = {"10k": 1e4, "100k": 1e5, "1M": 1e6}

LABEL = {
    "a_simple_and": "a) today: simple AND + ts_rank",
    "b_simple_or_rank": "b) simple OR + ts_rank",
    "b_simple_or_rank_cd": "b) simple OR + ts_rank_cd",
    "c_dual_or": "c) english+simple stored, OR",
    "d_bm25_querytime": "d) BM25, query-time stats",
    "d_bm25_maintained": "d) BM25, maintained stats",
    "e_trgm_text": "e) pg_trgm on text",
    "e_trgm_ident": "e) pg_trgm on identifiers",
    "a_stored_and": "a') AND over a stored simple tsvector",
    "b_stored_or": "b') OR over a stored simple tsvector",
    "d_bm25_stored": "d') BM25 maintained, stored tsvector",
    "f_hnsw": "f) pgvector HNSW 384-d",
    "a_simple_and@flat": "a) on one unpartitioned table",
    "b_simple_or_rank@flat": "b) on one unpartitioned table",
}
ORDER = list(LABEL)


def load(name):
    p = RES / name
    return json.loads(p.read_text()) if p.exists() else None


def f(x, nd=1):
    if x is None:
        return "–"
    if x >= 1000:
        return f"{x:,.0f}"
    if x >= 100:
        return f"{x:.0f}"
    return f"{x:.{nd}f}"


def mb(b):
    return "–" if b is None else f"{int(b) / 2**20:,.1f}"


def lat(run, appr, phase="warm", key="p95_ms", rls=True):
    if not run:
        return None
    src = run["latency"] if rls else run["latency_no_rls"]
    v = src.get(appr)
    return v[phase][key] if v and phase in v else None


def loglog_cross(points):
    """Fit log(p95) = a + b log(N) over the points; return (b, N at 50 ms)."""
    pts = [(math.log(n), math.log(y)) for n, y in points if y and y > 0]
    if len(pts) < 2:
        return None, None
    mx = sum(p[0] for p in pts) / len(pts)
    my = sum(p[1] for p in pts) / len(pts)
    sxx = sum((p[0] - mx) ** 2 for p in pts)
    if sxx == 0:
        return None, None
    b = sum((p[0] - mx) * (p[1] - my) for p in pts) / sxx
    a = my - b * mx
    if b <= 0:
        return b, None
    return b, math.exp((math.log(BUDGET) - a) / b)


def human_n(n):
    if n is None:
        return "not within the fit"
    if n >= 1e6:
        return f"~{n / 1e6:.1f}M"
    if n >= 1e3:
        return f"~{n / 1e3:.0f}k"
    return f"~{n:.0f}"


def trim_plan(text: str, max_lines: int = 40) -> str:
    text = re.sub(r"'\{[0-9a-f,\-]{60,}\}'", "'{…readable workareas…}'", text)
    text = re.sub(r"'\[[-0-9.,e]{60,}\]'", "'[…384 floats…]'", text)
    text = re.sub(r"[0-9a-f]{32}", "<tenant>", text)
    text = re.sub(r"'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'", "'<uuid>'", text)
    lines = [l for l in text.splitlines() if not l.strip().startswith(("Planning:", "Buffers: shared hit=5", "Index Searches"))]
    return "\n".join(lines[:max_lines])


def svg_chart(series: dict[str, list[tuple[float, float]]]) -> str:
    W, H, L, R, T, B = 720, 420, 60, 230, 20, 40
    xs = [1e4, 1e5, 1e6]
    ymin, ymax = 0.1, 20000.0
    def X(n): return L + (math.log10(n) - 4) / 2 * (W - L - R)
    def Y(v): return T + (1 - (math.log10(max(v, ymin)) - math.log10(ymin)) / (math.log10(ymax) - math.log10(ymin))) * (H - T - B)
    colors = ["#2a6fdb", "#d1495b", "#edae49", "#00798c", "#6a4c93", "#8ac926", "#ff7f11", "#3d405b", "#9e2a2b"]
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
           'font-family="sans-serif" font-size="11">',
           f'<rect width="{W}" height="{H}" fill="white"/>']
    for v in [0.1, 1, 10, 100, 1000, 10000]:
        out.append(f'<line x1="{L}" x2="{W - R}" y1="{Y(v):.1f}" y2="{Y(v):.1f}" stroke="#ddd"/>')
        out.append(f'<text x="{L - 6}" y="{Y(v) + 4:.1f}" text-anchor="end">{v:g} ms</text>')
    for n, lab in zip(xs, ["10k", "100k", "1M"]):
        out.append(f'<line x1="{X(n):.1f}" x2="{X(n):.1f}" y1="{T}" y2="{H - B}" stroke="#ddd"/>')
        out.append(f'<text x="{X(n):.1f}" y="{H - B + 16}" text-anchor="middle">{lab} rows / tenant</text>')
    out.append(f'<line x1="{L}" x2="{W - R}" y1="{Y(BUDGET):.1f}" y2="{Y(BUDGET):.1f}" stroke="#000" stroke-dasharray="5,4"/>')
    out.append(f'<text x="{W - R - 4}" y="{Y(BUDGET) - 4:.1f}" text-anchor="end">50 ms budget</text>')
    for i, (name, pts) in enumerate(series.items()):
        c = colors[i % len(colors)]
        pts = [(n, v) for n, v in pts if v]
        if not pts:
            continue
        d = " ".join(f"{'M' if j == 0 else 'L'}{X(n):.1f},{Y(v):.1f}" for j, (n, v) in enumerate(pts))
        out.append(f'<path d="{d}" fill="none" stroke="{c}" stroke-width="2"/>')
        for n, v in pts:
            out.append(f'<circle cx="{X(n):.1f}" cy="{Y(v):.1f}" r="3" fill="{c}"/>')
        ly = T + 14 + i * 16
        out.append(f'<rect x="{W - R + 10}" y="{ly - 9}" width="12" height="3" fill="{c}"/>')
        out.append(f'<text x="{W - R + 26}" y="{ly - 4}">{name}</text>')
    out.append("</svg>")
    return "\n".join(out)


def main():
    runs = {k: load(v) for k, v in RUNS}
    vruns = {k: load(v) for k, v in VEC_RUNS}
    mt = load(MT)
    md = []

    extras = {k: [x for x in (load(n) for n in v) if x] for k, v in EXTRA.items()}

    def run_for(appr, scale):
        main = (vruns if appr == "f_hnsw" else runs).get(scale)
        if appr == "f_hnsw" or (main and appr in main["latency"]):
            return main
        for x in extras.get(scale, []):
            if appr in x["latency"]:
                return x
        return main

    # 1. headline
    md.append("## Headline: warm p95 latency (ms) as the serving role under row security\n")
    md.append("| approach | 10k | 100k | 1M | 10 tenants × 100k | slope (log-log) | crosses 50 ms p95 at |")
    md.append("|---|---:|---:|---:|---:|---:|---|")
    series, cross = {}, {}
    for appr in ORDER:
        if "@" in appr:
            continue
        vals = [lat(run_for(appr, s), appr) for s in ("10k", "100k", "1M")]
        mtv = lat(mt, appr) if mt else None
        slope, n50 = loglog_cross([(NS[s], v) for s, v in zip(("10k", "100k", "1M"), vals)])
        if vals[0] and vals[0] > BUDGET:
            where = "already at 10k"
        elif all(v is not None and v <= BUDGET for v in vals):
            where = f"not by 1M (fit: {human_n(n50)})" if n50 else "not by 1M"
        else:
            where = human_n(n50)
        cross[appr] = where
        series[LABEL[appr]] = [(NS[s], v) for s, v in zip(("10k", "100k", "1M"), vals)]
        md.append(f"| {LABEL[appr]} | {' | '.join(f(v) for v in vals)} | {f(mtv)} | "
                  f"{'–' if slope is None else f'{slope:.2f}'} | {where} |")
    (RES / "p95.svg").write_text(svg_chart(series))

    md.append("\n## The same statements without row security (superuser, same tenant predicate), warm p95 (ms)\n")
    md.append("What each index could give if its operator were usable under the policy; not a configuration "
              "OML may run (ADR-0034 d13).\n")
    md.append("| approach | 10k | 100k | 1M | 10 tenants × 100k |")
    md.append("|---|---:|---:|---:|---:|")
    for appr in ORDER:
        vals = [lat(run_for(appr, s), appr, rls=False) for s in ("10k", "100k", "1M")]
        if any(vals) or (mt and appr in mt["latency_no_rls"]):
            md.append(f"| {LABEL[appr]} | {' | '.join(f(v) for v in vals)} | "
                      f"{f(lat(mt, appr, rls=False) if mt else None)} |")

    # 2. cold/warm detail
    md.append("\n## Cold and warm latency, p50 / p95 / p99 (ms), under row security\n")
    md.append("Cold: the cluster restarted and the OS page cache of its files and of the loop image evicted "
              "before the pass; the pass itself warms the cache, so it is a cold start, not 330 cold queries.\n")
    for s in ("10k", "100k", "1M"):
        md.append(f"\n### {s} memories per tenant\n")
        md.append("| approach | cold p50 | cold p95 | cold p99 | first query | warm p50 | warm p95 | warm p99 | timeouts | empty results |")
        md.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
        for appr in ORDER:
            r = run_for(appr, s)
            if not r or appr not in r["latency"]:
                continue
            v = r["latency"][appr]
            c, w = v.get("cold", {}), v["warm"]
            md.append(f"| {LABEL[appr]} | {f(c.get('p50_ms'))} | {f(c.get('p95_ms'))} | {f(c.get('p99_ms'))} | "
                      f"{f(c.get('first_ms'))} | {f(w['p50_ms'])} | {f(w['p95_ms'])} | {f(w['p99_ms'])} | "
                      f"{w['timeouts']} | {w['empty']} of {w['n']} |")
    if mt:
        md.append("\n### 10 tenants × 100k in one partitioned table (the measured tenant is one of ten)\n")
        md.append("| approach | warm p50 | warm p95 | warm p99 | no-RLS warm p95 |")
        md.append("|---|---:|---:|---:|---:|")
        for appr in ORDER:
            if appr in mt["latency"]:
                w = mt["latency"][appr]["warm"]
                md.append(f"| {LABEL[appr]} | {f(w['p50_ms'])} | {f(w['p95_ms'])} | {f(w['p99_ms'])} | "
                          f"{f(lat(mt, appr, rls=False))} |")

    # 3. by category at the largest scale
    big = runs.get("1M") or runs.get("100k")
    if big:
        cats = big["workload"]["categories"]
        md.append(f"\n## Warm p95 (ms) by query category at {big['scale']:,} memories\n")
        md.append("| approach | " + " | ".join(cats) + " |")
        md.append("|---|" + "---:|" * len(cats))
        for appr in ORDER:
            r = big if appr != "f_hnsw" else (vruns.get("1M") or big)
            if appr not in r["latency"]:
                continue
            bc = r["latency"][appr]["warm_by_category"]
            md.append(f"| {LABEL[appr]} | " + " | ".join(f(bc.get(c, {}).get("p95_ms")) for c in cats) + " |")

    md.append("\n## Warm p50 / p95 (ms) by readable scope, under row security\n")
    md.append("leaf: the leaf and universe; project: a project, its leaves and universe; universe: every workarea. "
              "From the runs that recorded it (1M, and the 10k/100k supplement for a and c).\n")
    md.append("| approach | scale | leaf | project | universe |")
    md.append("|---|---|---:|---:|---:|")
    for appr in ORDER:
        for sc in ("10k", "100k", "1M"):
            r = run_for(appr, sc)
            if not r or appr not in r["latency"] or "warm_by_scope" not in r["latency"][appr]:
                continue
            bs = r["latency"][appr]["warm_by_scope"]
            md.append(f"| {LABEL[appr]} | {sc} | " + " | ".join(
                f"{f(bs[k]['p50_ms'])} / {f(bs[k]['p95_ms'])}" if k in bs else "–"
                for k in ("leaf", "project", "universe")) + " |")

    # 4. index cost
    md.append("\n## Index build time (s) and size (MiB) per tenant\n")
    md.append("| index | 10k build | 10k size | 100k build | 100k size | 1M build | 1M size |")
    md.append("|---|---:|---:|---:|---:|---:|---:|")
    def idx_row(label, getter):
        cells = []
        for s in ("10k", "100k", "1M"):
            b, z = getter(s)
            cells += [f(b, 2), mb(z)]
        md.append(f"| {label} | " + " | ".join(cells) + " |")
    def load0(s, key):
        r = runs.get(s)
        if not r:
            return None, None
        x = r["load"][0]["index"].get(key)
        return (x["build_s"], x["bytes"]) if x else (None, None)
    def heap(s):
        r = runs.get(s)
        return (r["load"][0]["copy_s"], r["load"][0]["heap_bytes"]) if r else (None, None)
    idx_row("heap (COPY), today's columns", heap)
    for key, lab in [("claim_fts", "today's GIN to_tsvector('simple')"), ("pkey", "primary key"),
                     ("workarea", "(tenant, workarea, status)"), ("served_tip", "served tip (partial unique)"),
                     ("claim_version", "claim version (partial unique)"), ("author", "author")]:
        idx_row(lab, lambda s, key=key: load0(s, key))
    def stg(s, st, b, z):
        r = runs.get(s)
        x = r and r["stages"].get(st)
        return (x.get(b) if b else None, x.get(z)) if x else (None, None)
    idx_row("c) table rewrite adding two stored tsvectors (heap after)", lambda s: stg(s, "dual", "rewrite_s", "heap_bytes_after"))
    idx_row("c) GIN on stored simple tsvector", lambda s: stg(s, "dual", "gin_simple_s", "gin_simple_bytes"))
    idx_row("c) GIN on stored english tsvector", lambda s: stg(s, "dual", "gin_english_s", "gin_english_bytes"))
    idx_row("d) maintained df table + PK (rows below)", lambda s: stg(s, "bm25", "build_s", "df_total_bytes"))
    idx_row("e) GIN gin_trgm_ops on claim", lambda s: stg(s, "trgm", "claim_build_s", "claim_bytes"))
    idx_row("e) GIN gin_trgm_ops on identifiers", lambda s: stg(s, "trgm", "idents_build_s", "idents_bytes"))
    def vec(s, b, z):
        r = vruns.get(s)
        x = r and r["stages"].get("vector")
        return (x.get(b), x.get(z)) if x else (None, None)
    idx_row("f) vector(384) table (COPY)", lambda s: vec(s, "load_s", "heap_bytes"))
    idx_row("f) HNSW m 16, ef_construction 64", lambda s: vec(s, "hnsw_build_s", "hnsw_bytes"))
    dfrows = [str((runs.get(s) or {}).get("stages", {}).get("bm25", {}).get("df_rows", "–")) for s in ("10k", "100k", "1M")]
    md.append(f"\nMaintained df rows (tenant, workarea, term): {', '.join(dfrows)} at 10k, 100k, 1M.")

    # 5. write cost
    md.append("\n## Write cost per memory (ms), serving role, synchronous_commit off\n")
    md.append("Insert = journal row + held memories row in one transaction; promote = the UPDATE that makes it "
              "a served tip. Stages are cumulative except the BM25 trigger, measured on today's indexes and "
              "then dropped; the stage's own cost is its row minus the row it builds on.\n")
    md.append("| stage | 10k insert p50 / p95 | 10k promote p50 / p95 | 100k insert | 100k promote | 1M insert | 1M promote |")
    md.append("|---|---:|---:|---:|---:|---:|---:|")
    for st, lab in [("base", "today's indexes"), ("+bm25_trigger", "+ BM25 df trigger (on today's)"),
                    ("+dual", "c) stored english+simple, 2 GIN (replaces today's GIN)"),
                    ("+trgm", "+ e) two trigram GIN")]:
        cells = []
        for s in ("10k", "100k", "1M"):
            w = (runs.get(s) or {}).get("write", {}).get(st)
            cells += [f"{f(w['insert']['p50_ms'], 2)} / {f(w['insert']['p95_ms'], 2)}" if w else "–",
                      f"{f(w['promote']['p50_ms'], 2)} / {f(w['promote']['p95_ms'], 2)}" if w else "–"]
        md.append(f"| {lab} | " + " | ".join(cells) + " |")
    cells = []
    for s in ("10k", "100k", "1M"):
        w = (vruns.get(s) or {}).get("write", {}).get("vector_insert")
        cells += [f"{f(w['p50_ms'], 2)} / {f(w['p95_ms'], 2)}" if w else "–", "–"]
    md.append("| f) one vector row into the HNSW table | " + " | ".join(cells) + " |")

    # 6. ADR-0050
    md.append("\n## ADR-0050 d1 write-time similar check (ms added per write)\n")
    md.append("Served-tip recall plus own-held recall, any word (simple OR), k 3, the new text as the query, "
              "at a leaf workarea's scope (leaf + universe), under row security.\n")
    md.append("| scale | p50 | p95 | p99 | mean candidates returned |")
    md.append("|---|---:|---:|---:|---:|")
    for s in ("10k", "100k", "1M"):
        r = runs.get(s)
        if r and "adr0050" in r:
            v = r["adr0050"]["base"]
            md.append(f"| {s} | {f(v['p50_ms'])} | {f(v['p95_ms'])} | {f(v['p99_ms'])} | {f(v['mean_rows'])} |")
    if mt and "adr0050" in mt:
        v = mt["adr0050"]["base"]
        md.append(f"| 10 × 100k | {f(v['p50_ms'])} | {f(v['p95_ms'])} | {f(v['p99_ms'])} | {f(v['mean_rows'])} |")

    # 7. BM25 RLS and vector recall
    md.append("\n## BM25 statistics against row security; HNSW recall\n")
    md.append("| scale | terms checked | maintained df = query-time df under RLS | tenant-wide df differs from readable df |")
    md.append("|---|---:|---:|---:|")
    for s in ("10k", "100k", "1M"):
        r = runs.get(s)
        if r and "bm25_rls_check" in r:
            v = r["bm25_rls_check"]
            md.append(f"| {s} | {v['terms']} | {v['maintained_equals_querytime']} | {v['tenant_wide_differs']} |")
    md.append("\n| scale | queries | HNSW recall@10 vs exact (same filter) | queries returning < k rows |")
    md.append("|---|---:|---:|---:|")
    for s in ("10k", "100k", "1M"):
        r = vruns.get(s)
        if r and "vector_recall" in r:
            v = r["vector_recall"]
            md.append(f"| {s} | {v['queries']} | {f(v['recall_at_k_mean'], 3)} | {v['short_results']} |")

    # 8. data shape
    md.append("\n## Data shape\n")
    md.append("| run | rows per tenant | tenants | workareas per tenant | served rows readable at universe | database size (MiB) |")
    md.append("|---|---:|---:|---:|---:|---:|")
    for lab, name in [*RUNS, ("1M (vector run)", "s1m_vector.json"), ("10 × 100k", MT)]:
        r = load(name)
        if r:
            md.append(f"| {lab} | {r['scale']:,} | {r['tenants']} | {r.get('workareas')} | {r.get('served_rows'):,} | {mb(r.get('db_bytes'))} |")

    (RES / "tables.md").write_text("\n".join(md) + "\n")

    # EXPLAIN excerpts
    ex = ["# EXPLAIN (ANALYZE, BUFFERS) excerpts\n",
          "Universe-scope queries (every workarea readable), as the serving role with oml.tenant_id set, "
          "and the same statement as the superuser (`@no_rls`). Long literals are elided.\n"]
    for lab, name in [*RUNS, ("1M vector run", "s1m_vector.json"), ("10 × 100k", MT)]:
        r = load(name)
        if not r:
            continue
        ex.append(f"\n## {lab}\n")
        for appr, plans in r["explain"].items():
            for cat in ("exact", "long"):
                if cat in plans:
                    ex.append(f"\n### {appr} — {cat} query\n\n```\n{trim_plan(plans[cat])}\n```")
    (RES / "explain.md").write_text("\n".join(ex) + "\n")
    print((RES / "tables.md").read_text())


# ---------------------------------------------------------------------------
# The common schema the Pages docs generator renders: one file per approach x
# scale, results/<method>/<rows>.json ("oml-bench/1").
# ---------------------------------------------------------------------------

SUITE_VERSION = "1"
COMMIT = "bb40436db8a2515e2055b81c1be480afcf206763"  # origin/main measured (migrations 0001-0016)
METHOD = {
    "a_simple_and": "keyword-all", "a_stored_and": "keyword-all-stored",
    "b_simple_or_rank": "keyword-any", "b_simple_or_rank_cd": "keyword-any-rank-cd",
    "b_stored_or": "keyword-any-stored", "c_dual_or": "english-simple",
    "d_bm25_querytime": "bm25-sql-querytime", "d_bm25_maintained": "bm25-sql-maintained",
    "d_bm25_stored": "bm25-sql-stored", "e_trgm_text": "trigram", "e_trgm_ident": "trigram-identifiers",
    "f_hnsw": "pgvector-hnsw", "a_simple_and@flat": "keyword-all-unpartitioned",
    "b_simple_or_rank@flat": "keyword-any-unpartitioned",
}
NOTE = ("Synthetic templated text from the recall eval's 200 seeds (and synthetic clustered vectors), "
        "one client on a shared 8-vCPU host, queried as the serving role under row security.")


def r4(x):
    return None if x is None else round(float(x), 4)


def index_cost(run: dict, appr: str):
    """(index MiB, build s, write overhead ms) of the index set an approach adds,
    the write overhead being the stage's insert+promote p50 minus its base."""
    st, w = run.get("stages", {}), run.get("write", {})
    ld = run["load"][0]["index"] if run.get("load") else {}
    def wsum(k):
        x = w.get(k)
        return x["insert"]["p50_ms"] + x["promote"]["p50_ms"] if x else None
    base = wsum("base")
    def diff(k, ref):
        a, b = wsum(k), ref
        return None if a is None or b is None else a - b
    if appr in ("a_simple_and", "b_simple_or_rank", "b_simple_or_rank_cd", "d_bm25_querytime",
                "a_simple_and@flat", "b_simple_or_rank@flat"):
        x = ld.get("claim_fts", {})
        # today's index set: the reference, nothing added
        return x.get("bytes", 0) / 2**20 if x else None, x.get("build_s"), 0.0
    if appr in ("c_dual_or", "a_stored_and", "b_stored_or") and "dual" in st:
        d = st["dual"]
        return ((int(d["gin_simple_bytes"]) + int(d["gin_english_bytes"])) / 2**20,
                d["rewrite_s"] + d["gin_simple_s"] + d["gin_english_s"], diff("+dual", base))
    if appr in ("d_bm25_maintained", "d_bm25_stored") and "bm25" in st:
        b = st["bm25"]
        return ((b["df_total_bytes"] + b["scope_total_bytes"]) / 2**20, b["build_s"],
                diff("+bm25_trigger", base))
    if appr in ("e_trgm_text", "e_trgm_ident") and "trgm" in st:
        t = st["trgm"]
        key = "claim" if appr == "e_trgm_text" else "idents"
        return int(t[f"{key}_bytes"]) / 2**20, t[f"{key}_build_s"], diff("+trgm", wsum("+dual"))
    if appr == "f_hnsw" and "vector" in st:
        v = st["vector"]
        vi = w.get("vector_insert")
        return int(v["hnsw_bytes"]) / 2**20, v["hnsw_build_s"], vi["p50_ms"] if vi else None
    return None, None, None


def export_common():
    files = [(n, load(n)) for n in sorted(p.name for p in RES.glob("*.json"))]
    written = 0
    for name, run in files:
        if not run or "latency" not in run:
            continue
        date = run.get("started", "")[:10]
        pgv = run.get("postgres", "PostgreSQL 18.6")
        for appr, v in run["latency"].items():
            method = METHOD.get(appr)
            if not method:
                continue
            vec = run.get("stages", {}).get("vector", {})
            if appr == "f_hnsw" and vec.get("type") == "halfvec":
                method = "pgvector-hnsw-halfvec"
            imb, ib, wo = index_cost(run, appr)
            rows = str(run["scale"]) + (f"-t{run['tenants']}" if run["tenants"] > 1 else "")
            c, w = v.get("cold", {}), v["warm"]
            doc = {
                "schema": "oml-bench/1", "suite": "scale", "suite_version": SUITE_VERSION, "kind": "scale",
                "method": method, "date": date, "commit": COMMIT,
                "environment": {"host": "8 vCPU / 15 GB", "postgres": pgv, "cache": "warm"},
                "n_docs": run["scale"], "tenants": run["tenants"],
                "metrics": {"index_mb": r4(imb), "index_build_s": r4(ib), "write_overhead_ms": r4(wo),
                            "cold_p50_ms": r4(c.get("p50_ms")), "cold_p95_ms": r4(c.get("p95_ms")),
                            "cold_p99_ms": r4(c.get("p99_ms")), "timeouts": w["timeouts"],
                            "empty_results": w["empty"], "queries": w["n"],
                            "write_total_p50_ms_today": r4((run["write"]["base"]["insert"]["p50_ms"] + run["write"]["base"]["promote"]["p50_ms"]) if "base" in run.get("write", {}) else None),
                            "no_rls_p95_ms": r4(run.get("latency_no_rls", {}).get(appr, {}).get("warm", {}).get("p95_ms"))},
                "latency_ms": {"p50": r4(w["p50_ms"]), "p95": r4(w["p95_ms"]), "p99": r4(w["p99_ms"])},
                "per_category": {k: {"p50": r4(x["p50_ms"]), "p95": r4(x["p95_ms"])}
                                 for k, x in v.get("warm_by_category", {}).items()},
                "extrapolated": False,
                "notes": NOTE + (" Timed-out queries count as the timeout (%d ms)." % run.get("timeout_ms", 10000)
                                 if w["timeouts"] else "") + (" halfvec(384) at this scale." if method.endswith("halfvec") else ""),
                "source": f"bench/scale/results/{name}",
            }
            out = RES / method / f"{rows}.json"
            out.parent.mkdir(exist_ok=True)
            out.write_text(json.dumps(doc, indent=1) + "\n")
            written += 1
        if "adr0050" in run:
            a = run["adr0050"]["base"]
            rows = str(run["scale"]) + (f"-t{run['tenants']}" if run["tenants"] > 1 else "")
            doc = {"schema": "oml-bench/1", "suite": "scale", "suite_version": SUITE_VERSION, "kind": "scale",
                   "method": "similar-check", "date": run.get("started", "")[:10], "commit": COMMIT,
                   "environment": {"host": "8 vCPU / 15 GB", "postgres": "PostgreSQL 18.6", "cache": "warm"},
                   "n_docs": run["scale"], "tenants": run["tenants"],
                   "metrics": {"index_mb": None, "index_build_s": None, "write_overhead_ms": r4(a["p50_ms"]),
                               "mean_candidates": r4(a["mean_rows"]), "writes": a["n"]},
                   "latency_ms": {"p50": r4(a["p50_ms"]), "p95": r4(a["p95_ms"]), "p99": r4(a["p99_ms"])},
                   "per_category": {}, "extrapolated": False,
                   "notes": NOTE + " ADR-0050 d1: served-tip plus own-held any-word recall, k 3, the new text as query.",
                   "source": f"bench/scale/results/{name}"}
            out = RES / "similar-check" / f"{rows}.json"
            out.parent.mkdir(exist_ok=True)
            out.write_text(json.dumps(doc, indent=1) + "\n")
            written += 1
    return written


if __name__ == "__main__":
    main()
    print("common-schema files:", export_common())
