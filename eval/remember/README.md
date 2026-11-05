# Remember eval

The recall eval (`eval/recall/`, branch `feat/recall-eval`) asks whether OML finds what was
written. This one asks the write side's three questions, the ones proposed ADR-0050 decides on:

1. **Duplicates.** When an agent writes a fact OML already holds, or a changed version of one,
   does the write path say so, naming the memory, so the agent supersedes instead of adding a
   second claim? Measured as the *hint recall* (the counterpart is among the `similar` rows),
   the *false alarm* rate (a new fact gets a hint anyway), *precision*, and the *residual
   duplicate rate*: the share of writes that still leave two live claims on one subject.
2. **Findability of a fresh write.** Once a memory is admitted, do paraphrased queries find it,
   with its text alone, with a title, and with the author's keywords? success@1, success@5 and
   the zero-result rate over the same store.
3. **"Latest".** Of two versions of a changed fact, does recall serve the current one? Measured
   as *latest correct@1* and *stale first* (the replaced value ranked first), with every version
   live (today's pile-up, two claims) against supersession (one claim, the tip served).

and a fourth, the advisory **quality warnings** (too long, several claims, vague, a relative
date with no date): the false-warning rate on well-formed memories and the share of planted
problems caught.

## Files

| file | what it is |
|---|---|
| `data.json` | 40 seed memories; 24 writes labelled `duplicate`, `update` (each with the seed it repeats or changes) or `novel`, each with an author's `title` and up to 8 `keywords`; 24 paraphrased queries for the writes admitted; 8 "latest" queries over the 8 changed facts; 8 planted low-quality texts |
| `run.py` | the offline harness, standard library only; `--check` validates the data |
| `results/offline.json` | the last offline run: every table below and every per-write and per-query ranking |

## Methods

Offline models of the served rules, never the server:

| name | models |
|---|---|
| `none` | today: nothing is checked at write |
| `digest` | the same NFC text already readable (the digest every write already computes) |
| `and_text` | today's recall run with the new text as the query: every word, `simple`, exact |
| `shared_terms>=θ` | the top 3 of an any-word BM25 recall with the new text as the query, kept when the row shares at least θ of the new text's distinct indexed terms: ADR-0048 d9's `matched.words / of`, a structural ratio, not a score |
| `trigram>=θ` | character-trigram Jaccard of the two texts at least θ |
| `served_and` | today's recall: every word must match |
| `text_any` | ADR-0048 S1–S2 modelled: any word, identifier parts, crude stemming, BM25 over the readable rows |
| `text_title` | the same, the title indexed beside the text at the text's weight |
| `text_title_keywords` | the same, keywords indexed at half the text's weight (ADR-0048 d6) |
| `pile_up` / `supersession` | the store with every version live, and the store with each update replacing its seed's tip |

The stemmer is a crude suffix stripper and stands in for PostgreSQL's `english`; BM25 uses k1
1.2 and b 0.75. The duplicate model assumes an agent that follows a correct hint: it writes
nothing for a duplicate and supersedes for an update. A missed pair leaves two live claims.

## First run, offline, 2026-11-04

```
python3 -I eval/remember/run.py
```

| similar hint | hint recall | false alarm | precision | residual duplicates |
|---|---:|---:|---:|---:|
| none (today) | 0.00 | 0.00 | — | 0.67 |
| digest | 0.06 | 0.00 | 1.00 | 0.62 |
| and_text | 0.06 | 0.00 | 1.00 | 0.62 |
| shared_terms ≥ 0.3 | 1.00 | 0.00 | 0.84 | 0.00 |
| shared_terms ≥ 0.4 | 1.00 | 0.00 | 0.94 | 0.00 |
| shared_terms ≥ 0.5 | 1.00 | 0.00 | 1.00 | 0.00 |
| trigram ≥ 0.3 | 1.00 | 0.00 | 1.00 | 0.00 |

| findability (24 paraphrases) | success@1 | success@5 | zero results |
|---|---:|---:|---:|
| served_and (today) | 0.00 | 0.00 | 1.00 |
| text_any | 0.50 | 0.67 | 0.04 |
| text_title | 0.58 | 0.67 | 0.04 |
| text_title_keywords | 0.71 | 0.88 | 0.00 |

| latest (8 changed facts) | latest correct@1 | stale first |
|---|---:|---:|
| pile_up (today, two claims) | 0.12 | 0.62 |
| supersession (one claim) | 0.75 | 0.00 |

Quality warnings: no false warning on the 64 well-formed texts, and all 8 planted problems
caught.

**Read these numbers as a smoke test and an order, never as sizes.** One model wrote the
seeds, the writes, the titles, the keywords, the queries, the labels and the warning rules, so
the keywords and the rules were written knowing the queries and the planted texts: the
`text_title_keywords` row and the quality line are upper bounds, and the novel writes share
fewer words with the seeds than a real crew's would, which flatters every hint's false-alarm
rate. With 24 writes and 24 queries, one item moves a rate by 0.04. Two findings hold whatever
the size: today's write path catches only a byte-identical repeat (`digest`, `and_text`: one
pair of 16), and with two live claims on one subject the replaced value ranks first for 5 of 8
"latest" queries, while under supersession it never can.

## The gate each decision of ADR-0050 must pass

Version 1 is too small and too self-written to accept anything; version 2, below, is the gate.
The same table is ADR-0050 d16. Each gate runs in CI against a live lead once W1 is built, as
the recall eval does (ADR-0048 d12); until then `run.py --check` and the offline run are the
job.

| ADR-0050 | gate on remember eval v2 |
|---|---|
| d1 similar hint (`on_similar: "hold"`) | hint recall ≥ 0.90 on duplicate and update writes, false alarm ≤ 0.20 on novel writes |
| d1 `on_similar: "refuse"` as the skill's default | precision ≥ 0.80 at hint recall ≥ 0.90 |
| d2–d4 upsert, revision and supersession | stale first = 0 and latest correct@1 not below the store's success@1 on the same queries; the structural tests of d14 |
| d5 title and keywords | text_title_keywords − text_any ≥ +0.10 success@5 on paraphrases, no category falling by more than one query, exact queries unchanged |
| d6 quality warnings | false warnings ≤ 0.10 on accepted memories, planted problems caught ≥ 0.80 |
| d8 batch | within-batch duplicates flagged at the same hint recall as d1 |

**Version 2** draws its writes from real ones: consented sessions of the maintainer's own crews,
whose held writes, the reviewer's verdicts (`duplicate_of`, d1) and the session that wrote them
give labelled duplicate and update pairs without a model choosing them. Its paraphrased queries
are written by someone who did not write the memories or their keywords, at least 30 per
category, and it adds held rows, workareas and steering rows, which version 1 lacks.
