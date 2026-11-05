"""Shingle length and threshold sweep on the labelled set (ADR-0050 d17).

Exact Jaccard over word shingles of 1, 2 and 3 words, with d1's guard, no
MinHash: the ceiling each choice allows. The served check (one-word
shingles, 0.9, MinHash candidates) is measured by
crates/oml-domain/tests/near_duplicates.rs. Standard library only:

    python3 eval/remember/near-duplicates/sweep.py
"""

import json
import math
import re
from collections import Counter

SEP = re.compile(r"[\x01-\x2f\x3a-\x40\x5b-\x60\x7b-\x7f]+")


def words(t):
    return [w for w in SEP.split("".join(c.lower() if c.isascii() else c for c in t)) if w]


def shingles(ws, k):
    if len(ws) < k:
        return {" ".join(ws)} if ws else set()
    return {" ".join(ws[i:i + k]) for i in range(len(ws) - k + 1)}


def guarded(ws):
    chars = "".join(ws)
    if len(set(ws)) < 10 or not chars:
        return False
    n = len(chars)
    return -sum(c / n * math.log2(c / n) for c in Counter(chars).values()) >= 3.0


def main():
    d = json.load(open("eval/remember/near-duplicates/data.json"))
    same = {}
    for a, b in d["same_as"]:
        same.setdefault(a, set()).add(b)
        same.setdefault(b, set()).add(a)
    stored = [(r["id"], words(r["text"])) for r in d["stored"]]
    print("k  thr   " + "  ".join(f"{l:>12}" for l in
          ["copy", "near_copy", "value_change", "paraphrase", "update", "new", "short", "false"]))
    for k in (1, 2, 3):
        sets = [(i, set(shingles(ws, k)), ws, guarded(ws)) for i, ws in stored]
        for thr in (0.8, 0.85, 0.9):
            found, total, false = Counter(), Counter(), 0
            for p in d["probes"]:
                ws = words(p["text"])
                s, g = set(shingles(ws, k)), guarded(ws)
                ok = {p["target"]} | same.get(p["target"], set()) if p["target"] else set()
                total[p["label"]] += 1
                hit = False
                for i, t, tws, tg in sets:
                    named = ws == tws or (g and tg and len(s & t) / len(s | t) >= thr)
                    if named and i in ok and p["label"] not in ("new", "short"):
                        hit = True
                    elif named:
                        false += 1
                        if i in ok:
                            found[p["label"]] += 1
                found[p["label"]] += hit
            row = [f"{found[l]}/{total[l]}" for l in
                   ["copy", "near_copy", "value_change", "paraphrase", "update", "new", "short"]]
            print(f"{k}  {thr:.2f}  " + "  ".join(f"{c:>12}" for c in row) + f"  {false:>12}")


if __name__ == "__main__":
    main()
