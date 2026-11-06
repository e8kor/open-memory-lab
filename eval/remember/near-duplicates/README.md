# The labelled near-duplicate set

Version 1, 2026-11-05. It measures the mechanics of the write-time similar check of
[ADR-0050](../../../docs/adr/ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md)
d1 and d17: the same text, and a near copy at a word-set Jaccard of 0.9 or more found by MinHash
keys, with no model.

## What it holds

`build.py` (standard library, seed 20261105) writes `data.json` from two sets already in the
repository: the recall eval's 200 memories and the remember eval's 40 seeds are the stored rows,
and 1,121 probes are written against them.

| label | probes | how each was made | the check should |
|---|---:|---|---|
| `copy` | 240 | the stored text lower-cased, its last full stop dropped, two spaces added | name it `same_text` |
| `near_copy` | 714 | one transposed-letter typo in a word of six letters or more (240), one stopword dropped (234), or `Note: ` prefixed (240) | name it |
| `value_change` | 137 | one number in the text moved by one | name it where 90% of the words survive (a replace signal), else not |
| `paraphrase` | 8 | the remember eval's `duplicate` writes, a model's restatement | not expected: other words |
| `update` | 8 | the remember eval's `update` writes, a model's changed value in other words | not expected |
| `new` | 8 | the remember eval's `new` writes | name nothing |
| `short` | 6 | short pairs that differ by one value, by order, or repeat one word | name nothing |

`same_as` lists the two stored pairs that are copies of each other in the source sets (m055 and
s09, m150 and s13), found by the check and confirmed by reading them, so a probe of one may name
the other.

**Limits.** The probes were derived by rule from texts one model wrote, so they measure what the
check does with copies and small edits, not how often real writers repeat themselves. The
paraphrases are the only model-written probes, and the check is not built to find them. Real
writes, labelled by reviewers' `duplicate_of` verdicts, are version 2 of the remember eval and
#141's question.

## Results

The served check, `crates/oml-domain/tests/near_duplicates.rs`, run by `cargo test` in CI, with the
gates of ADR-0050 d16 asserted:

| label | named | rate |
|---|---:|---:|
| copy | 240 / 240 (all `same_text`) | 1.00 |
| near_copy | 613 / 714 | 0.86 |
| — typo | 139 / 240 | 0.58 |
| — stopword dropped | 234 / 234 | 1.00 |
| — prefix | 240 / 240 | 1.00 |
| value_change | 88 / 137 | 0.64 |
| paraphrase | 1 / 8 | 0.13 |
| update | 0 / 8 | 0.00 |
| new | 0 / 8 named | — |
| short | 0 / 6 named | — |

Of 952 rows named, none was a wrong row: precision 1.00. A typo or a changed value replaces one
word, so it keeps 0.9 only in a text of 19 distinct words or more; the misses are the shorter
texts. The MinHash candidates lost nothing: every pair whose exact ratio passes 0.9 was a
candidate.

`sweep.py` gives the ceiling of other shingle lengths and thresholds (exact Jaccard, no MinHash):

| shingle | 0.80 | 0.85 | 0.90 |
|---|---:|---:|---:|
| one word | 714 / 714 | 713 / 714 | **613 / 714** |
| two words | 646 / 714 | 445 / 714 | 288 / 714 |
| three words | 350 / 714 | 292 / 714 | 277 / 714 |

None named a wrong row at any setting. One-word shingles at 0.9 are served.

```sh
python3 eval/remember/near-duplicates/build.py > eval/remember/near-duplicates/data.json
python3 eval/remember/near-duplicates/sweep.py
cargo test -p oml-domain --test near_duplicates -- --nocapture
```
