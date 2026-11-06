"""Build the labelled near-duplicate set (ADR-0050 d17), version 1.

Standard library only, deterministic (seed 20261105). The stored rows are the
recall eval's 200 memories and the remember eval's 40 seeds; every probe is
derived from one stored row by a rule, or taken from the remember eval's
labelled writes, so no label here was chosen by a model:

  copy          the text lower-cased, its last full stop dropped, spaces added
  near_copy     one transposed-letter typo in a word of six letters or more;
                one stopword dropped; or "Note: " prefixed
  value_change  one number in the text moved by one
  paraphrase    the remember eval's `duplicate` writes (a model's restatement)
  update        the remember eval's `update` writes (a model's changed value)
  new           the remember eval's `new` writes, which name no stored row
  short         six short pairs, stored and probe, that differ by one value
                or by order, or repeat one word

`same_as` lists stored pairs that are copies of each other in the source sets
(found by the check and confirmed by reading them), so a probe of one may
name the other. Run from the repository root:

    python3 eval/remember/near-duplicates/build.py > eval/remember/near-duplicates/data.json
"""

import json
import random
import re

SEED = 20261105
STOP = {"the", "a", "an", "that", "of", "to", "and", "is", "in"}
SHORT = [
    ("Use port 8080.", "Use port 8081."),
    ("Deploy on Tuesday.", "Deploy on Thursday."),
    ("Port 8080, not 8081.", "Port 8081, not 8080."),
    ("Tests pass on main.", "Tests fail on main."),
    ("The API answers 404 for a missing key.", "The API answers 403 for a missing key."),
    (
        "yes yes yes yes yes yes yes yes yes yes yes yes",
        "yes yes yes yes yes yes yes yes yes yes yes no",
    ),
]
SAME_AS = [["m055", "s09"], ["m150", "s13"]]


def main():
    recall = json.load(open("eval/recall/corpus.json"))["memories"]
    remember = json.load(open("eval/remember/data.json"))
    stored = [{"id": m["id"], "text": m["text"]} for m in recall]
    stored += [{"id": s["id"], "text": s["text"]} for s in remember["seeds"]]
    rng = random.Random(SEED)
    probes = []

    def add(label, target, text, rule):
        probes.append(
            {"id": f"p{len(probes) + 1:04d}", "label": label, "target": target,
             "rule": rule, "text": text}
        )

    for row in stored:
        t, words = row["text"], row["text"].split(" ")
        add("copy", row["id"], t.lower().rstrip(".") + "  ", "lower-case, no full stop")
        long_words = [i for i, w in enumerate(words) if len(w) > 5 and w.isalpha()]
        if long_words:
            i = rng.choice(long_words)
            w = words[i]
            j = rng.randrange(1, len(w) - 2)
            typo = w[:j] + w[j + 1] + w[j] + w[j + 2:]
            add("near_copy", row["id"], " ".join(words[:i] + [typo] + words[i + 1:]), "typo")
        stops = [i for i, w in enumerate(words) if w.lower() in STOP]
        if stops:
            i = rng.choice(stops)
            add("near_copy", row["id"], " ".join(words[:i] + words[i + 1:]), "stopword dropped")
        add("near_copy", row["id"], "Note: " + t, "prefix")
        numbers = list(re.finditer(r"\d+", t))
        if numbers:
            m = rng.choice(numbers)
            add("value_change", row["id"], t[: m.start()] + str(int(m.group()) + 1) + t[m.end():],
                "number + 1")
    for w in remember["writes"]:
        label = {"duplicate": "paraphrase", "update": "update"}.get(w["label"], "new")
        add(label, w.get("of") if label != "new" else None, w["text"], "remember eval " + w["id"])
    for n, (a, b) in enumerate(SHORT, 1):
        stored.append({"id": f"short{n}", "text": a})
        add("short", f"short{n}", b, "short pair")
    json.dump(
        {"version": 1, "seed": SEED, "note": __doc__.strip().splitlines()[0],
         "same_as": SAME_AS, "stored": stored, "probes": probes},
        __import__("sys").stdout, indent=1, ensure_ascii=False,
    )
    print()


if __name__ == "__main__":
    main()
