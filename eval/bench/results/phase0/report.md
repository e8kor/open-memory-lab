# ADR-0075 phase 0: generated questions against `bm25-sparse`

**Labels checked.** The operator checked a seeded tenth of the grown set's labels (67) on 2026-11-11: 58 marked correct, 6 unsure, 3 wrong; 1 label corrected (g0113, marked unsure, now labels m033 grade 2, m037 grade 2, m038 grade 2 as the operator chose). This run uses version 2, with the correction (`person_check` in `eval/recall/queries-grown.json`). Offline: no server was built or run; the lexemes are migration 0020's, computed in a throwaway PostgreSQL, and the gate is ADR-0075 d12 (the operator's decision of 2026-11-11).

Generator: `doc2query/msmarco-t5-base-v1` at `e673dca0dff1` (weights SHA-256 `f0a1ec383a679613…`, Apache-2.0 (weights); trained on MS MARCO (terms unchecked, d5's register)), top-k 10, at most 64 new tokens, ten drawn per memory, the first five the arm. Sparse leg: opensearch-neural-sparse-encoding-doc-v3-distill at `babf71f3`, as served.

## Parity with the served runs

| pipeline | queries whose success@5 matches the served run | top 1 matches |
|---|---|---|
| `bm25-postings` | 786 of 786 | 780 of 786 |
| `bm25-sparse` | 784 of 786 | 775 of 786 |

`bm25-sparse` differs from the served run on g0468, g0590.

## Success@5

| pipeline | all (786) | question-shaped (687) | other (99) | nDCG@10, question-shaped |
|---|---|---|---|---|
| `bm25-postings` | 0.916 (720/786; 0.895 to 0.933) | 0.911 (626/687; 0.888 to 0.930) | 0.950 (94/99; 0.887 to 0.978) | 0.862 |
| `sparse` | 0.958 (753/786; 0.942 to 0.970) | 0.953 (655/687; 0.935 to 0.967) | 0.990 (98/99; 0.945 to 0.998) | 0.904 |
| `bm25-sparse` | 0.963 (757/786; 0.948 to 0.974) | 0.958 (658/687; 0.940 to 0.971) | 1.000 (99/99; 0.963 to 1.000) | 0.913 |
| `bm25-questions` | 0.927 (729/786; 0.907 to 0.944) | 0.926 (636/687; 0.904 to 0.943) | 0.939 (93/99; 0.874 to 0.972) | 0.869 |
| `bm25-sparse-questions` | 0.964 (758/786; 0.949 to 0.975) | 0.959 (659/687; 0.942 to 0.972) | 1.000 (99/99; 0.963 to 1.000) | 0.915 |
| `bm25-questions.strict` | 0.921 (724/786; 0.900 to 0.938) | 0.917 (630/687; 0.894 to 0.935) | 0.950 (94/99; 0.887 to 0.978) | 0.869 |
| `bm25-sparse-questions.strict` | 0.963 (757/786; 0.948 to 0.974) | 0.958 (658/687; 0.940 to 0.971) | 1.000 (99/99; 0.963 to 1.000) | 0.913 |
| `bm25-questions.ten` | 0.933 (733/786; 0.913 to 0.948) | 0.930 (639/687; 0.909 to 0.947) | 0.950 (94/99; 0.887 to 0.978) | 0.874 |
| `bm25-sparse-questions.ten` | 0.966 (759/786; 0.951 to 0.976) | 0.961 (660/687; 0.943 to 0.973) | 1.000 (99/99; 0.963 to 1.000) | 0.917 |
| `bm25-questions.unfiltered` | 0.931 (732/786; 0.911 to 0.947) | 0.930 (639/687; 0.909 to 0.947) | 0.939 (93/99; 0.874 to 0.972) | 0.876 |
| `bm25-sparse-questions.unfiltered` | 0.964 (758/786; 0.949 to 0.975) | 0.959 (659/687; 0.942 to 0.972) | 1.000 (99/99; 0.963 to 1.000) | 0.916 |

Per category (success@5):

| category | n | `bm25-postings` | `sparse` | `bm25-sparse` | `bm25-questions` | `bm25-sparse-questions` |
|---|---|---|---|---|---|---|
| exact | 11 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| identifier | 11 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| inflection | 11 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| long | 11 | 1.000 | 0.909 | 1.000 | 1.000 | 1.000 |
| multihop | 11 | 0.909 | 1.000 | 1.000 | 0.909 | 1.000 |
| negation | 11 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| paraphrase | 314 | 0.825 | 0.911 | 0.917 | 0.857 | 0.920 |
| partial | 11 | 1.000 | 1.000 | 1.000 | 0.909 | 1.000 |
| question | 373 | 0.984 | 0.989 | 0.992 | 0.984 | 0.992 |
| temporal | 11 | 0.818 | 1.000 | 1.000 | 0.818 | 1.000 |
| typo | 11 | 0.818 | 1.000 | 1.000 | 0.818 | 1.000 |

## The gate (d12)

One-sided exact McNemar on success@5 at 0.025 per comparison, a 1-point floor; the gain with its 95% paired bootstrap interval (10,000 resamples).

| arm | comparison | n | wins / losses | gain (95% interval) | p | nDCG@10 gain |
|---|---|---|---|---|---|---|
| primary | `bm25-questions` vs `bm25-sparse` | 687 | 2 / 24 | -3.2 points (-4.7 to -1.9) | 1.0000 | -4.4 |
| secondary | `bm25-sparse-questions` vs `bm25-sparse` | 687 | 3 / 2 | +0.1 points (-0.4 to +0.7) | 0.5000 | +0.2 |
| non-inferiority, other, m 0.073 | `bm25-questions` vs `bm25-postings` | 99 | 0 / 1 | -1.0 points (-3.0 to +0.0) | 1.0000 | -0.5 |
| non-inferiority, other, m 0.073 | `bm25-sparse-questions` vs `bm25-sparse` | 99 | 0 / 0 | +0.0 points (+0.0 to +0.0) | 1.0000 | -0.5 |
| reported | `bm25-questions` vs `bm25-sparse` | 99 | 0 / 6 | -6.1 points (-11.1 to -2.0) | 1.0000 | -4.1 |
| reported | `bm25-questions` vs `bm25-postings` | 786 | 13 / 4 | +1.1 points (+0.1 to +2.2) | 0.0245 | +0.5 |
| reported | `bm25-questions` vs `bm25-postings` | 687 | 13 / 3 | +1.5 points (+0.4 to +2.6) | 0.0106 | +0.7 |
| reported | `bm25-questions.strict` vs `bm25-sparse` | 687 | 1 / 29 | -4.1 points (-5.7 to -2.6) | 1.0000 | -4.5 |
| reported | `bm25-questions.ten` vs `bm25-sparse` | 687 | 3 / 22 | -2.8 points (-4.2 to -1.5) | 1.0000 | -3.9 |
| reported | `bm25-questions.unfiltered` vs `bm25-sparse` | 687 | 4 / 23 | -2.8 points (-4.2 to -1.3) | 1.0000 | -3.7 |
| reported | `bm25-sparse-questions` vs `bm25-sparse` | 786 | 3 / 2 | +0.1 points (-0.4 to +0.8) | 0.5000 | +0.1 |
| reported | `bm25-sparse-questions.strict` vs `bm25-sparse` | 687 | 1 / 1 | +0.0 points (-0.4 to +0.4) | 0.7500 | -0.0 |
| reported | `bm25-sparse-questions.ten` vs `bm25-sparse` | 687 | 3 / 1 | +0.3 points (-0.3 to +0.9) | 0.3125 | +0.4 |
| reported | `bm25-sparse-questions.unfiltered` vs `bm25-sparse` | 687 | 3 / 2 | +0.1 points (-0.4 to +0.7) | 0.5000 | +0.3 |

Primary: fails. Observed discordance 0.0378; at it the set detects a gain of 2.2 points with 80% power, and the observed difference is -3.2 points.
Secondary: fails. Observed discordance 0.0073 (3 wins, 2 losses): the 95% interval rules out an added gain above 0.7 points.

**Verdict: no go.** Neither comparison passes, so by d12 v0.7.1 recommends `bm25-sparse` for question-style use and #236 to #238 are parked with this evidence. 

## What the filters dropped (of 1,000 questions, five per memory)

| variant | shape | generic | grounding | near_duplicate | novel_cap | scan |
|---|---|---|---|---|---|---|
| arm | 35 | 30 | 73 | 307 | 0 | 0 |
| strict | 903 | 4 | 4 | 4 | 0 | 0 |
| ten | 98 | 49 | 139 | 704 | 0 | 0 |

`ten` draws ten per memory (2,000 questions). oml-scan (`oml-scan/1`, the `fp_rate` example) found nothing in any generated question or in the 200 memories, so no version drew a `source_finding`.

## Cost and size

| | measured |
|---|---|
| memories | 200 |
| generation, wall | 229 s for ten drawn per memory, 4 CPU threads (median 0.77 s per memory) |
| tokens in / out | 6,671 / 18,428 (ten drawn) |
| questions kept (arm) | 555 on 199 memories |
| question text | 17,913 bytes |
| kind 3 lexemes / kind 1 lexemes | 3,052 / 5,309 = 0.575 (budget: at most 1) |

w_q by five-fold cross-validation (per fold; the synthetic queries take the whole grown set's): `bm25-questions` {'0': 0.6, '1': 0.9, '2': 0.2, '3': 0.5, '4': 0.9} / 0.6; `bm25-sparse-questions` {'0': 0.4, '1': 0.9, '2': 0.4, '3': 0.4, '4': 0.4} / 0.4.

