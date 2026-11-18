---
status: Measured
verified: 2026-11-06
answers: "how recall latency grows with the rows in a recall's scope, per search pipeline, against the 50 ms line"
---

# Latency at scale

*These are measurements by the OML maintainers on their own hardware, not independent results. Generated from the committed `oml-bench/1` results.*

[All suites](index.md)

The ten-fold SciFact corpus (about 50,000 memories) is the largest suite. The default pipeline, `bm25-postings`, has not been run at that size in this suite. At the database it measured 25.5 ms p95 at 99,721 readable memories (`bench/scale/REPORT.md`).

Each point is one suite's run: the rows in the recall's scope (for LoCoMo, all conversations together, though each query reads one) and the client's p50 and p95 in milliseconds. The line is 50 ms.

**Why latency grows with the rows.** Under row security no text index serves a recall. Every pipeline except `bm25-postings` therefore reads the stored text vectors of every row the key may read, and its cost grows with those rows. Stored vectors made a row several times cheaper, not free. An every-word match crossed 50 ms near 70,000 readable rows (`bench/scale/REPORT-baseline.md`). `bm25` reads more per row, so on abstract-length text it crosses sooner, as the tables show. `bm25-postings` reads integer postings, which row security lets an index serve, so its cost follows the postings it reads. It measured 25.5 ms p95 at 99,721 readable memories, universe scope, against `bm25`'s 485 ms. That figure was measured at the database, not through the client, on 2026-11-05 on a host shared with other jobs, so treat it as an upper bound (`bench/scale/REPORT.md`).

**Not yet measured: the in-progress mark.** A search over a scope that holds an in-progress workspace admits every author's held row on that node, and the statement recognises such a row by computing a SHA-256 mark from each held row's `scanned_under` and `workspace_id` and comparing it with an array of marks (`held_authors` in `oml-pg`, ADR-0080 d16). The mark is computed per row, so no index can serve that predicate; the cost grows with the held rows on the readable nodes, which are few beside the promoted ones. Binding the in-progress nodes as an array and comparing `workspace_id = ANY($n) AND scanned_under IS NOT DISTINCT FROM $cur` would let an index serve it. That change renumbers the parameters of every search statement that shares the scope, so it is filed as #462 and not made here.

**Documents grow the index by their passages.** A document is searched by its passages ([ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md) d4), each a search unit with its own postings. On 2026-11-18, 19 of OML's own guides, 142,552 bytes of markdown, cut into 129 passages and added 16,678 postings beside the 5,309 of the recall eval's 200 memories: about 880 postings a document, 129 a passage, 120 a kilobyte of text. The postings partition, with its indexes, grew from 1.6 MiB to 7.5 MiB, and the passages' own table took 0.5 MiB. On a corpus with no documents, the scale lane's 10,000 memories, `bm25-postings` measured p50 7.2 ms and p95 13.1 ms after the change against 6.6 and 11.5 ms before, and `bm25` p95 35.8 against 31.9 ms, medians of 9 and 6 rounds on a host shared with other jobs (load average 2.4 to 3.6), the driver a debug build on both sides. The numbers are in `bench/scale/results/passages/`. Recall over many large documents is not yet measured.

| method | rows | suite | p50 ms | p95 ms | p95 under 50 ms |
|---|---:|---|---:|---:|---|
| keyword-all | 200 | recall-synthetic | 14.2 | 18.6 | yes |
| keyword-all | 3,633 | beir-nfcorpus | 477 | 513 | no |
| keyword-all | 5,183 | beir-scifact | 629 | 672 | no |
| keyword-all | 5,882 | locomo | 31.4 | 41.3 | yes |
| keyword-all | 51,830 | beir-scifact-x10 | 2,142 | 2,195 | no |
| keyword-any | 200 | recall-synthetic | 18.9 | 29.7 | yes |
| keyword-any | 3,633 | beir-nfcorpus | 586 | 1,016 | no |
| keyword-any | 5,183 | beir-scifact | 815 | 1,075 | no |
| keyword-any | 5,882 | locomo | 62.2 | 86.4 | no |
| keyword-any | 51,830 | beir-scifact-x10 | 4,278 | 4,670 | no |
| bm25 | 200 | recall-synthetic | 16.3 | 21.2 | yes |
| bm25 | 3,633 | beir-nfcorpus | 51.8 | 83.8 | no |
| bm25 | 5,183 | beir-scifact | 86.1 | 116 | no |
| bm25 | 5,882 | locomo | 31.6 | 39.2 | yes |
| bm25 | 51,830 | beir-scifact-x10 | 604 | 849 | no |
| bm25-trigram | 200 | recall-synthetic | 30.4 | 41.9 | yes |
| bm25-trigram | 3,633 | beir-nfcorpus | 2,172 | 3,047 | no |
| bm25-trigram | 5,183 | beir-scifact | 3,707 | 5,454 | no |
| bm25-trigram | 5,882 | locomo | 89.1 | 112 | no |
| bm25-trigram | 51,830 | beir-scifact-x10 | 13,271 | 19,248 | no |
| bm25-postings | 200 | recall-synthetic | 10.1 | 48.0 | yes |
| bm25-postings | 200 | recall-grown | 23.5 | 32.5 | yes |
| bm25-postings | 3,633 | beir-nfcorpus | 38.0 | 54.8 | no |
| bm25-postings | 5,183 | beir-scifact | 46.2 | 65.6 | no |
| bm25-postings | 5,882 | locomo | 29.9 | 37.0 | yes |
| bm25-rerank | 200 | recall-synthetic | 128 | 215 | no |
| bm25-rerank | 200 | recall-grown | 142 | 240 | no |
| sparse | 200 | recall-synthetic | 9.3 | 15.4 | yes |
| sparse | 3,633 | beir-nfcorpus | 35.7 | 49.4 | yes |
| sparse | 5,183 | beir-scifact | 36.4 | 50.5 | no |
| sparse | 5,882 | locomo | 16.0 | 24.6 | yes |
| bm25-sparse | 200 | recall-synthetic | 12.5 | 18.7 | yes |
| bm25-sparse | 200 | recall-grown | 42.5 | 49.7 | yes |
| bm25-sparse | 3,633 | beir-nfcorpus | 48.5 | 65.9 | no |
| bm25-sparse | 5,183 | beir-scifact | 52.6 | 83.7 | no |
| bm25-sparse | 5,882 | locomo | 23.5 | 32.6 | yes |
| bm25-sparse-rrf | 200 | recall-grown | 38.3 | 48.1 | yes |
| bm25-sparse-rrf | 3,633 | beir-nfcorpus | 34.4 | 58.7 | no |
| bm25-sparse-rrf | 5,183 | beir-scifact | 50.5 | 89.6 | no |
| bm25-sparse-rrf | 5,882 | locomo | 45.4 | 72.2 | no |
| bm25-k10 | 5,183 | beir-scifact | 70.5 | 98.5 | no |
| bm25-k10 | 51,830 | beir-scifact-x10 | 572 | 804 | no |
| bm25-rerank-d20 | 3,633 | beir-nfcorpus | 5,247 | 15,467 | no |
| bm25-rerank-d20 | 5,183 | beir-scifact | 4,563 | 5,711 | no |
| bm25-rerank-d20 | 5,882 | locomo | 2,522 | 3,367 | no |
| bm25-sampled | 3,633 | beir-nfcorpus | 69.5 | 140 | no |
| bm25-sampled | 5,183 | beir-scifact | 102 | 140 | no |
| bm25-sampled | 5,882 | locomo | 57.6 | 77.5 | no |
| bm25-trigram-k10 | 5,183 | beir-scifact | 3,664 | 5,334 | no |
| keyword-any-k10 | 5,183 | beir-scifact | 630 | 1,090 | no |
