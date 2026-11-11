# ADR-0075 phase 0a: baselines and power on the grown set

## 1. Baselines on the grown set

| pipeline | n | success@5 | 95% interval (Wilson) | nDCG@10 | wins / losses vs `bm25-postings` | one-sided exact McNemar p |
|---|---|---|---|---|---|---|
| `bm25-postings` | 665 | 0.913 (607/665) | 0.889 to 0.932 | 0.869 | — | — |
| `bm25-rerank` | 665 | 0.941 (626/665) | 0.921 to 0.957 | 0.916 | 19 / 0 | < 0.0001 |
| `bm25-sparse` | 665 | 0.956 (636/665) | 0.938 to 0.969 | 0.918 | 31 / 2 | < 0.0001 |
| `bm25-sparse-rrf` | 665 | 0.947 (630/665) | 0.928 to 0.962 | 0.897 | 24 / 1 | < 0.0001 |

| pipeline | paraphrase success@5 (95% interval) | question success@5 (95% interval) |
|---|---|---|
| `bm25-postings` | 0.828 (251/303; 0.782 to 0.867) | 0.983 (356/362; 0.964 to 0.992) |
| `bm25-rerank` | 0.888 (269/303; 0.847 to 0.919) | 0.986 (357/362; 0.968 to 0.994) |
| `bm25-sparse` | 0.914 (277/303; 0.877 to 0.941) | 0.992 (359/362; 0.976 to 0.997) |
| `bm25-sparse-rrf` | 0.898 (272/303; 0.858 to 0.927) | 0.989 (358/362; 0.972 to 0.996) |

## 2. The superiority arm's power

n = 665; `bm25-postings` misses the top 5 on 58 queries (8.7%), so no arm can win more than 8.7% of queries.

| effect | wins p10 | losses p01 | gain | power at n (exact, 1.5-point floor) | n for 80% / 90% (exact) | Connor n for 80% / 90% |
|---|---|---|---|---|---|---|
| d8's planning effect | 0.0500 | 0.0170 | +0.0330 | 0.947 | 412 / 547 | 378 / 523 |
| `bm25-sparse` vs `bm25-postings`, measured here | 0.0466 | 0.0030 | +0.0436 | 1.000 | 176 / 219 | 159 / 220 |

## 3. Non-inferiority margins

| set | n | sigma of the nDCG@10 difference | m (uncapped) | m used | capped | power at a true difference of 0 |
|---|---|---|---|---|---|---|
| SciFact | 300 | 0.1945 | 0.0481 | 0.0200 | yes | 0.429 |
| NFCorpus | 323 | 0.1074 | 0.0256 | 0.0200 | yes | 0.917 |
| LoCoMo (10 conversations, 1535 queries) | 10 | 0.0235 (of the per-conversation means) | 0.0378 | 0.0200 | yes | 0.664 (sign-flip, 4,000 runs) |

## 4. Joint power

1 − (0.053 + 0.571 + 0.083 + 0.336 + 0.033) = **-0.076** at the planned effect, by the union bound over the superiority arm at d8's planning effect, the 3 non-inferiority arms measured here and d8's displacement arm (a negative bound says nothing: the arms' type II errors sum past 1); below 0.80, so by d8 phase 0 does not run yet. Growing the grown set raises only the superiority term; the non-inferiority terms are fixed by SciFact's, NFCorpus's and LoCoMo's own sizes and by the 0.02 cap on m.
