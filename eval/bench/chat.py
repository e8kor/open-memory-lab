#!/usr/bin/env python3
"""Chat memory end to end: LongMemEval (S) and LoCoMo through ADR-0063's path.

    chat.py workareas <dataset> [--limit N]       the workareas boot.sh must create
    chat.py load --state S <dataset> [--limit N]  write every turn as a chat memory
    chat.py run  --state S --out DIR <dataset> [--limit N] [--k 50]
                                                  retrieval proxy: recall@k of evidence turns
    chat.py qa   --state S --out DIR <dataset> --answer-model M --judge-model J
                                                  answer with a fixed model, judge (needs a key)

Datasets: `longmemeval-s` (LongMemEval's cleaned S set, 500 questions, each
with its own haystack of about 50 sessions and 500 turns) and `locomo` (ten
conversations, about 1,990 questions). Both are read from BENCH_CACHE
(default /srv/build/bench-data) and never committed.

load: each turn is one `POST /v1/memories` by the writer (agent) key with
`origin: {kind: chat}` and a chat anchor, `chat://bench/<session>#turn=<n>`,
whose session is the first 32 hex digits of SHA-256 of the dataset's session
id and whose hash is `oml-turn/1` of the turn's text keyed by a random salt per run (ADR-0063 d4, d5). The
rows stay held, as a person's chat workarea holds them before review, and
are recalled by their author with `include_unreviewed` (ADR-0025 d4). Each
LongMemEval question is its own workarea, `bench-chat-lme-<qid>`; each
LoCoMo conversation its own, `bench-chat-locomo-<sample>`. Turn-level
ingestion is the floor an agent's own extraction must beat (d12; FR-160).

run: each question is asked once through `POST /v1/memories/recall` in its
workarea with `match: any`, `include_unreviewed: true` and `origin_kinds:
[chat]`, k 50. Measured without a model: turn recall@k (k 5, 10, 50) of the
evidence turns (LongMemEval's `has_answer` turns, LoCoMo's `evidence`), and,
for LongMemEval, session recall@k against `answer_session_ids`. A question
without evidence (LongMemEval's abstention `_abs` questions) is left out of
the proxy.

qa: the top-k hits' text and the question go to one fixed answering model,
and the answer is graded by a fixed judge model with LongMemEval's published
judge prompts (MIT, xiaowu0162/LongMemEval src/evaluation/evaluate_qa.py,
`get_anscheck_prompt`, copied verbatim below); LoCoMo is graded with the
single-session template. Both model ids are required and recorded in the
result. It needs ANTHROPIC_API_KEY (models named `claude-…`) or
OPENAI_API_KEY (the rest); without the key it names it and exits 2, and the
key's value is never printed.

Results are written in `oml-bench/1` (README.md, Results schema), kind
`chat-recall` or `chat-qa`, one file per suite and method under --out.
"""

import argparse
import concurrent.futures as cf
import datetime
import hashlib
import hmac
import json
import os
import random
import re
import statistics
import sys
import time
import unicodedata
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "recall"))
from run import Client  # noqa: E402

DEFAULT_CACHE = os.environ.get("BENCH_CACHE", "/srv/build/bench-data")
LOCOMO_URL = "https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json"
KS = [5, 10, 50]
K = 50
SEED = 20261105


def pseudonym(session_id):
    return hashlib.sha256(session_id.encode()).hexdigest()[:32]


# oml-turn/1 is keyed by the person's salt (ADR-0063 d4); the bench keys it
# by a random salt per run, as a person's never-sent salt would.
BENCH_SALT = os.urandom(32)


def turn_hash(text):
    nfc = unicodedata.normalize("NFC", text)
    return "sha256:" + hmac.new(BENCH_SALT, nfc.encode(), hashlib.sha256).hexdigest()


def safe(s):
    return re.sub(r"[^a-z0-9-]", "-", s.lower()).strip("-")[:40] or "x"


class Turn:
    __slots__ = ("id", "session", "n", "text", "group", "date")

    def __init__(self, id, session, n, text, group, date=""):
        self.id, self.session, self.n, self.text, self.group, self.date = (
            id, session, n, text, group, date)


class Question:
    __slots__ = ("id", "text", "group", "category", "answer", "evidence", "sessions", "date")

    def __init__(self, id, text, group, category, answer, evidence, sessions=(), date=""):
        self.id, self.text, self.group, self.category = id, text, group, category
        self.answer, self.evidence, self.sessions, self.date = answer, set(evidence), set(sessions), date


def sample(items, limit, seed):
    if limit and len(items) > limit:
        items = sorted(random.Random(seed).sample(items, limit), key=lambda q: q.id)
    return items


def load_longmemeval(cache, limit, seed):
    path = Path(cache) / "longmemeval" / "longmemeval_s_cleaned.json"
    if not path.exists():
        raise SystemExit(f"{path} is missing: download LongMemEval's cleaned S set there "
                         "(huggingface.co/datasets/xiaowu0162/longmemeval-cleaned)")
    data = json.loads(path.read_text())
    questions = []
    for q in data:
        group = "lme-" + safe(q["question_id"])
        questions.append((q, Question(q["question_id"], q["question"], group, q["question_type"]
                                      + ("-abstention" if "_abs" in q["question_id"] else ""),
                                      q["answer"], (), q.get("answer_session_ids", []),
                                      q.get("question_date", ""))))
    chosen = {qq.id for _, qq in questions}
    if limit:
        chosen = {qq.id for qq in sample([qq for _, qq in questions], limit, seed)}
    turns, qs = [], []
    for raw, qq in questions:
        if qq.id not in chosen:
            continue
        for sid, date, session in zip(raw["haystack_session_ids"], raw["haystack_dates"],
                                      raw["haystack_sessions"]):
            for n, t in enumerate(session, 1):
                tid = f"{qq.group}/{sid}/{n}"
                turns.append(Turn(tid, sid, n, f"[{date}] {t['role']}: {t['content']}",
                                  qq.group, date))
                if t.get("has_answer"):
                    qq.evidence.add(tid)
        qs.append(qq)
    return turns, qs


LOCOMO_CATEGORIES = {1: "multi-hop", 2: "temporal", 3: "open-domain", 4: "single-hop",
                     5: "adversarial"}


def load_locomo(cache, limit, seed):
    path = Path(cache) / "locomo" / "locomo10.json"
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        urllib.request.urlretrieve(LOCOMO_URL, path)
    data = json.loads(path.read_text())
    turns, qs = [], []
    for conv in data:
        group = "locomo-" + safe(conv["sample_id"])
        c = conv["conversation"]
        ids = {}
        s = 1
        while f"session_{s}" in c:
            date = c.get(f"session_{s}_date_time", "")
            for n, t in enumerate(c[f"session_{s}"], 1):
                text = f"[{date}] {t['speaker']}: {t['text']}"
                if t.get("blip_caption"):
                    text += f" [shares a photo: {t['blip_caption']}]"
                tid = f"{group}/session_{s}/{n}"
                ids[t["dia_id"]] = tid
                turns.append(Turn(tid, f"{conv['sample_id']}-session_{s}", n, text, group, date))
            s += 1
        for i, q in enumerate(conv["qa"]):
            ev = set()
            for e in q.get("evidence") or []:
                for one in re.split(r"[;,\s]+", e):
                    if one.strip() in ids:
                        ev.add(ids[one.strip()])
            answer = q.get("answer", q.get("adversarial_answer", ""))
            qs.append(Question(f"{group}.q{i}", q["question"], group,
                               LOCOMO_CATEGORIES.get(q["category"], str(q["category"])),
                               str(answer), ev))
    groups = sorted({q.group for q in qs})
    if limit and limit < len(groups):
        keep = set(random.Random(seed).sample(groups, limit))
        turns = [t for t in turns if t.group in keep]
        qs = [q for q in qs if q.group in keep]
    return turns, qs


DATASETS = {"longmemeval-s": load_longmemeval, "locomo": load_locomo}
SUITES = {"longmemeval-s": "chat-longmemeval-s", "locomo": "chat-locomo"}


def dataset(name, cache, limit, seed):
    if name not in DATASETS:
        raise SystemExit(f"unknown dataset {name!r}; known: {', '.join(DATASETS)}")
    return DATASETS[name](cache, limit, seed)


def workarea(group):
    return f"bench-chat-{group}"


def claim_key(turn):
    return "chat/" + re.sub(r"[^A-Za-z0-9._/-]", "_", turn.id)


def anchor(turn):
    return {"uri": f"chat://bench/{pseudonym(turn.session)}", "fragment": f"turn={turn.n}",
            "hash": turn_hash(turn.text), "hasher": "oml-turn/1"}


def writer(state):
    state = Path(state)
    return Client((state / "url").read_text().strip(), (state / "writer.key").read_text().strip())


def cmd_workareas(a):
    _, qs = dataset(a.dataset, a.cache, a.limit, a.seed)
    print(" ".join(sorted({workarea(q.group) for q in qs})))


def cmd_load(a):
    turns, _ = dataset(a.dataset, a.cache, a.limit, a.seed)
    w = writer(a.state)
    refused, t0 = {}, time.perf_counter()

    def write(t):
        body = {"body": t.text[:200_000], "claim_key": claim_key(t), "kind": "memory",
                "workarea": workarea(t.group), "origin": {"kind": "chat"}, "anchor": anchor(t)}
        for attempt in range(8):
            status, resp = w.call("POST", "/v1/memories", body, idempotent=True)
            if status in (202, 200):
                return None
            if status == 409 and isinstance(resp, dict) and \
                    resp.get("error", {}).get("code") == "submission.claim_key_taken":
                return None
            if status < 500 and status != 429:
                break
            time.sleep(0.05 * 2 ** attempt)
        return f"{status} {json.dumps(resp)[:200]}"

    with cf.ThreadPoolExecutor(a.workers) as pool:
        for t, err in zip(turns, pool.map(write, turns)):
            if err:
                refused[t.id] = err
    wall = time.perf_counter() - t0
    log = {"dataset": a.dataset, "turns": len(turns), "refused": len(refused),
           "examples": dict(list(refused.items())[:5]), "wall_seconds": round(wall, 1)}
    print(json.dumps(log))
    if refused:
        raise SystemExit(1)


def recall(w, q, k):
    body = {"query": q.text, "k": k, "workarea": workarea(q.group), "match": "any",
            "include_unreviewed": True, "include_text": True, "origin_kinds": ["chat"]}
    t0 = time.perf_counter()
    status, resp = w.call("POST", "/v1/memories/recall", body)
    ms = (time.perf_counter() - t0) * 1000
    if status != 200:
        raise SystemExit(f"recall failed for {q.id}: {status} {resp}")
    hits = [h.get("memory", h) for h in resp.get("hits", [])]
    return hits, ms


def turn_of(hit):
    ck = hit.get("claim_key", "")
    return ck[len("chat/"):] if ck.startswith("chat/") else None


def session_of(turn_id):
    parts = turn_id.split("/")
    return parts[1] if len(parts) >= 3 else None


def pct(values, p):
    if not values:
        return None
    values = sorted(values)
    return round(values[min(len(values) - 1, int(round(p / 100 * (len(values) - 1))))], 2)


def cmd_run(a):
    _, qs = dataset(a.dataset, a.cache, a.limit, a.seed)
    qs = [q for q in qs if q.evidence]
    w = writer(a.state)

    def ask(q):
        hits, ms = recall(w, q, a.k)
        ranked = [turn_of(h) for h in hits]
        row = {"latency_ms": round(ms, 2), "ranked": ranked}
        for k in KS:
            top = set(ranked[:k])
            row[f"turn_recall@{k}"] = len(q.evidence & top) / len(q.evidence)
            if q.sessions:
                got = {session_of(t) for t in ranked[:k] if t}
                row[f"session_recall@{k}"] = len(q.sessions & got) / len(q.sessions)
        return q, row

    per, t0 = {}, time.perf_counter()
    with cf.ThreadPoolExecutor(a.workers) as pool:
        for q, row in pool.map(ask, qs):
            per[q.id] = {**row, "category": q.category}
    wall = time.perf_counter() - t0
    metrics = summary(per)
    cats = sorted({r["category"] for r in per.values()})
    result = envelope(a, "chat-recall", "turns-as-memories/bm25-postings", len(per), metrics, {
        c: summary({k: r for k, r in per.items() if r["category"] == c}) for c in cats},
        [r["latency_ms"] for r in per.values()],
        ["retrieval proxy: no model; each turn stored as one held chat memory; "
         "questions without evidence turns left out",
         f"wall {wall:.0f}s with {a.workers} workers"])
    out = Path(a.out) / SUITES[a.dataset]
    out.mkdir(parents=True, exist_ok=True)
    (out / "turns-as-memories.json").write_text(json.dumps(result, indent=1) + "\n")
    (out / "turns-as-memories.per-query.json").write_text(json.dumps(per) + "\n")
    print(json.dumps({"suite": SUITES[a.dataset], "n": len(per), **metrics}))


def summary(per):
    keys = sorted({k for r in per.values() for k in r if "@" in k})
    return {k: round(statistics.mean(r[k] for r in per.values() if k in r), 4) for k in keys
            if any(k in r for r in per.values())}


def envelope(a, kind, method, n, metrics, per_category, latencies, notes, models=None):
    return {
        "schema": "oml-bench/1", "suite": SUITES[a.dataset], "suite_version": None,
        "kind": kind, "method": method,
        "date": datetime.date.today().isoformat(), "commit": a.commit,
        "environment": {"host": a.host, "postgres": a.postgres, "build": a.build},
        "n_queries": n, "n_docs": None,
        "sampled": {"seed": a.seed, "limit": a.limit} if a.limit else None,
        "metrics": metrics, "per_category": per_category,
        "latency_ms": {"p50": pct(latencies, 50), "p95": pct(latencies, 95)},
        "models": models, "reference": None, "notes": notes,
    }


# LongMemEval's judge prompts, verbatim from src/evaluation/evaluate_qa.py
# (MIT, Copyright (c) 2024 Di Wu), get_anscheck_prompt.
_HEAD = ("I will give you a question, a correct answer, and a response from a model. Please answer "
         "yes if the response contains the correct answer. Otherwise, answer no. ")
JUDGE = {
    "default": _HEAD + "If the response is equivalent to the correct answer or contains all the "
    "intermediate steps to get the correct answer, you should also answer yes. If the response only "
    "contains a subset of the information required by the answer, answer no. \n\nQuestion: {}\n\n"
    "Correct Answer: {}\n\nModel Response: {}\n\nIs the model response correct? Answer yes or no only.",
    "temporal-reasoning": _HEAD + "If the response is equivalent to the correct answer or contains all "
    "the intermediate steps to get the correct answer, you should also answer yes. If the response "
    "only contains a subset of the information required by the answer, answer no. In addition, do "
    "not penalize off-by-one errors for the number of days. If the question asks for the number of "
    "days/weeks/months, etc., and the model makes off-by-one errors (e.g., predicting 19 days when "
    "the answer is 18), the model's response is still correct. \n\nQuestion: {}\n\nCorrect Answer: "
    "{}\n\nModel Response: {}\n\nIs the model response correct? Answer yes or no only.",
    "knowledge-update": _HEAD + "If the response contains some previous information along with an "
    "updated answer, the response should be considered as correct as long as the updated answer is "
    "the required answer.\n\nQuestion: {}\n\nCorrect Answer: {}\n\nModel Response: {}\n\nIs the "
    "model response correct? Answer yes or no only.",
    "single-session-preference": "I will give you a question, a rubric for desired personalized "
    "response, and a response from a model. Please answer yes if the response satisfies the desired "
    "response. Otherwise, answer no. The model does not need to reflect all the points in the "
    "rubric. The response is correct as long as it recalls and utilizes the user's personal "
    "information correctly.\n\nQuestion: {}\n\nRubric: {}\n\nModel Response: {}\n\nIs the model "
    "response correct? Answer yes or no only.",
    "abstention": "I will give you an unanswerable question, an explanation, and a response from a "
    "model. Please answer yes if the model correctly identifies the question as unanswerable. The "
    "model could say that the information is incomplete, or some other information is given but "
    "the asked information is not.\n\nQuestion: {}\n\nExplanation: {}\n\nModel Response: {}\n\n"
    "Does the model correctly identify the question as unanswerable? Answer yes or no only.",
}

ANSWER = ("Answer the question using only the memories below, which were recalled from earlier "
          "conversations. If they do not hold the answer, say that you do not know.\n\n"
          "Current date: {date}\n\nMemories:\n{memories}\n\nQuestion: {question}\n\nAnswer briefly.")


def key_for(model):
    name = "ANTHROPIC_API_KEY" if model.startswith("claude") else "OPENAI_API_KEY"
    value = os.environ.get(name, "")
    if not value:
        raise SystemExit(2)
    return name, value


def complete(model, prompt, max_tokens):
    name, key = key_for(model)
    if name == "ANTHROPIC_API_KEY":
        req = urllib.request.Request(
            "https://api.anthropic.com/v1/messages",
            data=json.dumps({"model": model, "max_tokens": max_tokens, "temperature": 0,
                             "messages": [{"role": "user", "content": prompt}]}).encode(),
            headers={"x-api-key": key, "anthropic-version": "2023-06-01",
                     "content-type": "application/json"})
        pick = lambda r: "".join(b.get("text", "") for b in r["content"])  # noqa: E731
    else:
        req = urllib.request.Request(
            "https://api.openai.com/v1/chat/completions",
            data=json.dumps({"model": model, "max_tokens": max_tokens, "temperature": 0,
                             "messages": [{"role": "user", "content": prompt}]}).encode(),
            headers={"authorization": f"Bearer {key}", "content-type": "application/json"})
        pick = lambda r: r["choices"][0]["message"]["content"]  # noqa: E731
    for attempt in range(6):
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return pick(json.loads(resp.read()))
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 502, 503, 529) and attempt < 5:
                time.sleep(2 ** attempt)
                continue
            raise SystemExit(f"{model}: HTTP {e.code}")
    raise SystemExit(f"{model}: no answer")


def cmd_qa(a):
    for model in (a.answer_model, a.judge_model):
        name = "ANTHROPIC_API_KEY" if model.startswith("claude") else "OPENAI_API_KEY"
        if not os.environ.get(name):
            print(f"chat.py qa: {name} is not set, so no answer or grade was produced; "
                  "run `chat.py run` for the retrieval proxy (FR-164)", file=sys.stderr)
            raise SystemExit(2)
    _, qs = dataset(a.dataset, a.cache, a.limit, a.seed)
    w = writer(a.state)

    def one(q):
        hits, ms = recall(w, q, a.k)
        memories = "\n".join(f"- {h.get('text', '')[:2000]}" for h in hits[: a.k])
        answer = complete(a.answer_model, ANSWER.format(date=q.date or "unknown",
                                                         memories=memories, question=q.text), 300)
        cat = q.category
        template = JUDGE["abstention"] if cat.endswith("-abstention") else JUDGE.get(cat, JUDGE["default"])
        verdict = complete(a.judge_model, template.format(q.text, q.answer, answer), 10)
        return q, {"category": cat, "latency_ms": round(ms, 2), "answer": answer,
                   "correct": "yes" in verdict.strip().lower()}

    per = {}
    with cf.ThreadPoolExecutor(a.workers) as pool:
        for q, row in pool.map(one, qs):
            per[q.id] = row
    acc = lambda rows: round(statistics.mean(1.0 if r["correct"] else 0.0 for r in rows), 4)  # noqa: E731
    cats = sorted({r["category"] for r in per.values()})
    result = envelope(a, "chat-qa", "turns-as-memories/bm25-postings", len(per),
                      {"accuracy": acc(per.values())},
                      {c: {"accuracy": acc([r for r in per.values() if r["category"] == c])}
                       for c in cats},
                      [r["latency_ms"] for r in per.values()],
                      [f"top {a.k} recalled turns given to the answering model",
                       "LongMemEval's judge prompts; yes anywhere in the judge's reply counts, as there"],
                      models={"answer": a.answer_model, "judge": a.judge_model})
    out = Path(a.out) / SUITES[a.dataset]
    out.mkdir(parents=True, exist_ok=True)
    (out / "turns-as-memories.qa.json").write_text(json.dumps(result, indent=1) + "\n")
    print(json.dumps({"suite": SUITES[a.dataset], "n": len(per), **result["metrics"]}))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("workareas", "load", "run", "qa"):
        s = sub.add_parser(name)
        s.add_argument("dataset")
        s.add_argument("--cache", default=DEFAULT_CACHE)
        s.add_argument("--limit", type=int, default=None,
                       help="questions (LongMemEval) or conversations (LoCoMo), drawn with --seed")
        s.add_argument("--seed", type=int, default=SEED)
        if name != "workareas":
            s.add_argument("--state", required=True)
            s.add_argument("--workers", type=int, default=8)
        if name in ("run", "qa"):
            s.add_argument("--out", required=True)
            s.add_argument("--k", type=int, default=K)
            s.add_argument("--commit", default=None)
            s.add_argument("--host", default=None)
            s.add_argument("--postgres", default=None)
            s.add_argument("--build", default="release")
        if name == "qa":
            s.add_argument("--answer-model", required=True)
            s.add_argument("--judge-model", required=True)
    a = p.parse_args()
    {"workareas": cmd_workareas, "load": cmd_load, "run": cmd_run, "qa": cmd_qa}[a.cmd](a)


if __name__ == "__main__":
    main()
