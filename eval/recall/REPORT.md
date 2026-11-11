# Recall eval: the first measurement

**Run:** 2026-11-04, branch `feat/recall-eval` from `babb004` (v0.5.0), release
build, a throwaway PostgreSQL 18.6, one workarea, 200 promoted memories, 121
queries ([README](README.md) for the set and how to rerun it).

**Headline.** Served recall answers 17% of the queries (success@5 0.17) and
returns **nothing at all for 83% of them**. Every query it answers is one whose
words all appear verbatim in the memory. The cheapest fix measured, joining the
query words with OR instead of AND on the index that exists today, takes
success@5 to 0.73 and the zero-result rate to 0.05. Adding English stemming
gives 0.86, adding trigram matching gives 0.93, and a BM25 + embedding hybrid
gives 0.97.

## How recall matches and ranks today

`oml_read` with a `query` is recall (`docs/api/mcp-plugin.md`), the same
operation as `POST /v1/memories/recall`. The store runs one statement,
`RECALL_FTS` in `crates/adapters/oml-pg/src/lib.rs`:

```sql
SELECT m.id, m.body_bytes,
       ts_rank(to_tsvector('simple', m.claim), plainto_tsquery('simple', $3), 0) AS score
FROM memories m
WHERE m.tenant_id = $1 AND m.status = 'promoted' AND m.is_tip
  AND NOT m.retirement AND m.kind = ANY($4) AND m.workarea_id = ANY($2)
  AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', $3)
ORDER BY score DESC, m.id ASC LIMIT $5
```

What that means for a query:

- **Every word must match.** `plainto_tsquery` joins all query tokens with
  AND. One word the memory lacks, including "what", "the", "do" or "not",
  removes the memory from the results.
- **No stemming and no stop words.** The `simple` configuration lower-cases
  tokens and keeps them whole, so "deployment" does not match "deploy" and
  "what" is a required term. `docs/architecture/retrieval.md` (Text handling)
  chose `simple` on purpose, so that non-English text is never stemmed
  wrongly, and says the cost was unmeasured (#102). This eval measures it.
- **The tokenizer decides what an identifier is.** PostgreSQL's default
  parser splits `fct_mrr_daily` into `fct`, `mrr` and `daily`. It keeps
  `InvoiceNumberingTest.concurrentIssue` and `useInvoiceFilters.ts` whole as
  one `host` token, so the class name alone matches nothing. A URL followed by
  a full stop becomes one token that includes the full stop.
- **Ranking** is `ts_rank` with normalization 0 (raw term frequency, no length
  normalization), ties broken by row id, cut at `k`. Steering rows come first,
  are not ranked and are not cut, and `max_tokens` keeps the longest rank
  prefix that fits.
- **There is no vector search.** The tool description says so ("full-text
  search, the only search mode served"). `http-api.md` describes recall as
  "FTS + vector", which reads as if vectors were served; they are not.
- **The test double matches differently.** The in-memory store in
  `crates/oml-app/src/doubles.rs` keeps a row when *any* query word matches
  (`matched > 0`) and scores by how many match, which is OR semantics. The
  Postgres store keeps a row only when *every* word matches. The use-case
  tests in `oml-app` therefore exercise a recall that is more forgiving than
  the one served. `retrieval.md` notes that "equal order across stores is not
  asserted", but here the two stores do not even return the same rows.
- **The server's own instructions describe the limitation.** The MCP
  `initialize` text tells the agent: "Matching is exact and every word must
  match, without stemming, so a whole question finds nothing; … pass 2-4 exact
  keywords".

## Methods compared

All methods rank the same 200 memories and return at most `k = 10` hits.

| method | what it is |
|---|---|
| `oml` | served recall: `oml_read` over `POST /mcp` with the writer key, `k = 10`, `max_tokens = 2000` (no query was cut by the budget) |
| `oml_keywords` | the same, with stop words and `?` removed from each query: an agent that follows the server's instructions literally |
| `pg_simple_and` | the served statement run directly against the database. It reproduces `oml` exactly, which confirms the harness |
| `pg_simple_or` | the same `simple` index, with the query terms joined by OR and ranked by `ts_rank_cd` |
| `pg_simple_and_or` | the served AND statement, falling back to `pg_simple_or` only when AND finds nothing |
| `pg_english_or` | `to_tsvector('english')` (stemming and stop words), with OR, ranked by `ts_rank_cd` |
| `pg_english_or_trgm` | `pg_english_or` fused by RRF (k = 60) with `pg_trgm` `word_similarity > 0.3` |
| `bm25` | BM25Okapi, Snowball English stems, English stop words dropped (`rank_bm25`) |
| `dense` | cosine similarity over `BAAI/bge-small-en-v1.5` embeddings (fastembed) |
| `hybrid_rrf` | RRF (k = 60) of the `bm25` and `dense` top 100 |

## Results

### Overall (n = 121)

| method | success@1 | success@5 | recall@10 | MRR | nDCG@10 | zero-result | median latency (ms) |
|---|---:|---:|---:|---:|---:|---:|---:|
| oml | 0.17 | 0.17 | 0.12 | 0.17 | 0.16 | 0.83 | 14.4 |
| oml_keywords | 0.24 | 0.24 | 0.18 | 0.24 | 0.21 | 0.75 | 13.9 |
| pg_simple_and | 0.17 | 0.17 | 0.12 | 0.17 | 0.16 | 0.83 | 0.2 |
| pg_simple_or | 0.49 | 0.73 | 0.64 | 0.59 | 0.59 | 0.05 | 1.0 |
| pg_simple_and_or | 0.49 | 0.73 | 0.63 | 0.59 | 0.59 | 0.05 | 0.9 |
| pg_english_or | 0.67 | 0.86 | 0.81 | 0.76 | 0.76 | 0.03 | 3.3 |
| pg_english_or_trgm | 0.75 | 0.93 | 0.90 | 0.82 | 0.83 | 0.00 | 12.1 |
| bm25 | 0.79 | 0.92 | 0.89 | 0.84 | 0.84 | 0.00 | 0.2 |
| dense | 0.79 | 0.95 | 0.91 | 0.85 | 0.85 | 0.00 | 4.3 |
| hybrid_rrf | 0.79 | 0.97 | 0.94 | 0.86 | 0.88 | 0.00 | 4.6 |

The latencies are not comparable with one another. `oml` is end to end: one
client, HTTP and MCP on loopback, authentication, the read log, and decryption
of the served bodies, p95 18.7 ms. The `pg_*` methods are a single SQL
statement over a local socket, and `bm25`, `dense` and `hybrid_rrf` run in
process. The trigram number comes from a sequential scan of 200 rows with no
trigram index, and says nothing about cost at scale.

95% bootstrap intervals for success@5 (2000 resamples over queries): `oml`
0.11–0.24, `pg_simple_or` 0.64–0.81, `pg_english_or` 0.80–0.92,
`pg_english_or_trgm` 0.88–0.98, `bm25` 0.87–0.96, `dense` 0.91–0.98,
`hybrid_rrf` 0.93–0.99. Paired nDCG@10 differences: trigram over
`pg_english_or` +0.04 to +0.12, `hybrid_rrf` over `pg_english_or_trgm` +0.02
to +0.07, `hybrid_rrf` over `bm25` +0.01 to +0.07, and `dense` over `bm25`
−0.04 to +0.06, which is no measurable difference.

### success@5 by category (n = 11 each)

| category | oml | oml_keywords | pg_simple_or | pg_english_or | pg_english_or_trgm | bm25 | dense | hybrid_rrf |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| paraphrase | 0.00 | 0.00 | 0.55 | 0.64 | 0.64 | 0.64 | 0.82 | 0.82 |
| inflection | 0.00 | 0.00 | 0.73 | 0.91 | 0.91 | 0.91 | 0.91 | 1.00 |
| partial | 0.00 | 0.00 | 0.64 | 0.73 | 1.00 | 0.82 | 1.00 | 0.91 |
| typo | 0.00 | 0.00 | 0.82 | 0.82 | 1.00 | 0.82 | 1.00 | 1.00 |
| identifier | 0.73 | 0.73 | 0.73 | 0.73 | 1.00 | 1.00 | 1.00 | 1.00 |
| question | 0.00 | 0.27 | 0.73 | 1.00 | 0.91 | 1.00 | 1.00 | 1.00 |
| long | 0.00 | 0.00 | 0.64 | 0.91 | 0.91 | 1.00 | 0.91 | 1.00 |
| negation | 0.18 | 0.46 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| temporal | 0.00 | 0.18 | 0.73 | 0.82 | 0.91 | 1.00 | 0.82 | 0.91 |
| multihop | 0.00 | 0.00 | 0.46 | 0.91 | 1.00 | 0.91 | 1.00 | 1.00 |
| **overall** | 0.17 | 0.24 | 0.73 | 0.86 | 0.93 | 0.92 | 0.95 | 0.97 |

### Zero-result rate by category

| category | oml | oml_keywords | pg_simple_or | pg_english_or | pg_english_or_trgm |
|---|---:|---:|---:|---:|---:|
| exact | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 |
| paraphrase | 1.00 | 1.00 | 0.00 | 0.00 | 0.00 |
| inflection | 1.00 | 1.00 | 0.00 | 0.00 | 0.00 |
| partial | 1.00 | 1.00 | 0.18 | 0.09 | 0.00 |
| typo | 1.00 | 1.00 | 0.09 | 0.00 | 0.00 |
| identifier | 0.27 | 0.27 | 0.27 | 0.27 | 0.00 |
| question | 1.00 | 0.64 | 0.00 | 0.00 | 0.00 |
| long | 1.00 | 1.00 | 0.00 | 0.00 | 0.00 |
| negation | 0.82 | 0.55 | 0.00 | 0.00 | 0.00 |
| temporal | 1.00 | 0.82 | 0.00 | 0.00 | 0.00 |
| multihop | 1.00 | 1.00 | 0.00 | 0.00 | 0.00 |

`bm25`, `dense` and `hybrid_rrf` never return zero results, because they
always rank something. [results/tables.md](results/tables.md) has success@1,
nDCG@10 and every method in every table. Per-query rankings are in
`results/<method>.json`.

## Failure taxonomy

Ten queries, what served recall returned, and why.

1. **A question finds nothing.** q067 "What is the maximum page size for the
   API?" returns no hits. The answer, m053 "The default page size is 50 and
   the maximum is 200 …", lacks "what", "for" and "API", and AND needs all
   of them. With the stop words removed (`oml_keywords`, "maximum page size
   API") it still returns nothing, because "API" is not in the memory. Every
   OR method ranks m053 first. Of the 11 questions, served recall answers
   none.
2. **A different word form finds nothing.** q023 "rollback deployment"
   returns no hits. m006 says "Rollbacks are done with helm rollback …,
   never by redeploying …", which has "rollback" but no "deployment".
   `pg_english_or`, the trigram fusion, `bm25`, `dense` and `hybrid_rrf` rank
   m006 first, and `pg_simple_or` ranks it second.
3. **One typo vetoes the whole query.** q047 "cert-manger renewal" returns
   no hits. With OR, the correct word "renewal" alone finds m011 at rank 1.
   That only works when the query has a correct word to lean on. q052
   "dunnig schedule" has none: `pg_english_or` returns m085, m136 and m158,
   which match "schedule" and have nothing to do with dunning. Only trigram
   (rank 4) and `dense` (rank 1) reach m150.
4. **An identifier inside a dotted name is invisible.** q063
   "InvoiceNumberingTest" returns no hits, and so do both OR variants. m110
   holds `InvoiceNumberingTest.concurrentIssue`, which the parser indexes as
   one `host` token. q060 "useInvoiceFilters" (stored as
   `useInvoiceFilters.ts`) fails the same way. q062, the image registry path,
   fails because the stored URL ends in a full stop and the full stop is part
   of the token. Trigram, BM25 and dense find all three at rank 1.
5. **A prefix or abbreviation finds nothing.** q036 "autoscal replicas"
   returns no hits, because m013 says "HorizontalPodAutoscaler". q034
   "postgres version" is worse once OR is on. `pg_english_or` returns m050,
   m105 and m116, which match only "version" (API versions, Gradle versions,
   release tags), because "postgres" is not a stem of "PostgreSQL". Trigram
   lifts m025 to rank 4.
6. **"Do not" is treated as search terms.** q091 "never use latest image
   tag" returns no hits. m012 says "Container images are tagged with the
   full git commit SHA, never with latest", which has "images" and "tagged"
   but not "image", "tag" or "use". Nine of the 11 negation queries return
   nothing.
7. **"Latest" returns the stale fact, under every method.** q100 "latest
   deploy window" returns nothing from served recall. Every other method
   ranks m007 first ("The deploy window … was Tuesday and Thursday … until
   2026-05-31"), above the window in force since 2026-06-01 (m008). No
   search method here knows which of two claims is newer, so the user gets a
   confident wrong answer. q106 "most recent incident" ranks the 2026-10-07
   incident at 4th under OR and at 1st only under `dense`.
8. **A long question finds nothing.** All 11 return no hits. For example,
   q081 "After rotating the API key of the email provider the invoice emails
   stopped going out, what do we need to do after a secret rotation?" Every
   other method ranks the incident m176 first, and `bm25` and `hybrid_rrf`
   put the fix m021 ("An ExternalSecret refresh does not restart pods …") at
   rank 2.
9. **Multi-hop needs two answers and AND gives none.** q111 "which machine
   type renders invoices and what font does it embed" needs m018 (the
   c2-standard-16 pool) and m198 (Noto Sans). Served recall returns no hits.
   `pg_simple_or` finds m198 but not m018, because "renders" is not
   "rendering". `pg_english_or` and `bm25` find both in the top 5.
10. **Even an exact hit loses its neighbours.** q003 "Idempotency-Key
    header" returns only m054. The related m055 ("Idempotency keys are
    remembered for 24 hours") and m182 (batch jobs pass an idempotency key)
    lack "header", so AND can never show them. This is why `exact` scores
    success@5 1.00 but recall@10 0.56.

## What a user sees

With served recall, failure means **an empty result, not a wrong one**: 100
of 121 queries return no hits, and every query that returns anything returns
a correct answer at rank 1 (success@1 equals success@5). An empty result
cannot be told apart from "OML knows nothing about this". The server's
instructions ask the agent to "try other words once before concluding OML
holds nothing". An agent that does so once with keywords (`oml_keywords`)
still gets nothing for 75% of the queries, and probably concludes the memory
is empty and moves on. The reviewed knowledge is there, but the agent never
reads it.

The OR methods trade empty results for **wrong results**. `pg_english_or`
returns something for 97% of the queries, ranks an answer first for 67% and
in the top 5 for 86%. When it misses, it returns plausible rows that share a
word with the query, such as the release-tag row for "postgres version" or
the "destroy schedule" row for "dunnig schedule". The dense and hybrid
methods always return 10 rows. This set has no unanswerable queries, so how
often a method shows confident rows when nothing relevant exists is not
measured. That is the next thing to measure before turning on any method
that never returns empty.

## Recommendations, ranked by gain per cost

1. **OR semantics on the existing index.** Gain: success@5 0.17 → 0.73,
   zero-result rate 0.83 → 0.05. It fixes question (0 → 0.73), negation
   (0.18 → 1.00), temporal (0 → 0.73) and long (0 → 0.64), and fixes typo
   and partial queries that contain at least one correct word. Cost: the
   statement builds the tsquery with `|` in place of `&` and ranks by
   `ts_rank_cd`. The `simple` configuration and its GIN expression index
   stay unchanged, so no reindex is needed and the decision against
   stemming is untouched. The conservative form is `pg_simple_and_or`:
   every query AND answers today is unchanged, and OR runs only when AND
   finds nothing. It scores the same success@5 (0.73). It also makes the
   Postgres store return the same rows as the in-memory test store, which
   already uses OR. It changes the score definition `retrieval.md` gives
   for the PoC store, so it needs an ADR.
2. **Fix the tokenizer for identifiers.** Gain: identifier zero-result rate
   0.27 → 0 for the three failures above. Cost: index a normalized copy of
   the claim in which `.`, `/` and trailing punctuation separate tokens
   (`regexp_replace` inside the expression index, applied to the query the
   same way). This needs a reindex, but it does not change the
   language-neutral choice. It was not measured separately; trigram covers
   the same queries (below).
3. **Stemming and stop words.** Gain over item 1: success@5 0.73 → 0.86,
   success@1 0.49 → 0.67. It fixes inflection (0.73 → 0.91), question
   (→ 1.00), long (→ 0.91) and multihop (0.46 → 0.91). Cost: a full FTS
   reindex, and the multilingual risk `retrieval.md` chose `simple` to
   avoid. One way to keep that choice is to index both a `simple` and an
   `english` vector and OR the two queries, so non-English text still
   matches exactly. The per-workarea configuration already on the roadmap
   is the other way.
4. **Trigram matching fused by RRF.** Gain over item 3: success@5 0.86 →
   0.93, nDCG@10 +0.04 to +0.12. It fixes partial (0.73 → 1.00), typo
   (0.82 → 1.00) and identifier (0.73 → 1.00), the only method here that
   does all three in SQL. Cost: the `pg_trgm` extension, a `gin_trgm_ops`
   index on the claim, and a second candidate list fused in the statement.
   Its latency at real corpus sizes is unmeasured. It also extends the
   erasure inventory and the held-text containment: held rows must not
   enter a trigram index any more than the FTS index.
5. **Hybrid dense retrieval** (designed in `retrieval.md`, not built). Gain
   over item 4: success@5 0.93 → 0.97, nDCG@10 +0.02 to +0.07. The real gain
   is paraphrase (0.64 → 0.82), the one category no lexical method fixes.
   Cost: the highest here. An embedding model at promotion, pgvector,
   embeddings in the erasure inventory, an approximate index whose
   determinism `retrieval.md` already lists as open, and a method that never
   returns empty (see above). On this set, BM25 alone (0.92) and trigram
   fusion (0.93) come within 0.05 of it.
6. **Make "latest" a structure, not a search.** No method fixes temporal
   queries (q100 ranks the stale m007 first everywhere). The fix is in the
   data and the filters. Writers should supersede (`replaces`) a fact that
   changed, so the old tip leaves recall, as the PoC demo shows. Recall
   could also take the `since`/`until` filters that the listing already
   has, or break ties by `promoted_at`. Structured filters by kind,
   workarea, term or anchor are not measured here, because the set has one
   workarea and one kind.
7. **Until item 1 ships, keep the instruction honest.** The MCP
   instructions already say every word must match. `oml_keywords` shows
   that following them literally lifts success@5 only from 0.17 to 0.24. A
   stronger interim instruction ("one or two rare words; an identifier
   alone") would help a little, but it is a workaround for the AND, not a
   fix.

## Served pipelines (ADR-0049), 2026-11-04

The recommendations below were built as search pipelines behind a port and measured through the
served `oml_read`, the same harness as `oml` above, by `served.sh` on a release build
(`results/served_*.json`). `keyword-all` is v0.5.0's recall and reproduces `oml` on every query.

| pipeline | success@1 | success@5 | recall@10 | nDCG@10 | zero-result | median ms | p95 ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| `keyword-all` | 0.17 | 0.17 | 0.12 | 0.16 | 0.83 | 15.9 | 18.8 |
| `keyword-any` | 0.65 | 0.81 | 0.73 | 0.69 | 0.03 | 19.4 | 29.6 |
| `bm25` | 0.82 | 0.91 | 0.91 | 0.86 | 0.00 | 33.9 | 52.1 |
| `bm25-trigram` | 0.78 | 0.94 | 0.93 | 0.86 | 0.00 | 73.9 | 100.6 |

Every pipeline passes its gate (ADR-0048 d1), so the default is `bm25-trigram`, the highest
success@5. `bm25`, served in SQL over the readable rows, lands on the offline BM25 baseline
(0.92); fusing trigrams lifts partial and typo to 1.00 each and costs latency, since the trigram
list scans the tenant's partition under row security. A second run moved each of the last three
by about one query (0.83, 0.92, 0.95 on a debug build): ties break by artifact ids a fresh load
draws anew.

The failure hook's own shape, the query minus stop words with `k` 3 under 200 tokens
(`results/hook_*.json`): success@3 0.24 and 75% empty under `keyword-all`, 0.88 and none empty
under `bm25-trigram`.

## Federated recall (ADR-0072), 2026-11-11

`federated.sh` loads the corpus as one `committed` workarea per topic, eleven, and recalls with a
key that reads them all, on `bm25-postings`, `k` 10 (`results/federated.json`). The **scoped**
split scopes each answerable query at its first answering memory's topic; the **cross** split at
the next topic holding none of its answers. `wide@8` is the default `max_sources` over ten
candidates, so the estimate chooses; the union searches all of them. The **skew** split pads
`database` with 2,400 SciFact abstracts, 100 times the largest other topic, and caps
`max_sources` at 4.

| split / mode | success@5 | nDCG@10 | selection precision | selection recall | p95 ms |
|---|---:|---:|---:|---:|---:|
| scoped / scope | 0.950 | 0.839 | n/a | n/a | 31.3 |
| scoped / wide@8 | 0.934 | 0.876 | 0.122 | 1.000 | 36.9 |
| scoped / union | 0.934 | 0.876 | 0.100 | 1.000 | 30.7 |
| cross / scope | 0.000 | 0.000 | n/a | n/a | 21.3 |
| cross / wide@8 | 0.934 | 0.876 | 0.121 | 0.992 | 24.9 |
| cross / union | 0.934 | 0.876 | 0.100 | 1.000 | 43.0 |
| skew / scope | 0.000 | 0.000 | n/a | n/a | 34.3 |
| skew / wide@4 | 0.926 | 0.844 | 0.215 | 0.975 | 41.3 |
| skew / union | 0.926 | 0.853 | 0.100 | 1.000 | 46.3 |

- **`wide` passes ADR-0049 d9's rule against `scope`.** On the cross split it finds an answer in the top 5 for 0.934 of the queries, where the scope holds none by construction; on the scoped split success@5 falls from 0.950 to 0.934, one query in `paraphrase` (9/11 to 8/11) and one in `temporal` (10/11 to 9/11), no category by more than one (`success@5_by_category` in `results/federated.json`), while nDCG@10 rises from 0.839 to 0.876. The default stays `scope` until a tenant admin sets `wide`.
- **Selection costs nothing here.** `wide@8` scores the union's success@5 on both splits and its selection recall is 0.992 on the cross split. On the skew split, at 4 of 10 candidates, it keeps the union's success@5 (0.926) with selection recall 0.975, above phase 2's 0.90 trigger, and the estimate's four agree with the exact ReDDE analogue's (the union's top 100, counted per workarea) on 0.882 of them: phase 2's first trigger (#206) is not met.
- **Unanswerable queries find more.** Answered with at least one hit: 0.682 under `scope`, 1.000 under `wide`: eleven workareas hold some word of every query. A relevance floor (ADR-0053) is the answer, not selection.
- **The widen hint.** Rule 1, a candidate at twice the scope's rate per memory with a word on two of its memories, is silent on 0.917 of the scoped queries and, on the cross split, fires on 108 of 121 with precision 0.935 (a wide retry raised success@5). Both pass d11's gate. On the scoped split its 10 firings never helped, since the scope already held the answer; d11 gates only its silence there. Rule 2, a thin answer, fired on 101 of 102 hints when both ran, silent on 0.157 of the scoped queries: a topic of 12 to 25 memories rarely holds 10 matches. It is dropped by amendment (ADR-0072, Phase 1 as built). Rule 1 is measured here but not served: computing it costs every `scope` recall a candidate and statistics read, and the default path pays nothing for federation (ADR-0072 d12 as amended); the hint is deferred and returns on a precomputed signal (#244).
- **Latency.** p95 stays within 47 ms in every mode on this host, which other builds shared; the reference set's statistics read is one more statement per recall.
- **At 1,000 candidates** (`pg_store`'s `federation_overhead_at_1000_candidates`, release build, the same shared host): the statistics reads alone take 6.2 ms at p50 and 10.9 ms at p95, against d12's 10 ms. A whole `scope` recall with federation on takes 27.8 ms at p50 and 55.3 ms at p95, against 12.6 ms and 38.0 ms with it off; `wide` at `max_sources` 8 takes 47.0 ms and 113.1 ms. Working out the candidates, one ancestry and one subtree read per reader grant, costs more than the counts.
- **The cross split** re-scopes the existing labelled queries to a topic holding none of their answers, so it measures what reach buys over the union, not cross-team phrasing; new cross-team queries remain to be written (#205).
- **The candidate count** over the public server's live keys is read by `cargo run --release -p oml-pg --example federation_candidates -- <owner url>`; that run is the operator's, since this report's host does not reach the public server's database.

## Federated recall, phase 2 gate (ADR-0072 d4.5, d11; #206), 2026-11-11

**Verdict: `described` fails d11's floor on both pipelines, and no descriptor selector could pass
it on this eval.** Against `content` on the cross and skew splits together (242 queries), success@5
rises 0.41 points, one query, with a 95% interval of 0.00 to 1.24 points; the floor is 3 points.
An oracle that gives every selection slot to a source holding an answer gains at most 0.83
points. The adversarial descriptor also fails its own clause on the skew split.

`EVAL_PHASE2=1 federated.sh` (`fed2.py`) loads the same eleven `committed` topic workareas. The
cross split runs at `max_sources` 8 of 10 candidates and the skew split at 4, with `database`
padded with 2,400 SciFact abstracts. For each query it reads the union's ranking from the served
lead at depth 100, with every candidate's E(s) (`results/fed2.json`), and it replays each selector
offline: under one list, the answer for a searched set is the union's ranking restricted to it
(d7.2, test 6). The replay reproduced the served `wide` answer on every query, searched set and
top 10, 385 of 385 on `bm25-postings`. `bm25-sparse` is a convex combination, which `wide` refuses
until its bounds are taken over R (d7.2). The replay therefore reads both legs over R and fuses
them before restricting, the change d7.2 names. The same fusion replays the served `bm25-sparse`
`scope` answer on 264 of 264 queries. Each topic's descriptor (`descriptors.json`) is a summary
and eight questions, written from `corpus.json` before `queries.json` was read. It is scored by
BM25 over the candidates' descriptors alone, through a served `bm25` recall that lists their
workareas. `descriptor_slots` is 2. α is 0.4, the largest value with the best selection recall on
a random half of the skew split; on the held-out half, selection recall is 0.984, the same as
`content`'s.

| pipeline | split | content s@5 | described s@5 | oracle, 2 slots | union | selection recall, content / described |
|---|---|---:|---:|---:|---:|---:|
| `bm25-postings` | cross (121) | 0.926 | 0.926 | 0.926 | 0.926 | 0.992 / 0.992 |
| `bm25-postings` | skew (121) | 0.926 | 0.934 | 0.934 | 0.926 | 0.975 / 0.983 |
| `bm25-postings` | scoped (121) | 0.926 | 0.926 | 0.926 | 0.926 | n/a |
| `bm25-sparse` | cross (121) | 0.983 | 0.983 | 0.983 | 0.983 | 0.992 / 0.992 |
| `bm25-sparse` | skew (121) | 0.942 | 0.950 | 0.959 | 0.959 | 0.975 / 0.983 |
| `bm25-sparse` | scoped (121) | 0.983 | 0.983 | 0.983 | 0.983 | n/a |

| d11's floor, cross and skew together | `bm25-postings` | `bm25-sparse` |
|---|---|---|
| success@5, described − content (≥ +3 points) | **+0.41** [0.00, 1.24]; 1 gained, 0 lost | **+0.41** [0.00, 1.24]; 1 gained, 0 lost |
| ceiling: oracle in the 2 slots / in every slot | +0.41 / +0.41 | +0.83 [0.00, 2.07] / +0.83 |
| selection recall does not fall | 0.984 → 0.988, holds | 0.984 → 0.988, holds |
| scoped split falls by at most one query | 0 gained, 0 lost, holds | 0 gained, 0 lost, holds |
| **admitted** | **no** | **no** |

Intervals are 95% percentile bootstraps over the paired queries (10,000 resamples, seed 20261111).

- **By category.** The one query gained is q063 (`identifier`) on the skew split, on both
  pipelines. Every other category shows +0 −0 of 22. The descriptor slots changed the searched
  set on 2 cross and 26 skew queries; only q063's answer moved.
- **Why the ceiling is low.** Only 4 of the 242 queries, 1 cross and 3 skew, miss because no
  source holding their answer was searched. On `bm25-postings`, the other 14 misses in the top 5
  searched such a source, and its row ranked below 5. Selection cannot move that row, because
  every hit scores with R's statistics whatever was selected (d7). Three points need about 7 of
  the 242 queries, so a descriptor selector would have to win back more queries than selection
  loses here.
- **The adversarial descriptor** holds every word of every query, more than d2's 600 characters
  and eight questions allow. Placed on each topic in turn, it lowers selection recall on no cross
  query. Placed on `observability`, it lowers it on 5 skew queries, where d11 allows one, and it
  displaces at most 2 sources, `descriptor_slots`. A descriptor held to the schema would match
  less; that case is not measured.
- **Phase 2's build condition (d13) is not met either.** On the skew split, content selection
  recall is 0.975, above the 0.90 trigger. The exact ReDDE analogue, the union's top 100 counted
  per source, scores 8.3 points **below** `content` on `bm25-postings` (0.843, 10 queries lost)
  and 6.6 points below on `bm25-sparse`, so `content` is not "more than two queries below" it.
- **This run** used main at 4db2e6cc. Its `content` cross success@5 is 0.926; phase 1's run
  reported 0.934 for the same mode. Every comparison here is within this one run.
- **Not measured.** Memory-kind-3 postings are not built, so the descriptor score comes from a
  served `bm25` recall over descriptor workareas, with the same tokenizer and the same BM25, and
  its latency is not measured. On a shared host, union recall p95 on the skew split was 126 ms.

## Limits of this eval

- **One model wrote the corpus, the queries and the labels.** Their wording
  overlaps more than a real crew's would, which flatters lexical methods,
  and the `exact` category is 100% for everyone by construction. The
  paraphrases were written to avoid the memory's words, which favours
  `dense`. The order of the methods is more trustworthy than the size of
  each gap.
- **The labels are incomplete.** Grade-1 labels come from the author's
  reading, not from pooling every method's top 10, so a method that surfaces
  a related row nobody labelled is not credited. recall@10 and nDCG@10 are
  lower bounds.
- **The set is small.** With 11 queries per category, one query moves a
  category by 0.09. The overall intervals are above, and per-category
  differences under about 0.2 are noise.
- **The corpus is small and flat.** 200 memories, one workarea, no steering
  rows, no held rows, no supersession chains, all English. A larger corpus
  makes AND return something more often, and makes ranking and OR's
  precision harder. Neither effect is measured. Nothing here covers
  non-English text, where `simple` was chosen to be safe.
- **There are no unanswerable queries**, so false positives are unmeasured.
- **Latency** was measured with one client on loopback and is not a capacity
  number.

**Replacing it with real traffic.** The read log already records, per
recall, the request digest and the ids served after the cuts
(`retrieval.md`, The read log). An opt-in, retention-bounded sample of
recall query texts from a dogfood tenant, joined to what the agent did next,
would give real queries with implicit labels: an `oml_read` get of a served
id counts as a click, an immediate re-query counts as a reformulation, and
an empty result followed by no retry counts as abandonment. Pool the top 10
of every method for each sampled query, have a reviewer who did not write
the memories grade them, and the same `run.py` and `metrics.py` can score
the result. A few dozen unanswerable queries should be added to measure
false positives. Until then, treat these numbers as a floor for how bad AND
is, and as an order for the fixes, not as their final size.
