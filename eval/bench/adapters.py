"""Dataset adapters: each turns a benchmark into memories, queries and qrels.

An adapter's `load(cache, seed, limit)` returns a `Bench`:

- `docs`: the memories to write, each `Doc(id, text, group)`. One doc is one
  memory, written as kind `memory` under claim_key `bench-<dataset>-<id>`.
- `queries`: `Query(id, text, group, category)`. A query is asked only of the
  workarea of its group, so a benchmark built of many separate haystacks
  (LoCoMo's ten conversations) keeps them apart: each group is its own
  workarea `bench-<dataset>-<group>`, and a dataset with one haystack has
  the single group "" and the workarea `bench-<dataset>`.
- `qrels`: `{query_id: {doc_id: grade}}`, grade >= 1 relevant.
- `success_rel`: the grade a hit needs to count for success@k (1 for most;
  2 for the synthetic set, whose grade 2 means "answers it").

Datasets are read from `cache`, never from the repository: BEIR through
ir_datasets (IR_DATASETS_HOME is pointed under the cache), LoCoMo from the
file its repository publishes. Licenses differ, so nothing downloaded is ever
committed (README.md, Datasets).
"""

import json
import os
import random
import re
import sys
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent


@dataclass
class Doc:
    id: str
    text: str
    group: str = ""


@dataclass
class Query:
    id: str
    text: str
    group: str = ""
    category: str = "all"


@dataclass
class Bench:
    name: str
    docs: list
    queries: list
    qrels: dict
    success_rel: int = 1
    notes: list = field(default_factory=list)


def workarea(dataset, group):
    return f"bench-{dataset}" + (f"-{group}" if group else "")


def safe(s):
    """A claim_key-safe spelling of an id."""
    return re.sub(r"[^A-Za-z0-9._-]", "_", s)


def subsample_queries(queries, qrels, seed, limit):
    """At most `limit` queries, drawn with a fixed seed; the corpus is kept whole."""
    queries = [q for q in queries if qrels.get(q.id)]
    if limit and len(queries) > limit:
        queries = sorted(random.Random(seed).sample(queries, limit), key=lambda q: q.id)
    return queries


class Synthetic:
    """eval/recall's 200 memories and 121 queries, through this harness."""

    name = "synthetic"
    groups = [""]

    def load(self, cache, seed, limit):
        sys.path.insert(0, str(HERE.parent / "recall"))
        import metrics  # eval/recall/metrics.py

        memories, queries = metrics.load()
        docs = [Doc(m["id"], m["text"]) for m in memories]
        qs = [Query(q["id"], q["query"], "", q["category"]) for q in queries]
        qrels = {q["id"]: dict(q["relevant"]) for q in queries}
        return Bench(self.name, docs, subsample_queries(qs, qrels, seed, limit), qrels,
                     success_rel=2)


class Beir:
    """A BEIR dataset's test split through ir_datasets; one document is one memory.

    The memory's text is the title, a full stop, and the abstract, the way
    BEIR's dense baselines join them; BEIR's BM25 (Anserini) indexes the two
    as separate fields instead. Every test query with a relevant document
    is asked; queries are not subsampled unless --limit says so.
    """

    groups = [""]

    def __init__(self, name, ir_id):
        self.name = name
        self.ir_id = ir_id

    def load(self, cache, seed, limit):
        os.environ.setdefault("IR_DATASETS_HOME", str(Path(cache) / "ir_datasets"))
        import ir_datasets

        ds = ir_datasets.load(self.ir_id)
        docs = []
        for d in ds.docs_iter():
            title = (getattr(d, "title", "") or "").strip()
            text = d.text.strip()
            body = f"{title}. {text}" if title and text else (title or text)
            if body:
                docs.append(Doc(d.doc_id, body))
        qrels = {}
        for r in ds.qrels_iter():
            if r.relevance > 0:
                qrels.setdefault(r.query_id, {})[r.doc_id] = r.relevance
        qs = [Query(q.query_id, q.text) for q in ds.queries_iter()]
        return Bench(self.name, docs, subsample_queries(qs, qrels, seed, limit), qrels)


class Scaled:
    """A BEIR set grown to `copies` times its corpus in one workarea, for latency at scale.

    Copy 0 is the set as published; copy i (1..copies-1) replaces each word of
    every document with probability 0.1 by a word drawn from the corpus's own
    vocabulary with the seed 20261104 + i, so the copies are near duplicates
    with statistics of their own rather than exact ones. The queries and qrels
    are the set's own and name copy-0 documents only, so the quality numbers
    measure ranking among nine distractors per relevant document; the run is
    for latency, and its quality is reported beside the 1x run, not instead of
    it.
    """

    groups = [""]

    def __init__(self, name, base, copies):
        self.name, self.base, self.copies = name, base, copies

    def load(self, cache, seed, limit):
        b = self.base.load(cache, seed, limit)
        vocab = sorted({w for d in b.docs for w in d.text.split()})
        docs = list(b.docs)
        for i in range(1, self.copies):
            rnd = random.Random(20261104 + i)
            for d in b.docs:
                text = " ".join(w if rnd.random() > 0.1 else rnd.choice(vocab)
                                for w in d.text.split())
                docs.append(Doc(f"{d.id}~c{i}", text))
        return Bench(self.name, docs, b.queries, b.qrels,
                     notes=[f"{self.copies} copies of {b.name}, copies 1-{self.copies - 1} "
                            "with 10% of words replaced; qrels name copy 0 only"])


LOCOMO_URL = "https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json"
# The LoCoMo repository's category numbers, as its evaluation code and the
# papers that report per-category results read them.
LOCOMO_CATEGORIES = {1: "multi-hop", 2: "temporal", 3: "open-domain", 4: "single-hop",
                     5: "adversarial"}


class Locomo:
    """LoCoMo's ten conversations; one dialogue turn is one memory.

    A turn's memory is `[<session date>] <speaker>: <text>`, with a shared
    photo's caption appended as `[shares a photo: <caption>]`: the date is
    what the temporal questions ask about and what a memory written at the
    time would carry. Each conversation is its own workarea, and each
    question is asked only of its own conversation. The qrels are the
    question's `evidence` dialogue ids. Category 5 (adversarial: the question
    presumes something the conversation never says) keeps its evidence
    labels, the turns the false premise is built from; it is reported as its
    own category and left out of the headline, which is categories 1-4.
    """

    name = "locomo"

    def __init__(self):
        self.groups = None

    def path(self, cache):
        p = Path(cache) / "locomo" / "locomo10.json"
        if not p.exists():
            p.parent.mkdir(parents=True, exist_ok=True)
            urllib.request.urlretrieve(LOCOMO_URL, p)
        return p

    def load(self, cache, seed, limit):
        data = json.loads(self.path(cache).read_text())
        docs, qs, qrels = [], [], {}
        for conv in data:
            group = safe(conv["sample_id"])
            c = conv["conversation"]
            ids = set()
            n = 1
            while f"session_{n}" in c:
                date = c.get(f"session_{n}_date_time", "")
                for t in c[f"session_{n}"]:
                    text = f"[{date}] {t['speaker']}: {t['text']}"
                    if t.get("blip_caption"):
                        text += f" [shares a photo: {t['blip_caption']}]"
                    did = f"{group}.{t['dia_id']}"
                    ids.add(did)
                    docs.append(Doc(did, text, group))
                n += 1
            for i, q in enumerate(conv["qa"]):
                qid = f"{group}.q{i}"
                rel = {}
                for e in q.get("evidence") or []:
                    # A few evidence entries join two ids ("D8:6; D9:17").
                    for one in re.split(r"[;,\s]+", e):
                        did = f"{group}.{one.strip()}"
                        if did in ids:
                            rel[did] = 1
                if not rel:
                    continue
                qrels[qid] = rel
                qs.append(Query(qid, q["question"], group,
                                LOCOMO_CATEGORIES.get(q["category"], str(q["category"]))))
        self.groups = sorted({d.group for d in docs})
        return Bench(self.name, docs, subsample_queries(qs, qrels, seed, limit), qrels,
                     notes=["headline over categories multi-hop, temporal, open-domain, "
                            "single-hop; adversarial reported apart"])


ADAPTERS = {
    "synthetic": Synthetic,
    "scifact": lambda: Beir("scifact", "beir/scifact/test"),
    "nfcorpus": lambda: Beir("nfcorpus", "beir/nfcorpus/test"),
    "arguana": lambda: Beir("arguana", "beir/arguana"),
    "locomo": Locomo,
    "scifact-x10": lambda: Scaled("scifact-x10", Beir("scifact", "beir/scifact/test"), 10),
}

# Published BM25 numbers for a sanity check: BEIR (Thakur et al., 2021,
# arXiv:2104.08663), Table 2 (nDCG@10) and Appendix Table 9 (Recall@100),
# Anserini BM25 with k1 0.9, b 0.4, title and text as separate fields.
PUBLISHED_BM25 = {
    "scifact": {"nDCG@10": 0.665, "R@100": 0.908},
    "nfcorpus": {"nDCG@10": 0.325, "R@100": 0.250},
    "arguana": {"nDCG@10": 0.315, "R@100": 0.942},
}
