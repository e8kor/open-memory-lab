#!/usr/bin/env python3
"""Remember eval, offline: duplicate hints, findability of a fresh write, "latest" under
supersession, and the quality warnings, over eval/remember/data.json.

Standard library only. Nothing here talks to a server: every method is a model of a served
rule that proposed ADR-0050 describes, so the numbers order the decisions and say nothing of
their served size. `--check` validates the data and exits.

    python3 -I eval/remember/run.py            # print the tables, write results/offline.json
    python3 -I eval/remember/run.py --check    # data check only
"""
from __future__ import annotations

import hashlib
import json
import math
import re
import sys
import unicodedata
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE / "data.json"
OUT = HERE / "results" / "offline.json"

STOP = set(
    "a an and are as at be because been but by can do does for from has have how i if in into is it "
    "its of on one or our so than that the their them then there these they this to up was we were "
    "what when where which while who why will with without you after before still not no".split()
)

# --- text handling: a model of ADR-0048 S2's normaliser and a crude English stemmer ------------


def nfc(text: str) -> str:
    return unicodedata.normalize("NFC", text)


def split_identifier(token: str) -> list[str]:
    """CamelCase and snake_case parts of one token, lower-cased."""
    parts = re.findall(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|\d+", token)
    return [p.lower() for p in parts if p]


def stem(word: str) -> str:
    for suffix, repl in (("ies", "y"), ("ing", ""), ("ed", ""), ("es", ""), ("s", ""), ("ly", "")):
        if len(word) > len(suffix) + 3 and word.endswith(suffix):
            return word[: -len(suffix)] + repl
    return word


def simple_tokens(text: str) -> list[str]:
    """Today's served index, roughly: PostgreSQL `simple` keeps dotted names whole."""
    return [t.lower() for t in re.findall(r"[\w][\w.\-/]*[\w]|[\w]", nfc(text))]


def indexed_terms(text: str) -> list[str]:
    """The terms an S2-style index would hold: words split at punctuation, identifier parts
    beside the whole, stop words dropped, stemmed."""
    out: list[str] = []
    for raw in re.findall(r"[\w][\w.\-/:#@]*[\w]|[\w]", nfc(text)):
        pieces = re.split(r"[.\-/:#@]+", raw)
        if len(pieces) > 1:
            out.append(raw.lower())
        for piece in pieces:
            low = piece.lower()
            if not low or low in STOP:
                continue
            out.append(stem(low))
            parts = split_identifier(piece) if ("_" in piece or re.search(r"[a-z][A-Z]", piece)) else []
            for part in parts:
                if part not in STOP and part != low:
                    out.append(stem(part))
            if "_" in piece:
                out.extend(stem(p) for p in low.split("_") if p and p not in STOP)
    return out


# --- BM25 over weighted fields ---------------------------------------------------------------


class Index:
    """BM25 (k1 1.2, b 0.75) over documents whose fields are weighted bags of terms."""

    def __init__(self, docs: dict[str, dict[str, float]]):
        self.docs = docs
        self.n = len(docs)
        self.lens = {d: sum(bag.values()) for d, bag in docs.items()}
        self.avg = sum(self.lens.values()) / max(self.n, 1)
        df: Counter[str] = Counter()
        for bag in docs.values():
            df.update(bag.keys())
        self.df = df

    def search(self, terms: list[str], k: int = 5) -> list[tuple[str, float, int]]:
        q = list(dict.fromkeys(terms))
        scored = []
        for d, bag in self.docs.items():
            s, hit = 0.0, 0
            for t in q:
                tf = bag.get(t, 0.0)
                if tf <= 0:
                    continue
                hit += 1
                idf = math.log(1 + (self.n - self.df[t] + 0.5) / (self.df[t] + 0.5))
                s += idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * self.lens[d] / self.avg))
            if hit:
                scored.append((d, s, hit))
        scored.sort(key=lambda x: (-x[1], x[0]))
        return scored[:k]


def bag(text: str, title: str = "", keywords: list[str] | None = None, w_title: float = 1.0,
        w_kw: float = 0.5) -> dict[str, float]:
    b: Counter[str] = Counter()
    for t in indexed_terms(text):
        b[t] += 1.0
    for t in indexed_terms(title):
        b[t] += w_title
    for kw in keywords or []:
        for t in indexed_terms(kw):
            b[t] += w_kw
    return dict(b)


def and_simple(docs: dict[str, str], query: str, k: int = 5) -> list[str]:
    """Today's served recall: every word, exactly, `simple`, ranked by term count."""
    q = set(simple_tokens(query))
    hits = []
    for d, text in docs.items():
        toks = simple_tokens(text)
        if q and q <= set(toks):
            hits.append((d, sum(toks.count(t) for t in q)))
    hits.sort(key=lambda x: (-x[1], x[0]))
    return [d for d, _ in hits[:k]]


def trigrams(text: str) -> set[str]:
    s = " " + re.sub(r"\s+", " ", nfc(text).lower()) + " "
    return {s[i : i + 3] for i in range(len(s) - 2)}


# --- 1. the similar hint ------------------------------------------------------------------


def similar_hint(method: str, write: dict, seeds: dict[str, str], seed_index: Index,
                 theta: float) -> list[str]:
    text = write["text"]
    if method == "none":
        return []
    if method == "digest":
        dig = hashlib.sha256(nfc(text).encode()).hexdigest()
        return [s for s, t in seeds.items() if hashlib.sha256(nfc(t).encode()).hexdigest() == dig]
    if method == "and_text":
        return and_simple(seeds, text, k=3)
    if method == "shared_terms":
        terms = list(dict.fromkeys(indexed_terms(text)))
        out = []
        for d, _, hit in seed_index.search(terms, k=3):
            if hit / max(len(terms), 1) >= theta:
                out.append(d)
        return out
    if method == "trigram":
        tg = trigrams(text)
        scored = []
        for s, t in seeds.items():
            other = trigrams(t)
            j = len(tg & other) / len(tg | other)
            if j >= theta:
                scored.append((s, j))
        scored.sort(key=lambda x: -x[1])
        return [s for s, _ in scored[:3]]
    raise ValueError(method)


def eval_hints(data: dict) -> list[dict]:
    seeds = {s["id"]: s["text"] for s in data["seeds"]}
    seed_index = Index({s: bag(t) for s, t in seeds.items()})
    rows = []
    configs = [("none", 0.0), ("digest", 0.0), ("and_text", 0.0)]
    configs += [("shared_terms", t) for t in (0.2, 0.3, 0.4, 0.5)]
    configs += [("trigram", t) for t in (0.2, 0.3, 0.4)]
    for method, theta in configs:
        found = misses = alarms = hints = right_hints = 0
        per = {}
        for w in data["writes"]:
            got = similar_hint(method, w, seeds, seed_index, theta)
            per[w["id"]] = got
            hints += len(got)
            if w["label"] in ("duplicate", "update"):
                if w["of"] in got:
                    found += 1
                    right_hints += 1
                else:
                    misses += 1
            elif got:
                alarms += 1
        n_pair = sum(1 for w in data["writes"] if w["label"] in ("duplicate", "update"))
        n_novel = sum(1 for w in data["writes"] if w["label"] == "novel")
        rows.append({
            "method": method if not theta else f"{method}>={theta}",
            "hint_recall": round(found / n_pair, 2),
            "false_alarm": round(alarms / n_novel, 2),
            "precision": round(right_hints / hints, 2) if hints else None,
            # An agent that follows a correct hint supersedes (an update) or writes nothing (a
            # duplicate); every missed pair leaves a second live claim on one subject.
            "residual_duplicate_rate": round(misses / len(data["writes"]), 2),
            "per_write": per,
        })
    return rows


# --- 2. findability of a fresh write -----------------------------------------------------


def stored_after(data: dict) -> dict[str, dict]:
    """The store once the writes are admitted under supersession: an update replaces its seed's
    tip, a duplicate is not written, a novel write is new."""
    store = {s["id"]: {"text": s["text"], "title": "", "keywords": []} for s in data["seeds"]}
    for w in data["writes"]:
        if w["label"] == "duplicate":
            continue
        if w["label"] == "update":
            store.pop(w["of"], None)
        store[w["id"]] = {"text": w["text"], "title": w["title"], "keywords": w["keywords"]}
    return store


def eval_find(data: dict) -> list[dict]:
    store = stored_after(data)
    methods = {
        "served_and": None,
        "text_any": dict(w_title=0.0, w_kw=0.0, use_title=False, use_kw=False),
        "text_title": dict(w_title=1.0, w_kw=0.0, use_title=True, use_kw=False),
        "text_title_keywords": dict(w_title=1.0, w_kw=0.5, use_title=True, use_kw=True),
    }
    rows = []
    for name, cfg in methods.items():
        s1 = s5 = zero = 0
        ranks = {}
        if cfg is None:
            texts = {d: v["text"] for d, v in store.items()}
        else:
            idx = Index({d: bag(v["text"], v["title"] if cfg["use_title"] else "",
                                v["keywords"] if cfg["use_kw"] else [], cfg["w_title"], cfg["w_kw"])
                         for d, v in store.items()})
        for q in data["find_queries"]:
            if cfg is None:
                got = and_simple(texts, q["query"], k=5)
            else:
                got = [d for d, _, _ in idx.search(indexed_terms(q["query"]), k=5)]
            ranks[q["id"]] = got
            zero += not got
            s1 += bool(got) and got[0] == q["target"]
            s5 += q["target"] in got
        n = len(data["find_queries"])
        rows.append({"method": name, "success@1": round(s1 / n, 2), "success@5": round(s5 / n, 2),
                     "zero_result": round(zero / n, 2), "ranks": ranks})
    return rows


# --- 3. "latest" ----------------------------------------------------------------------------


def eval_latest(data: dict) -> list[dict]:
    seeds = {s["id"]: s["text"] for s in data["seeds"]}
    updates = {w["id"]: w["text"] for w in data["writes"] if w["label"] == "update"}
    piled = {**seeds, **updates}
    tips = dict(seeds)
    for w in data["writes"]:
        if w["label"] == "update":
            tips.pop(w["of"], None)
            tips[w["id"]] = w["text"]
    rows = []
    for name, store in (("pile_up", piled), ("supersession", tips)):
        idx = Index({d: bag(t) for d, t in store.items()})
        right = stale_first = 0
        ranks = {}
        for q in data["latest_queries"]:
            got = [d for d, _, _ in idx.search(indexed_terms(q["query"]), k=5)]
            ranks[q["id"]] = got
            right += bool(got) and got[0] == q["current"]
            stale_first += bool(got) and got[0] == q["stale"]
        n = len(data["latest_queries"])
        rows.append({"store": name, "latest_correct@1": round(right / n, 2),
                     "stale_first": round(stale_first / n, 2), "ranks": ranks})
    return rows


# --- 4. quality warnings ------------------------------------------------------------------

RELATIVE = re.compile(r"\b(today|yesterday|tomorrow|recently|last (?:week|month|year)|next (?:week|month)|"
                      r"this (?:week|month)|currently|now)\b", re.I)
ISO_DATE = re.compile(r"\b\d{4}-\d{2}-\d{2}\b")
VAGUE_START = re.compile(r"^\s*(it|this|that|these|those|they|here)\b", re.I)
VAGUE_WORDS = re.compile(r"\b(somehow|something|somebody|some changes|later|stuff|etc)\b", re.I)


def warnings(text: str) -> list[str]:
    out = []
    if len(text.encode()) > 600:
        out.append("too_long")
    lines = [ln for ln in text.splitlines() if re.match(r"\s*([-*•]|\d+[.)])\s+", ln)]
    sentences = [s for s in re.split(r"(?<=[.!?])\s+", text.strip()) if s]
    if len(lines) >= 3 or len(sentences) >= 4:
        out.append("many_claims")
    if VAGUE_START.search(text) and VAGUE_WORDS.search(text):
        out.append("vague")
    if RELATIVE.search(text) and not ISO_DATE.search(text):
        out.append("relative_time")
    return out


def eval_quality(data: dict) -> dict:
    good = [s["text"] for s in data["seeds"]] + [w["text"] for w in data["writes"]]
    false = [t for t in good if warnings(t)]
    caught = sum(1 for b in data["quality_planted"] if b["expect"] in warnings(b["text"]))
    return {
        "false_warning_rate": round(len(false) / len(good), 2),
        "false_warnings": false,
        "planted_caught": round(caught / len(data["quality_planted"]), 2),
        "per_planted": {b["id"]: warnings(b["text"]) for b in data["quality_planted"]},
    }


# --- check and main -----------------------------------------------------------------------


def check(data: dict) -> list[str]:
    problems = []
    ids = {s["id"] for s in data["seeds"]} | {w["id"] for w in data["writes"]}
    for w in data["writes"]:
        if w["label"] not in ("duplicate", "update", "novel"):
            problems.append(f"{w['id']}: label {w['label']}")
        if w["label"] != "novel" and w.get("of") not in ids:
            problems.append(f"{w['id']}: of {w.get('of')}")
        if len(w["keywords"]) > 8 or len(w["title"]) > 100:
            problems.append(f"{w['id']}: more than 8 keywords or a title over 100 characters")
    for q in data["find_queries"]:
        if q["target"] not in ids:
            problems.append(f"{q['id']}: target {q['target']}")
    for q in data["latest_queries"]:
        if q["current"] not in ids or q["stale"] not in ids:
            problems.append(f"{q['id']}: unknown id")
    return problems


def table(rows: list[dict], cols: list[str]) -> str:
    head = "| " + " | ".join(cols) + " |\n|" + "|".join("---" for _ in cols) + "|\n"
    return head + "".join("| " + " | ".join(str(r.get(c, "")) for c in cols) + " |\n" for r in rows)


def main() -> int:
    data = json.loads(DATA.read_text())
    problems = check(data)
    if problems:
        print("\n".join(problems))
        return 1
    if "--check" in sys.argv:
        print("remember eval data: ok")
        return 0
    hints, find, latest, quality = eval_hints(data), eval_find(data), eval_latest(data), eval_quality(data)
    print("## Similar hint (24 writes: 8 duplicates, 8 updates, 8 novel)\n")
    print(table(hints, ["method", "hint_recall", "false_alarm", "precision", "residual_duplicate_rate"]))
    print("## Findability of a fresh write (24 paraphrased queries)\n")
    print(table(find, ["method", "success@1", "success@5", "zero_result"]))
    print("## Latest (8 queries over 8 changed facts)\n")
    print(table(latest, ["store", "latest_correct@1", "stale_first"]))
    print("## Quality warnings\n")
    print(f"false warnings on the 64 well-formed texts: {quality['false_warning_rate']}; "
          f"planted problems caught: {quality['planted_caught']}")
    OUT.parent.mkdir(exist_ok=True)
    OUT.write_text(json.dumps({"data_version": data["version"], "hints": hints, "find": find,
                               "latest": latest, "quality": quality}, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
