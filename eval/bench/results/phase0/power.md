# ADR-0075 d8 as amended on 2026-11-11: the gate's power

Primary and secondary: n 687 question-shaped queries, one-sided exact McNemar at 0.025, floor 1 point. `bm25-sparse` misses the top 5 on 31 of them (4.5%), the most any arm can win.

| planning discordance | pi_d | least gain for 80% power (exact) | for 90% (exact) | Connor, 80% |
|---|---|---|---|---|
| `bm25-sparse` vs `bm25-postings` (34 of 687) | 0.0495 | 2.5 points | 2.8 points | 2.4 points |
| `bm25-sparse` vs `bm25-rerank` (19 of 687) | 0.0277 | 1.9 points | 2.1 points | 1.8 points |
| `bm25-sparse` vs `bm25-sparse-rrf` (10 of 687) | 0.0146 | 1.4 points | not reachable | 1.3 points |

At the planning discordance 0.0495, a 2.5-point gain is detected with power 0.813.

Non-inferiority: n 99 other queries, planning pi_d 0.0505 (5 of 99).

| power at a true difference of 0 | m |
|---|---|
| 0.80 | 0.0633 |
| 0.90 | 0.0732 |
| 0.95 | 0.0814 |
