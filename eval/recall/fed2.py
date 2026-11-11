#!/usr/bin/env python3
"""The phase 2 gate of federated recall (ADR-0072 d4.5, d11, d13; #206). Standard library only.

    python3 eval/recall/fed2.py collect --url URL --state DIR --out DIR --scifact-zip PATH \
        --metrics-url URL
    python3 eval/recall/fed2.py analyze --out DIR

`collect` runs under federated.sh (EVAL_SCRIPT=fed2.py): it loads the corpus as federated.py
does, one committed workarea per topic under `eval-fed`, and reads from the served lead, for
every query of the scoped, cross and unanswerable splits and again after the skew padding:

- the union's ranking, `sources: "wide"` at `max_sources` 64, `k` 100, on `bm25-postings`
  (the default pipeline) and on `sparse`, with each hit's score and each candidate's E(s);
- the served `wide` answer at the split's cap on `bm25-postings`, and the served `bm25-sparse`
  `scope` answer with its two legs read at `scope`, so `analyze` can check its replay of both;
- the descriptor score of every candidate: each topic's descriptor (`descriptors.json`) is one
  memory in its own committed workarea under `eval-desc`, and a `bm25` recall whose `sources`
  lists the candidates' descriptor workareas alone scores them with BM25 over the candidates'
  descriptors, the reference set d4.5 names; once more for each topic in turn with its
  descriptor replaced by the adversarial one, whose text holds every word of every query.

Writes <out>/fed2_raw.json. `analyze` replays selection from it without a server: under one
list (`bm25-postings`) a searched set's answer is the union's ranking restricted to it (d7.2,
which test 6 holds), and under `bm25-sparse` the two legs are each read over R and fused by
the convex combination after restriction would move their bounds, so the replay fuses over R
and restricts after fusion, the reading d7.2 names for `Cc` ("until its bounds are taken over
R"). It writes <out>/fed2.json and prints the gate.
"""

import argparse
import json
import math
import os
import random
import subprocess
import sys
import time
from collections import Counter, OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import metrics  # noqa: E402
from federated import (SKEW_FACTOR, SKEW_MAX_SOURCES, SKEW_TOPIC, cross_scope, oml,  # noqa: E402
                       scifact, workarea, write_all)
from run import Client  # noqa: E402

HERE = Path(__file__).resolve().parent
DEPTH = 100          # the union's depth read per leg; CC_DEPTH (oml-app search.rs)
# include_text false: no token budget applies, so the replay cuts by k alone
K = 10
CROSS_MAX_SOURCES = 8  # federated.py's wide@8: 10 candidates, the estimate chooses
CC_WEIGHTS = (0.3, 0.7)  # CC_BM25_SPARSE: bm25-postings, then sparse
SLOTS = 2            # descriptor_slots' default (d4.5)
ALPHAS = [round(i / 10, 1) for i in range(11)]
SEED = 20261111
BOOT = 10000
DESC_PARENT = "eval-desc"


def desc_wa(topic):
    return f"desc-{topic}"


def recall(client, body):
    t0 = time.perf_counter()
    status, resp = client.call("POST", "/v1/memories/recall", body)
    ms = (time.perf_counter() - t0) * 1000
    if status != 200:
        raise SystemExit(f"recall {body}: {status} {resp}")
    return resp, round(ms, 2)


def hits(resp, idmap):
    return [[idmap.get(h["memory"]["artifact_id"], "?"), h["memory"]["artifact_id"], h["score"],
             h["memory"]["workarea_path"].rsplit(".", 1)[-1]] for h in resp["hits"]]


def sources_of(resp):
    s = resp["search"].get("sources") or {}
    return [{"wa": x["workarea_path"].rsplit(".", 1)[-1], "via": x["via"],
             "estimate": x["estimate"]} for x in s.get("searched", [])]


def issue(name, role, held_by, grants, state):
    """A key whose value goes straight to an owner-only file, never printed."""
    args = ["keys", "issue", name, "--role", role, "--held-by", held_by,
            "--principal", f"crew/{name}", "--default-workarea", DESC_PARENT]
    for g in grants:
        args += ["--grant", g]
    out = oml(*args)
    path = state / f"{name}.key"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(out["value"])
    return Client(os.environ["EVAL_URL"], out["value"])


def wait_sparse(metrics_url, written):
    subprocess.run([str(HERE.parent / "bench" / "wait_sparse.sh"), metrics_url, str(written)],
                   check=True, stdout=subprocess.DEVNULL)


def adversarial_text(queries):
    words = []
    seen = set()
    for q in queries:
        for w in q["query"].replace("?", " ").replace(",", " ").split():
            if w.lower() not in seen:
                seen.add(w.lower())
                words.append(w)
    return " ".join(words)


def load_descriptors(state, url, queries):
    """One committed workarea per descriptor under eval-desc, the adversary's beside them."""
    oml("workareas", "create", "universe", DESC_PARENT)
    os.environ["EVAL_URL"] = url
    writer = issue("desc-writer", "writer", "agent",
                   [f"{DESC_PARENT}:writer", f"{DESC_PARENT}:reader"], state)
    reviewer = issue("desc-reviewer", "dreamer", "human",
                     [f"{DESC_PARENT}:dreamer", f"{DESC_PARENT}:reader"], state)
    reader = issue("desc-reader", "reader", "agent", [f"{DESC_PARENT}:reader"], state)
    desc = json.loads((HERE / "descriptors.json").read_text())["descriptors"]
    texts = {t: d["summary"] + " " + " ".join(d["questions"]) for t, d in desc.items()}
    texts["adv"] = adversarial_text(queries)
    for t in texts:
        oml("workareas", "create", DESC_PARENT, desc_wa(t))
    idmap = write_all(writer, reviewer, [(f"desc-{t}", x, desc_wa(t)) for t, x in texts.items()])
    rkey = (state / "desc-reviewer.key").read_text().strip()
    for t in texts:
        tid = oml("workareas", "status", "propose", desc_wa(t), "--to", "committed")["transition_id"]
        subprocess.run([os.environ["OML"], "--url", url, "--api-key", rkey, "--json", "workareas",
                        "status", "ratify", desc_wa(t), tid], check=True, capture_output=True)
    return reader, idmap, {t: len(x) for t, x in texts.items()}


def descriptor_scores(reader, didmap, query, candidates, swap=None):
    """{topic: BM25 score} over the candidates' descriptors alone; swap = (topic, 'adv')."""
    names = {t: (desc_wa("adv") if swap and t == swap else desc_wa(t)) for t in candidates}
    body = {"query": query, "k": len(names), "workarea": DESC_PARENT, "search": "bm25",
            "include_text": False, "sources": sorted(names.values())}
    resp, _ = recall(reader, body)
    by_wa = {h["memory"]["workarea_path"].rsplit(".", 1)[-1]: h["score"] for h in resp["hits"]}
    return {t: by_wa.get(w, 0.0) for t, w in names.items()}


def collect(a):
    state = Path(a.state)
    key = lambda n: (state / n).read_text().strip()  # noqa: E731
    writer, reviewer, reader = (Client(a.url, key(f"{n}.key")) for n in ("writer", "reviewer", "reader"))
    memories, queries = metrics.load()
    topic_of = {m["id"]: m["topic"] for m in memories}
    topics = sorted({m["topic"] for m in memories})
    idmap = write_all(writer, reviewer, [(m["id"], m["text"], workarea(m["topic"])) for m in memories])
    wait_sparse(a.metrics_url, len(idmap))
    dreader, didmap, dlens = load_descriptors(state, a.url, queries)

    answerable = [q for q in queries if q["category"] != metrics.UNANSWERABLE]
    unanswerable = [q for q in queries if q["category"] == metrics.UNANSWERABLE]
    splits = OrderedDict([
        ("scoped", [(q, topic_of[sorted(m for m, g in q["relevant"].items() if g == 2)[0]])
                    for q in answerable]),
        ("cross", [(q, t) for q in answerable if (t := cross_scope(q, topic_of, topics))]),
        ("unanswerable", [(q, topics[i % len(topics)]) for i, q in enumerate(unanswerable)]),
    ])

    def base(q, scope, search, k, sources):
        b = {"query": q["query"], "k": k, "workarea": workarea(scope), "search": search,
             "include_text": False}
        if sources:
            b["sources"] = sources
        return b

    def read_split(items, cap, skew):
        rows = []
        for q, scope in items:
            row = {"id": q["id"], "category": q["category"], "scope": scope}
            oml("search", "federation", "--max-sources", "64")
            for leg in ("bm25-postings", "sparse"):
                resp, ms = recall(reader, base(q, scope, leg, DEPTH, "wide"))
                row[f"union/{leg}"] = hits(resp, idmap)
                row[f"union/{leg}/ms"] = ms
                if leg == "bm25-postings":
                    row["candidates"] = [s for s in sources_of(resp) if s["via"] == "federated"]
            if not skew:
                for leg in ("bm25-postings", "sparse"):
                    resp, _ = recall(reader, base(q, scope, leg, DEPTH, None))
                    row[f"scope/{leg}"] = hits(resp, idmap)
                resp, _ = recall(reader, base(q, scope, "bm25-sparse", K, None))
                row["served/scope/bm25-sparse"] = hits(resp, idmap)
            rows.append(row)
        # The served wide answer at the cap, for the replay's check.
        oml("search", "federation", "--max-sources", str(cap))
        for (q, scope), row in zip(items, rows):
            resp, ms = recall(reader, base(q, scope, "bm25-postings", K, "wide"))
            row["served/wide/bm25-postings"] = hits(resp, idmap)
            row["served/wide/searched"] = [s["wa"] for s in sources_of(resp) if s["via"] == "federated"]
            row["served/wide/ms"] = ms
        oml("search", "federation", "--max-sources", "8")
        return rows

    raw = OrderedDict([("topics", topics), ("descriptor_chars", dlens), ("splits", OrderedDict())])
    recall(reader, base(answerable[0], topics[0], "bm25-postings", K, None))
    for name, items in splits.items():
        raw["splits"][name] = read_split(items, CROSS_MAX_SOURCES, False)

    # Descriptor scores per (query, scope): they read eval-desc alone, so the padding cannot move them.
    pairs = {(q["id"], s): q["query"] for items in splits.values() for q, s in items}
    oml("search", "federation", "--max-sources", "64")  # a list of ten descriptor workareas
    desc = {}
    for (qid, scope), text in pairs.items():
        cands = [t for t in topics if t != scope]
        entry = {"plain": descriptor_scores(dreader, didmap, text, cands)}
        if qid in {q["id"] for q in answerable}:
            entry["adversary"] = {t: descriptor_scores(dreader, didmap, text, cands, swap=t)
                                  for t in cands}
        desc[f"{qid}@{scope}"] = entry
    raw["descriptor_scores"] = desc
    oml("search", "federation", "--max-sources", "8")

    # The skew split: `database` padded to 100 times the largest other topic.
    sizes = Counter(m["topic"] for m in memories)
    pad = SKEW_FACTOR * max(c for t, c in sizes.items() if t != SKEW_TOPIC)
    idmap.update(write_all(writer, reviewer,
                           [(k, t, workarea(SKEW_TOPIC)) for k, t in scifact(a.scifact_zip, pad)]))
    wait_sparse(a.metrics_url, len(idmap) + len(didmap))
    raw["skew"] = {"padded_topic": SKEW_TOPIC, "padded_rows": pad, "max_sources": SKEW_MAX_SOURCES}
    raw["splits"]["skew"] = read_split(splits["cross"], SKEW_MAX_SOURCES, True)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "fed2_raw.json").write_text(json.dumps(raw, indent=0))
    print(f"wrote {out / 'fed2_raw.json'}", file=sys.stderr)


# --- the replay, offline -------------------------------------------------------------------


def content_order(cands):
    """d4.4: E descending, then workarea name ascending."""
    return [c["wa"].removeprefix("fed-") for c in sorted(cands, key=lambda c: (-c["estimate"], c["wa"]))]


def select_content(cands, cap):
    return content_order(cands)[:cap]


def select_described(cands, cap, dscore, alpha, slots=SLOTS):
    """d4.5: the last `slots` of `cap` go to the highest descriptor scores outside the content
    order's first cap - slots, at least alpha x the query's top descriptor score and above 0,
    among candidates with E > 0; a slot no descriptor fills goes back to the content order."""
    order = content_order(cands)
    e = {c["wa"].removeprefix("fed-"): c["estimate"] for c in cands}
    head = order[:cap - slots]
    top = max(dscore.values(), default=0.0)
    eligible = sorted((t for t in order[cap - slots:]
                       if e[t] > 0 and dscore.get(t, 0) > 0 and dscore[t] >= alpha * top),
                      key=lambda t: (-dscore[t], t))[:slots]
    rest = [t for t in order[cap - slots:] if t not in eligible]
    return head + eligible + rest[:slots - len(eligible)]


def select_oracle(cands, cap, holding, slots=SLOTS):
    """The ceiling of any descriptor selector: its slots go to sources holding an answer."""
    order = content_order(cands)
    head = order[:cap - slots]
    picks = [t for t in order[cap - slots:] if t in holding][:slots]
    rest = [t for t in order[cap - slots:] if t not in picks]
    return head + picks + rest[:slots - len(picks)]


def select_redde(cands, cap, union):
    """The exact ReDDE analogue: the union's top 100 counted per federated source, the rest by E."""
    fed = {c["wa"].removeprefix("fed-") for c in cands}
    counts = Counter(h[3].removeprefix("fed-") for h in union if h[3].removeprefix("fed-") in fed)
    first = [t for t, _ in sorted(counts.items(), key=lambda x: (-x[1], x[0]))][:cap]
    return first + [t for t in content_order(cands) if t not in first][:cap - len(first)]


def fuse_cc(bm25, sparse):
    """oml-app's fuse() under Cc: each leg min-max normalised over its own list, weighted, summed;
    score descending, then artifact id ascending."""
    out = {}
    for w, leg in zip(CC_WEIGHTS, (bm25, sparse)):
        if not leg:
            continue
        lo, hi = min(h[2] for h in leg), max(h[2] for h in leg)
        for h in leg:
            n = (h[2] - lo) / (hi - lo) if hi > lo else 1.0
            cur = out.setdefault(h[1], [h[0], h[1], 0.0, h[3]])
            cur[2] += w * n
    return sorted(out.values(), key=lambda h: (-h[2], h[1]))


def ranking(row, pipeline):
    if pipeline == "bm25-postings":
        return row["union/bm25-postings"]
    return fuse_cc(row["union/bm25-postings"], row["union/sparse"])


def answer(rank, scope, searched):
    allowed = {workarea(scope)} | {workarea(t) for t in searched}
    return [h[0] for h in rank if h[3] in allowed][:K]


def holding_of(q, topic_of):
    return {topic_of[m] for m, g in q["relevant"].items() if g == 2}


def per_query(rows, queries_by_id, topic_of, pipeline, cap, chooser):
    out = []
    for r in rows:
        q = queries_by_id[r["id"]]
        searched = chooser(r, q)
        ranked = answer(ranking(r, pipeline), r["scope"], searched)
        rec = {"id": r["id"], "category": r["category"], "searched": searched, "ranked": ranked}
        if q["category"] != metrics.UNANSWERABLE:
            hold = holding_of(q, topic_of) - {r["scope"]}
            rec["s5"] = metrics.success_at(ranked, q["relevant"], 5)
            rec["ndcg"] = metrics.ndcg_at(ranked, q["relevant"], 10)
            sel = set(searched)
            rec["sel_p"] = len(sel & hold) / len(sel) if sel else None
            rec["sel_r"] = len(sel & hold) / len(hold) if hold else None
        else:
            rec["false_hit"] = 1 if ranked else 0
        out.append(rec)
    return out


def mean(v):
    v = [x for x in v if x is not None]
    return round(sum(v) / len(v), 4) if v else None


def summary(recs):
    ans = [r for r in recs if "s5" in r]
    cats = OrderedDict()
    for r in ans:
        c = cats.setdefault(r["category"], [0, 0])
        c[0] += int(r["s5"])
        c[1] += 1
    un = [r["false_hit"] for r in recs if "false_hit" in r]
    return OrderedDict([
        ("n", len(ans)), ("success@5", mean([r["s5"] for r in ans])),
        ("hits@5", int(sum(r["s5"] for r in ans))),
        ("ndcg@10", mean([r["ndcg"] for r in ans])),
        ("selection_precision", mean([r["sel_p"] for r in ans])),
        ("selection_recall", mean([r["sel_r"] for r in ans])),
        ("unanswerable_false_hits", mean(un) if un else None),
        ("success@5_by_category", {c: f"{h}/{n}" for c, (h, n) in cats.items()}),
    ])


def paired(a_recs, b_recs, key="s5"):
    """b - a over paired queries: the delta in points, a percentile bootstrap 95% interval
    (BOOT resamples, fixed seed) and the discordant counts."""
    d = [b[key] - a[key] for a, b in zip(a_recs, b_recs) if key in a]
    if not d:
        return None
    rng = random.Random(SEED)
    n = len(d)
    boots = sorted(sum(d[rng.randrange(n)] for _ in range(n)) / n for _ in range(BOOT))
    return OrderedDict([
        ("n", n), ("delta_points", round(100 * sum(d) / n, 2)),
        ("ci95_points", [round(100 * boots[int(0.025 * BOOT)], 2), round(100 * boots[int(0.975 * BOOT) - 1], 2)]),
        ("gained", sum(1 for x in d if x > 0)), ("lost", sum(1 for x in d if x < 0)),
    ])


def by_category_delta(a_recs, b_recs):
    out = OrderedDict()
    for a, b in zip(a_recs, b_recs):
        if "s5" not in a:
            continue
        c = out.setdefault(a["category"], [0, 0, 0])
        c[0] += int(b["s5"] > a["s5"])
        c[1] += int(b["s5"] < a["s5"])
        c[2] += 1
    return {c: f"+{g} -{l} of {n}" for c, (g, l, n) in out.items()}


def check_replay(rows, topic_of, cap):
    """The replay against the served answer: the searched set and the top 10 on bm25-postings
    (wide at the cap), and on the scoped rows the bm25-sparse scope answer from its two legs."""
    sel_ok = sum(select_content(r["candidates"], cap) and
                 sorted(select_content(r["candidates"], cap)) == sorted(t.removeprefix("fed-") for t in r["served/wide/searched"])
                 for r in rows)
    top_ok = sum(answer(r["union/bm25-postings"], r["scope"], select_content(r["candidates"], cap))
                 == [h[0] for h in r["served/wide/bm25-postings"]] for r in rows)
    out = {"rows": len(rows), "searched_set_equal": sel_ok, "top10_equal": top_ok}
    sc = [r for r in rows if "served/scope/bm25-sparse" in r]
    if sc:
        out["bm25_sparse_scope_top10_equal"] = sum(
            [h[0] for h in fuse_cc(r["scope/bm25-postings"], r["scope/sparse"])][:K]
            == [h[0] for h in r["served/scope/bm25-sparse"]] for r in sc)
        out["bm25_sparse_scope_rows"] = len(sc)
    return out


def analyze(a):
    out = Path(a.out)
    raw = json.loads((out / "fed2_raw.json").read_text())
    memories, queries = metrics.load()
    qbid = {q["id"]: q for q in queries}
    topic_of = {m["id"]: m["topic"] for m in memories}
    desc = raw["descriptor_scores"]
    caps = {"scoped": CROSS_MAX_SOURCES, "cross": CROSS_MAX_SOURCES,
            "unanswerable": CROSS_MAX_SOURCES, "skew": raw["skew"]["max_sources"]}
    report = OrderedDict()
    report["replay_check"] = {s: check_replay(rows, topic_of, caps[s]) for s, rows in raw["splits"].items()}

    def dsc(r, swap=None):
        e = desc[f"{r['id']}@{r['scope']}"]
        return e["adversary"][swap] if swap else e["plain"]

    # alpha: chosen on one random half of the skew split by selection recall, on bm25-postings;
    # ties go to the largest alpha, the fewest descriptor fills.
    skew_ids = sorted(r["id"] for r in raw["splits"]["skew"])
    rng = random.Random(SEED)
    half = set(rng.sample(skew_ids, len(skew_ids) // 2))
    tune = [r for r in raw["splits"]["skew"] if r["id"] in half]
    held = [r for r in raw["splits"]["skew"] if r["id"] not in half]
    cap_s = caps["skew"]

    def sel_recall(rows, alpha):
        recs = per_query(rows, qbid, topic_of, "bm25-postings", cap_s,
                         lambda r, q: select_described(r["candidates"], cap_s, dsc(r), alpha))
        return summary(recs)["selection_recall"]

    sweep = {str(al): sel_recall(tune, al) for al in ALPHAS}
    best = max(ALPHAS, key=lambda al: (sweep[str(al)], al))
    report["alpha"] = OrderedDict([
        ("tuned_on", f"{len(tune)} skew queries, seed {SEED}"), ("sweep_selection_recall", sweep),
        ("chosen", best), ("held_out_selection_recall", sel_recall(held, best)),
        ("held_out_content_selection_recall", summary(per_query(
            held, qbid, topic_of, "bm25-postings", cap_s,
            lambda r, q: select_content(r["candidates"], cap_s)))["selection_recall"]),
    ])

    selectors = OrderedDict([
        ("content", lambda cap: lambda r, q: select_content(r["candidates"], cap)),
        ("described", lambda cap: lambda r, q: select_described(r["candidates"], cap, dsc(r), best)),
        ("oracle_slots", lambda cap: lambda r, q: select_oracle(r["candidates"], cap, holding_of(q, topic_of) if "relevant" in q else set())),
        ("oracle_all", lambda cap: lambda r, q: select_oracle(r["candidates"], cap, holding_of(q, topic_of) if "relevant" in q else set(), slots=cap)),
        ("redde_analogue", lambda cap: lambda r, q: select_redde(r["candidates"], cap, r["union/bm25-postings"])),
        ("union", lambda cap: lambda r, q: [c["wa"].removeprefix("fed-") for c in r["candidates"]]),
    ])
    report["pipelines"] = OrderedDict()
    for pipeline in ("bm25-postings", "bm25-sparse"):
        P = report["pipelines"][pipeline] = OrderedDict()
        recs = {}
        for split, rows in raw["splits"].items():
            P[split] = OrderedDict()
            for name, mk in selectors.items():
                if split == "unanswerable" and name.startswith("oracle"):
                    continue
                recs[(split, name)] = per_query(rows, qbid, topic_of, pipeline, caps[split], mk(caps[split]))
                P[split][name] = summary(recs[(split, name)])
        gate_c = recs[("cross", "content")] + recs[("skew", "content")]
        gate_d = recs[("cross", "described")] + recs[("skew", "described")]
        gate_o = recs[("cross", "content")] + recs[("skew", "content")]
        gate_oo = recs[("cross", "oracle_slots")] + recs[("skew", "oracle_slots")]
        sel_c = mean([r["sel_r"] for r in gate_c if "sel_r" in r])
        sel_d = mean([r["sel_r"] for r in gate_d if "sel_r" in r])
        scoped = paired(recs[("scoped", "content")], recs[("scoped", "described")])
        g = paired(gate_c, gate_d)
        P["per_query"] = {split: [
            {"id": c["id"], "scope": r["scope"], "content": c["searched"], "described": d["searched"],
             "s5": [c["s5"], d["s5"]]}
            for r, c, d in zip(raw["splits"][split], recs[(split, "content")], recs[(split, "described")])
            if set(c["searched"]) != set(d["searched"]) or c["s5"] != d["s5"]] for split in ("cross", "skew")}
        P["gate"] = OrderedDict([
            ("success@5_described_minus_content_cross_and_skew", g),
            ("by_category", by_category_delta(gate_c, gate_d)),
            ("ceiling_oracle_slots_minus_content", paired(gate_o, gate_oo)),
            ("ceiling_oracle_every_slot_minus_content", paired(gate_o, recs[("cross", "oracle_all")] + recs[("skew", "oracle_all")])),
            ("selection_recall", {"content": sel_c, "described": sel_d}),
            ("scoped_success@5_described_minus_content", scoped),
            ("passes", bool(g["delta_points"] >= 3.0 and sel_d >= sel_c and scoped["lost"] - scoped["gained"] <= 1)),
            ("redde_minus_content_skew", paired(recs[("skew", "content")], recs[("skew", "redde_analogue")])),
        ])

    # The adversarial descriptor (d11): each topic's descriptor replaced in turn; selection recall
    # on the cross and skew splits, counted in queries whose selection recall falls.
    adv = OrderedDict()
    for split in ("cross", "skew"):
        rows = raw["splits"][split]
        cap = caps[split]
        base = {r["id"]: per_query([r], qbid, topic_of, "bm25-postings", cap,
                                   lambda r, q: select_described(r["candidates"], cap, dsc(r), best))[0]
                for r in rows}
        worst = None
        for t in raw["topics"]:
            fell, displaced = 0, 0
            for r in rows:
                if t == r["scope"]:
                    continue
                d0 = select_described(r["candidates"], cap, dsc(r), best)
                d1 = select_described(r["candidates"], cap, dsc(r, swap=t), best)
                rec = per_query([r], qbid, topic_of, "bm25-postings", cap, lambda r, q: d1)[0]
                if rec.get("sel_r") is not None and rec["sel_r"] < base[r["id"]]["sel_r"]:
                    fell += 1
                displaced = max(displaced, len(set(d0) - set(d1)))
            if worst is None or fell > worst[1]:
                worst = (t, fell, displaced)
        adv[split] = {"worst_topic": worst[0], "queries_whose_selection_recall_fell": worst[1],
                      "max_sources_displaced": worst[2]}
    report["adversarial_descriptor"] = adv
    report["latency_ms_union_p95"] = {
        s: sorted(r["union/bm25-postings/ms"] for r in rows)[int(0.95 * len(rows)) - 1]
        for s, rows in raw["splits"].items()}

    (out / "fed2.json").write_text(json.dumps(report, indent=1))
    print(json.dumps({k: report[k] for k in ("replay_check", "alpha")}, indent=1))
    for p, P in report["pipelines"].items():
        print(p, json.dumps(P["gate"], indent=1))
    print(json.dumps(report["adversarial_descriptor"], indent=1))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect")
    c.add_argument("--url", required=True)
    c.add_argument("--state", required=True)
    c.add_argument("--out", required=True)
    c.add_argument("--scifact-zip", required=True)
    c.add_argument("--metrics-url", required=True)
    an = sub.add_parser("analyze")
    an.add_argument("--out", required=True)
    a = ap.parse_args()
    collect(a) if a.cmd == "collect" else analyze(a)


if __name__ == "__main__":
    main()
