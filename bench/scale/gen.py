"""Synthetic OML memories and queries for the scale bench.

Seeded from the recall eval's 200-memory corpus and 121 queries
(eval/recall/corpus.json, queries.json on origin/feat/recall-eval, data
version 1, copied here as seed_corpus.json and seed_queries.json). Every
generated text is a seed text with templated variation: numbers redrawn,
identifiers mutated or replaced from a pool that grows with the corpus (so the
vocabulary grows roughly as Heaps' law says a real one does), topic words
swapped within their topic, an optional second clause from another seed of the
same topic, and optional ticket, host or version references. Deterministic for
a given (seed, n).

The generator knows nothing about which text answers which query: the bench
measures latency, index size and write cost, not relevance.
"""

from __future__ import annotations

import hashlib
import json
import random
import re
import uuid
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOKEN = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.\-/%']*[A-Za-z0-9%]|[A-Za-z0-9]")
NUM = re.compile(r"\d+")
IDENT = re.compile(r"[_.]|[a-z][A-Z]|^[A-Z0-9_]{3,}$|-\w*\d|\d\w*-")

STATUS_MIX = [  # (status, is_tip, weight): a realistic held/promoted mix
    ("promoted", True, 0.65),
    ("promoted", False, 0.10),  # replaced versions, readable history, not served
    ("held", False, 0.15),
    ("rejected", False, 0.06),
    ("demoted", False, 0.04),
]
KIND_MIX = [("memory", 0.85), ("derived_fact", 0.09), ("steering", 0.05), ("guardrail", 0.01)]
SUFFIXES = ["_v2", "_v3", "_daily", "_hourly", "_eu", "_us", "-prod", "-stage", "-euw1",
            "-use1", "_legacy", "_tmp", "_raw", "_agg", "Service", "Client", "Handler",
            "Job", "_id", "_at", "_count", "Config", "-worker", "-api", "_backfill"]
PREFIXES = ["fct", "dim", "stg", "int", "tl", "svc", "job", "cron", "use", "get", "set",
            "is", "on", "api", "billing", "invoice", "ledger", "auth", "pdf", "sync"]
TICKETS = ["INC", "OPS", "BILL", "PLAT", "DATA", "SEC", "FE", "API"]


def _words(text: str) -> list[str]:
    return TOKEN.findall(text)


def _is_ident(tok: str) -> bool:
    return bool(IDENT.search(tok)) and len(tok) > 3


@dataclass
class Seeds:
    memories: list[dict]
    queries: list[dict]
    topic_words: dict[str, list[str]] = field(default_factory=dict)
    idents: list[str] = field(default_factory=list)
    nouns: list[str] = field(default_factory=list)

    @classmethod
    def load(cls) -> "Seeds":
        mem = json.loads((HERE / "seed_corpus.json").read_text())["memories"]
        qs = json.loads((HERE / "seed_queries.json").read_text())["queries"]
        s = cls(mem, qs)
        stop = set("the a an and or of to in on for with by is are was be at as it this that "
                   "from we our not every never so if than then only into its has have".split())
        idents, nouns = set(), set()
        for m in mem:
            ws = []
            for w in _words(m["text"]):
                if _is_ident(w):
                    idents.add(w)
                elif w.isalpha() and w.lower() not in stop and len(w) > 3:
                    ws.append(w)
                    nouns.add(w.lower())
            s.topic_words.setdefault(m["topic"], []).extend(ws)
        s.idents = sorted(idents)
        s.nouns = sorted(nouns)
        return s


class Generator:
    """Texts for one tenant: deterministic per (seed, n)."""

    def __init__(self, seeds: Seeds, n: int, seed: int):
        self.s = seeds
        self.r = random.Random(seed)
        # The identifier pool grows with the corpus: about one new identifier
        # per 6 memories (Heaps-like), each a mutation of a seed identifier or
        # a prefix_noun_noun composition.
        r = self.r
        pool = list(seeds.idents)
        target = max(len(pool), n // 6)
        while len(pool) < target:
            if r.random() < 0.5:
                pool.append(r.choice(seeds.idents) + r.choice(SUFFIXES))
            else:
                a, b = r.choice(seeds.nouns), r.choice(seeds.nouns)
                sep = r.choice(["_", "_", "-", ""])
                tok = f"{r.choice(PREFIXES)}{sep}{a}{sep}{b}"
                pool.append(tok if sep else tok[0] + tok[1:].title().replace(" ", ""))
        self.idents = pool
        self.by_topic: dict[str, list[dict]] = {}
        for m in seeds.memories:
            self.by_topic.setdefault(m["topic"], []).append(m)

    def _num(self, m: re.Match) -> str:
        d = m.group(0)
        if len(d) == 4 and d.startswith("20"):
            return str(self.r.choice([2024, 2025, 2026]))
        return str(self.r.randint(10 ** (len(d) - 1) if len(d) > 1 else 0, 10 ** len(d) - 1))

    def _vary(self, text: str, topic: str) -> str:
        r = self.r
        tw = self.s.topic_words[topic]
        out = []
        for tok in re.split(r"(\s+)", text):
            core = tok.strip(",.;:()'\"")
            if not core:
                out.append(tok)
                continue
            if _is_ident(core) and r.random() < 0.6:
                tok = tok.replace(core, self._ident())
            elif core.isalpha() and len(core) > 3 and r.random() < 0.12:
                tok = tok.replace(core, r.choice(tw))
            out.append(tok)
        return NUM.sub(self._num, "".join(out))

    def _ident(self) -> str:
        # Zipf-ish: a few identifiers are everywhere, most are rare.
        i = int(len(self.idents) * (self.r.random() ** 2.2))
        return self.idents[min(i, len(self.idents) - 1)]

    def text(self) -> tuple[str, int, str]:
        """(text, seed index, topic)."""
        r = self.r
        si = r.randrange(len(self.s.memories))
        m = self.s.memories[si]
        t = self._vary(m["text"], m["topic"])
        if r.random() < 0.35:
            other = r.choice(self.by_topic[m["topic"]])
            clause = self._vary(other["text"], m["topic"]).rstrip(".")
            t = t.rstrip(".") + "; " + clause[0].lower() + clause[1:] + "."
        if r.random() < 0.2:
            t += f" See {r.choice(TICKETS)}-{r.randint(100, 9999)}."
        if r.random() < 0.1:
            t += f" Verified on {self._ident()} {r.randint(1, 9)}.{r.randint(0, 30)}."
        return t, si, m["topic"]


def idents_of(text: str) -> str:
    """The identifier-like tokens of a text, space-separated (approach e's column)."""
    return " ".join(w for w in _words(text) if _is_ident(w))


def pick(r: random.Random, mix):
    x, acc = r.random(), 0.0
    for *v, w in mix:
        acc += w
        if x < acc:
            return v if len(v) > 1 else v[0]
    return mix[-1][:-1] if len(mix[-1]) > 2 else mix[-1][0]


@dataclass
class Tree:
    universe: uuid.UUID
    projects: list[uuid.UUID]
    leaves: dict[uuid.UUID, list[uuid.UUID]]  # project -> leaves

    def all(self) -> list[uuid.UUID]:
        return [self.universe, *self.projects, *(l for ls in self.leaves.values() for l in ls)]

    def leaf_list(self) -> list[uuid.UUID]:
        return [l for ls in self.leaves.values() for l in ls]


def make_tree(r: random.Random, n: int) -> Tree:
    """universe -> projects -> leaves; about one workarea per 2,000 memories,
    at least 20 (10k: 20, 100k: 50, 1M: 500)."""
    w = max(20, n // 2000)
    n_proj = max(4, w // 10)
    per = max(1, (w - 1 - n_proj) // n_proj)
    u = uuid.UUID(int=r.getrandbits(128), version=4)
    projects = [uuid.UUID(int=r.getrandbits(128), version=4) for _ in range(n_proj)]
    leaves = {p: [uuid.UUID(int=r.getrandbits(128), version=4) for _ in range(per)] for p in projects}
    return Tree(u, projects, leaves)


def readable_sets(r: random.Random, tree: Tree, count: int) -> list[tuple[str, list[str]]]:
    """Recall scopes as the lead computes them: subtree(scope) ∩ readable,
    plus universe (ADR-0044 d10). 40% a leaf, 40% a project, 20% universe."""
    out = []
    for _ in range(count):
        x = r.random()
        if x < 0.4:
            p = r.choice(tree.projects)
            s = ("leaf", [r.choice(tree.leaves[p]), tree.universe])
        elif x < 0.8:
            p = r.choice(tree.projects)
            s = ("project", [p, *tree.leaves[p], tree.universe])
        else:
            s = ("universe", tree.all())
        out.append((s[0], [str(u) for u in s[1]]))
    return out


def rows(n: int, tenant: uuid.UUID, tree: Tree, seed: int, keys: list[uuid.UUID]):
    """Yield memory rows (dicts) for one tenant. Workarea sizes are skewed:
    a leaf's weight is drawn from a Pareto, and 3% of rows live on project
    nodes, 1% on universe."""
    seeds = Seeds.load()
    g = Generator(seeds, n, seed)
    r = random.Random(seed + 1)
    leaves = tree.leaf_list()
    weights = [r.paretovariate(1.3) for _ in leaves]
    for i in range(n):
        text, si, topic = g.text()
        x = r.random()
        if x < 0.01:
            wa = tree.universe
        elif x < 0.04:
            wa = r.choice(tree.projects)
        else:
            wa = r.choices(leaves, weights)[0]
        status, is_tip = pick(r, STATUS_MIX)
        kind = pick(r, KIND_MIX)
        retirement = status == "promoted" and is_tip and kind != "guardrail" and r.random() < 0.01
        b = text.encode()
        yield {
            "id": uuid.UUID(int=r.getrandbits(128), version=4),
            "tenant_id": tenant,
            "workarea_id": wa,
            "claim": text,
            "claim_digest": hashlib.sha256(b).digest(),
            "status": status,
            "created_event_id": uuid.UUID(int=r.getrandbits(128), version=4),
            "claim_key": f"k{seed:x}-{i:x}",
            "author_principal": "agent",
            "author_key_id": r.choice(keys),
            "version": 1,
            "kind": kind,
            "is_tip": is_tip,
            "body_bytes": len(b),
            "retirement": retirement,
            "severity": "should" if kind == "guardrail" else None,
            "idents": idents_of(text),
            "seed_index": si,
        }


CATEGORIES = ["exact", "paraphrase", "inflection", "partial", "typo", "identifier",
              "question", "long", "negation", "temporal", "multihop"]


def queries(count: int, seed: int, n: int) -> list[dict]:
    """`count` queries, an equal share per eval category: each seed query
    as written, then variants with an identifier or number swapped through the
    same generator the corpus used (so identifier queries name identifiers
    the corpus holds)."""
    seeds = Seeds.load()
    g = Generator(seeds, n, seed)
    r = random.Random(seed + 7)
    by_cat: dict[str, list[str]] = {}
    for q in seeds.queries:
        by_cat.setdefault(q["category"], []).append(q["query"])
    per = -(-count // len(CATEGORIES))
    out = []
    for c in CATEGORIES:
        base = by_cat[c]
        for j in range(per):
            q = base[j % len(base)]
            if j >= len(base):
                if c == "identifier":
                    q = g._ident()
                else:
                    q = NUM.sub(g._num, " ".join(
                        g._ident() if _is_ident(w) and r.random() < 0.7 else w for w in q.split()))
            out.append({"category": c, "query": q})
    return out[:count] if count % len(CATEGORIES) == 0 else out


def new_texts(count: int, seed: int, n: int) -> list[str]:
    """Texts for writes (ADR-0050's check and the write-cost runs)."""
    g = Generator(Seeds.load(), n, seed)
    return [g.text()[0] for _ in range(count)]


if __name__ == "__main__":
    r = random.Random(1)
    t = make_tree(r, 10000)
    rs = list(rows(5, uuid.uuid4(), t, 1, [uuid.uuid4()]))
    for x in rs:
        print(x["status"], x["kind"], x["claim"], "|", x["idents"])
    for q in queries(33, 2, 10000):
        print(q)
