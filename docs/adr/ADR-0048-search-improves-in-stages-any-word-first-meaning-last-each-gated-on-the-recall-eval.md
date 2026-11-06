<!--
id: adr/ADR-0048-search-improves-in-stages-any-word-first-meaning-last-each-gated-on-the-recall-eval
tier: engineering
status: proposed · partly built: S1–S3 in v0.5.5, S6 in v0.5.6
answers: how recall works today and what OML's recall eval measured of it; in which order search improves and why, each stage with its measured or expected gain, cost, risk, the eval category it targets and how it is measured; how any-word matching, identifier tokens, stemming, rarity-aware ranking, trigrams, structured filters, write-time keywords, a reranker and embeddings each enter; why graph retrieval, a hosted query rewriter, a generative answerer and fine-tuning are not taken now and when to revisit; what every hit and every empty result tells the caller; what `oml_read` gains without a new tool, and how `search: keyword` keeps today's behaviour exactly; the budget and the latency budget; the eval as a CI gate and how real queries could be gathered; how the constitution holds at each stage; which downsides are arguments and which are opportunities; the release; and the questions left for the maintainer
-->

# ADR-0048 — Search improves in stages: any word first, meaning last, each gated on the recall eval

**Status:** proposed · 2026-11-04 · partly built while the record is still proposed

**Release:** S1–S3 introduced in v0.5.5 as the first pipelines of
[ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md)
(`keyword-any`, `bm25`, `bm25-trigram`), which also replaced d10's `search` values with pipeline
names and its default rule with ADR-0049 d9's; S6 introduced in v0.5.6 as
[ADR-0053](ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md)'s
opt-in `bm25-rerank`; the default since v0.5.6 is
[ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)'s
`bm25-postings`. S4 (filters) and S5 (author keywords) are not built and not assigned; S7
(meaning) is ADR-0052's learned sparse kind, introduced in v0.5.7 as the opt-in `sparse` and `bm25-sparse`. Open questions 1, 2, 4, 5 and 6 are
answered by what shipped ([releases](../changelog.md))

**Amends:** ADR-0013 (d4, at S7 only), ADR-0025 (d8: the score; d9: the read digest; d13),
ADR-0038 (d3, d6), ADR-0039 (d4); [retrieval](../architecture/retrieval.md)'s text handling and
the PoC score; answers #153 at S7's acceptance

The design mockups for this record are on one page, kept with the branch it was proposed on:
[search mockups](https://github.com/e8kor/open-memory-lab/blob/archive/feat/code-map/docs/design/search/mockups.html) (today against S1–S3 on five eval queries, an
`oml_read` answer in a Claude Code session, the user page's search, the admin search, the eval
dashboard and the stage roadmap).

## Context

**Recall today matches every word, exactly.** A recall runs one statement, `RECALL_FTS` in
`crates/adapters/oml-pg/src/lib.rs`: `to_tsvector('simple', m.claim) @@ plainto_tsquery('simple',
query)`, ranked by `ts_rank(…, 0)`, ties broken by id, cut at `k`, over the promoted served tips of
the readable workareas; the caller's own held rows come from a second statement with the same
score ([retrieval](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked)).
Four properties follow. `plainto_tsquery` joins the words with AND, so one word the memory lacks
removes it. `simple` stems nothing and drops no stop word, so "what" and "deployment" are required
terms ([text handling](../architecture/retrieval.md#text-handling)). The ranking functions "do not
use any global information" ([PostgreSQL, text search controls](https://www.postgresql.org/docs/current/textsearch-controls.html)),
so a rare word weighs what a common one does. And PostgreSQL's default parser keeps
`useInvoiceFilters.ts` and `InvoiceNumberingTest.concurrentIssue` whole as one token, so the name
inside is not found. Steering rows come first, unranked and never cut; the hits are the longest
rank prefix that fits `max_tokens` ([token budgets](../architecture/retrieval.md#token-budgets)).
The server tells agents to "pass 2-4 exact keywords" because of this
([ADR-0038](ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md)
d3), and two of the plugin's hooks rely on the AND as their gate (d6 there). No vector search is
served.

**OML's recall eval measured it.** On 2026-11-04 a fixed set of 200 promoted memories of a
fictional invoicing crew and 121 queries, 11 in each of 11 categories with graded labels, was run
through the served `oml_read` and through nine comparison methods over the same rows
([eval report](https://github.com/e8kor/open-memory-lab/blob/main/eval/recall/REPORT.md),
`eval/recall/`, data version 1). Success@5 is the share of queries with an answering
memory in the top five:

| method | success@1 | success@5 | zero results | what it is |
|---|---:|---:|---:|---|
| served recall | 0.17 | 0.17 | 0.83 | AND, `simple`, `ts_rank` |
| served, stop words removed | 0.24 | 0.24 | 0.75 | an agent following the instruction literally |
| `simple`, any word | 0.49 | 0.73 | 0.05 | the same index, words joined by OR, `ts_rank_cd` |
| `simple`, all words then any | 0.49 | 0.73 | 0.05 | AND, and OR only when AND finds nothing |
| `english`, any word | 0.67 | 0.86 | 0.03 | stemming and stop words |
| `english`, any word + trigram | 0.75 | 0.93 | 0.00 | fused by reciprocal rank with `pg_trgm` |
| BM25 | 0.79 | 0.92 | 0.00 | Snowball stems, rarity-weighted, split on non-alphanumerics |
| dense | 0.79 | 0.95 | 0.00 | `BAAI/bge-small-en-v1.5`, cosine |
| hybrid | 0.79 | 0.97 | 0.00 | BM25 and dense fused by reciprocal rank |

The report's bootstrap intervals for success@5 are 0.80–0.92 for `english` any word, 0.88–0.98
with trigrams, 0.87–0.96 for BM25 and 0.93–0.99 for hybrid. With 11 queries a category, one query
moves a category by 0.09, and the report reads per-category differences under about 0.2 as noise.
One model wrote the corpus, the queries and the labels; the set holds one workarea, one kind, no
held rows, no supersession, no non-English text and no unanswerable query, so false positives are
unmeasured. The report names three more findings this record takes: no method finds "the latest"
of two facts (q100 ranks the superseded deploy window first under every method); identifiers fail
on the tokenizer, not the ranking; and the in-memory test store keeps a row when any word matches
while PostgreSQL keeps it only when every word does, so `oml-app`'s use-case tests run a more
forgiving recall than users get.

**A reranker was measured beside it, in scratch.** For this record an off-the-shelf cross-encoder,
`Xenova/ms-marco-MiniLM-L-6-v2` through fastembed 0.9.1 on the 8-core host, re-scored the eval's
BM25 top 50: success@1 0.79 → 0.88, success@5 0.92 → 0.94, paraphrase success@5 0.64 → 0.82, the
same as dense, with no embedding stored. Over the hybrid top 50: 0.79 → 0.89 and 0.97 → 0.98. It
cost 33 ms median and 154 ms at p95 per query over BM25's candidates (BM25 serves only rows with a
matching word, often fewer than 50), and 176 ms and 210 ms over 50 hybrid candidates, against the
served recall's 14.4 ms median end to end. One run, not in the eval's committed results.

**What others do, labelled.** The retrieval-heavy memory products fuse keyword and meaning:
Zep/Graphiti ranks by "cosine semantic similarity search", "Okapi BM25 full-text search" and
"breadth-first search" ([Zep paper](https://arxiv.org/html/2501.13956)); mem0's 2026 algorithm
scores "semantic search, BM25 keyword search, entity search, and temporal reasoning" in parallel
([mem0 docs](https://docs.mem0.ai/core-concepts/memory-evaluation)). The one independent
agent-memory benchmark found puts a plain BM25 retriever at 41.5 overall, above every dense
embedder it tested (text-embedding-3-large 38.0) and above Zep, mem0 and Cognee at 24.0, 21.1 and
20.6 ([MemoryAgentBench](https://arxiv.org/html/2507.05257)). Anthropic reports that adding BM25 to
embeddings cut the top-20 retrieval failure rate by 49% against 35% for embeddings alone, and by
67% with a reranker, and that BM25 "is particularly effective for queries that include unique
identifiers or technical terms" ([Contextual Retrieval](https://www.anthropic.com/news/contextual-retrieval),
VENDOR). LongMemEval found that indexing short extracted facts beside each session raised BM25's
recall@5 from 0.634 to 0.683 ([LongMemEval](https://arxiv.org/html/2410.10813v2)). The coding
agents use no index: Claude Code loads "the first 200 lines of `MEMORY.md`" and reads topic files
"on demand using its standard file tools" ([Claude Code memory](https://code.claude.com/docs/en/memory)),
and Letta's file-system agent scored 74.0 on LoCoMo with grep and file search, VENDOR, writing that
"memory is more about how agents manage context than the exact retrieval mechanism used"
([Letta](https://www.letta.com/blog/benchmarking-ai-agent-memory)). Vendor benchmark numbers do not
compare: "6.4%" of LoCoMo's answer key is wrong
([Penfield audit](https://dev.to/penfieldlabs/we-audited-locomo-64-of-the-answer-key-is-wrong-and-the-judge-accepts-up-to-63-of-intentionally-33lg)),
and Zep's LoCoMo score "ranges from 58.44 to 94.7 depending on who measured it"
([braindetox](https://braindetox.kr/en/posts/memory_benchmark_scores_reliability_2026.html),
secondary). OML's own eval is the number this record ranks the stages by.

**The rule that must hold.** "R3, the gate changes visibility, not ranking. Nothing is readable by
others until it is ratified. Review never reorders results, and steering rows, which precede hits,
are never ranked" ([ADR-0001](ADR-0001-the-constitution.md) d3). Decided, not built.

## Decision

- **d1 — Search improves in seven stages, ordered by measured gain per cost, each its own decision
  and each gated on the recall eval.** A stage ships when its served form, run through the eval in
  CI (d12), reaches the gate its row names with no category falling by more than one query, and
  within the latency budget (d11):

    | Stage | What | Targets | Success@5, measured or expected | Cost | Gate |
    |---|---|---|---|---|---|
    | **S1** any word | OR on today's index; `match: all` with a flagged fallback; the test store agrees | question, negation, temporal, long | 0.17 → 0.73, measured | one statement, no reindex | ≥ 0.70 overall, zero results ≤ 0.05 |
    | **S2** tokens and rarity | identifier splitting; `simple` and `english` vectors; BM25-class weights over the readable rows | inflection, question, long, multihop, identifier | 0.73 → 0.86–0.92, between the measured `english` OR and BM25 | one migration, one reindex | ≥ 0.86, success@1 ≥ 0.67 |
    | **S3** typos and fragments | `pg_trgm` fused by reciprocal rank; did you mean | partial, typo, identifier | 0.86 → 0.93, measured on `english` OR | two trusted extensions, a trigram index | ≥ 0.92; partial and typo ≥ 0.91 |
    | **S4** structure | filters inside the scan; time windows; supersession for "latest" | temporal; filters (eval v2) | unmeasured: version 1 has one workarea and one kind | indexed columns, parameters | eval v2's filter and supersession queries; no under-return |
    | **S5** keywords at write | an author's index terms, reviewed with the text | paraphrase, partial | unmeasured; LongMemEval +0.049 recall@5 for BM25 | one field, one weighted vector | paraphrase rises, nothing falls |
    | **S6** reranker | an off-the-shelf cross-encoder over the top 50, opt-in | success@1, paraphrase | 0.92 → 0.94, success@1 0.79 → 0.88, measured in scratch | a model in the lead; 33–176 ms median | inside the latency budget, else opt-in |
    | **S7** meaning | embeddings fused with the keyword lists | paraphrase | 0.93 → 0.97, measured offline | an indexer, a vector index, held embeddings, erasure rows | a paraphrase gap left after S1–S6 |

    Graph retrieval, a hosted query rewriter, a generative answerer and fine-tuning are not
    stages (rejected alternatives). Each later stage is decided again on the eval's numbers after
    the one before it is served; this order is this record's reading of version 1 of the data, and
    a later version may reorder S5–S7.

- **d2 — S1: a recall matches any word of the query, ranked, and says when every word matched.**
  `recall_fts` and `recall_own_held` build the tsquery from the query's words joined by `|`
  (`websearch_to_tsquery`'s parse with its `&` turned to `|`, so a quoted phrase stays a phrase and
  a `-word` stays an exclusion) and rank by `ts_rank_cd` with normalization 0, ties by
  `artifact_id`. The `simple` configuration and its GIN expression index stay, so nothing is
  reindexed. A request may send `match: "all"`: every word must match, and when no row has them
  all the recall serves the any-word list marked `fallback: "any_word"`. The eval measured that
  order equal to plain any-word at success@5 (0.73) and lower at nDCG@10 on exact queries (0.84
  against 0.90), because all-words-first hides a correct answer's neighbours (q003 loses m055 and
  m182). So the default is `any`, and `all` is the graceful order for a caller who wants it. The
  in-memory test store already keeps a row when any word matches and scores by the count of
  distinct words matched; S1 makes PostgreSQL return the same rows, and a test asserts the same set
  from both stores for every eval query (equal order stays unasserted). The gain is the measured
  0.17 → 0.73 and 0.83 → 0.05 zero results. The risk is the tail: an any-word recall serves rows
  that share one common word with the query (the report's "postgres version" returns release
  tags), which d9's `matched` shows the caller and S2's weights push down.

- **d3 — S2: tokens are split where identifiers join them, stemmed beside the exact form, and
  weighted by rarity among the rows the key may read.**
    - **Identifiers.** The indexed text and the query pass through one normaliser before the
      parser: `.`, `/`, `:`, `#` and `@` between word characters become spaces, a trailing full
      stop is dropped, and a CamelCase or snake_case run yields its parts beside the whole, so
      `useInvoiceFilters.ts` indexes `useinvoicefilters`, `use`, `invoice`, `filters` and `ts`.
      The eval's three identifier failures are all tokenizer failures (report, finding 4); BM25,
      which splits on non-alphanumerics, finds all three at rank 1.
    - **Stemming without losing the exact form.** Each row is indexed under `simple` and under the
      configuration the new setting `search.fts_config_stemmed` names, default `english`, and a
      query matches either. `simple` stays, so the reason
      [retrieval](../architecture/retrieval.md#text-handling) gave for it holds: a non-English word
      still matches exactly, and English words also match their stems (the eval: 0.73 → 0.86;
      inflection 0.73 → 0.91; multihop 0.46 → 0.91). `unaccent` folds accents in both. A
      per-workarea language stays #155.
    - **Rarity, over the readable rows only.** The score is BM25-class: for each query term, its
      inverse document frequency times a saturated, length-normalised term frequency, with k1 1.2
      and b 0.75, the textbook constants, unmeasured on OML. The document frequencies, the row
      count and the average length are computed in the same statement over the rows the key may
      read, promoted and its own held alike, so a row the key cannot read never moves another
      row's rank, as a tenant-wide statistics table would. No new table and no new extension. The
      eval brackets the gain: `english` any-word without weights reaches success@1 0.67, BM25 0.79.
    - **Phrase and prefix.** A quoted phrase is a phrase (`<->`); a word ending in `*` is a prefix
      (`:*`). No implicit prefix: the eval measured none.

    One migration adds the two vector expressions and their GIN indexes per tenant partition; the
    owner role builds them, as it builds today's
    ([ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d11).

- **d4 — S3: trigrams find misspellings and fragments, fused by reciprocal rank.** A third
  candidate list ranks by `pg_trgm`'s `word_similarity` above 0.3 over a `gin_trgm_ops` index on
  the normalised claim, fused with S2's list by reciprocal rank, constant 60, unmeasured on OML
  beyond this set. The eval: 0.86 → 0.93; partial 0.73 → 1.00; typo 0.82 → 1.00; nDCG@10 +0.04 to
  +0.12 paired. `pg_trgm` and `unaccent` are trusted extensions, which "can be run by any user who
  has `CREATE` privilege on the current database"
  ([PostgreSQL, additional modules](https://www.postgresql.org/docs/current/contrib.html)), so the
  owner role creates them and the serving role still owns nothing. The eval's trigram number comes
  from a sequential scan of 200 rows; its cost at scale is #102. A held row enters trigram rows
  only inside held-text containment, as it enters full text today.

- **d5 — S4: filters run inside the scan, before ranking, and time is structure, not search.**
  `oml_read` in recall mode and `GET /v1/memories` take: `kinds` (as today; a kind kept out of the
  default set, such as `code` from ADR-0047 d2, is served only when named); `workareas`, up to four
  scopes read as one, each under its own access and ADR-0030's boundary; `since` and `until` on a
  version's creation time, as the listing has them; `anchor_repo`, `anchor_path_prefix` and
  `anchor_symbol` (ADR-0047 d3); and `claim_key_prefix`. Each is a predicate in the candidate
  statement and an indexed column on every index row, so a filtered recall never under-returns by
  discarding candidates after a cut. Valid time waits for the parked ADR-0036. A filter on author
  or key is served only on the admin search of ADR-0045 d21, never to an agent key: ADR-0044 d9
  shows universe claims to agents without their source. "The latest" of two facts is answered by
  supersession, which already keeps a replaced version out of the hits; the plugin's skill asks a
  writer to replace a changed fact rather than add a second, and no ranking term reads
  `promoted_at`, which would let review's timing reorder results. The gain is unmeasured: version
  2 of the eval adds filter, multi-workarea and supersession queries (d12).

- **d6 — S5: an author may give a memory up to eight index keywords, reviewed with its text.**
  `oml_write` takes `keywords`, words or short phrases indexed at a lower weight than the text,
  shown to the reviewer beside the text and part of the signed content. A reviewer who wants other
  keywords supersedes the version, as for any change of content, and a third identity reviews it.
  So no verdict changes a row's rank: the keywords are authored content, not a reviewer's act at
  promotion, which R3 would forbid. The scanner adds none; a term it derived would be a
  deterministic function of the text, which S2's normaliser already is. The expected gain is
  LongMemEval's fact-key result (BM25 recall@5 0.634 → 0.683), on paraphrase and partial queries,
  unmeasured on OML until eval v2 holds memories with keywords a second model wrote.

- **d7 — S6: an off-the-shelf cross-encoder may rerank the top 50, inside the deployment, opt-in
  until it fits the latency budget.** The lead scores the first 50 fused candidates, or fewer, with
  a cross-encoder reading the query and each claim together, and reorders them; steering is
  untouched and the budget cut runs after. The model reads only rows the key may read, already
  candidates, and stores nothing, so it adds no index, no erasure row and no held-embedding rule.
  It runs in pure Rust through `candle` ([ADR-0006](ADR-0006-pure-rust.md)); the scratch
  measurement used ONNX Runtime, so `candle`'s CPU cost is #102. No fine-tuning. A request asks
  with `rerank: true`; the deployment setting `search.rerank` turns it on for every recall once
  #102's p95 fits d11's budget. Measured in scratch: success@1 0.79 → 0.88, and the paraphrase gap
  S7 exists to close closed on this set (0.64 → 0.82) without embeddings. That is why S6 comes
  before S7: the same paraphrase number on version 1, at a fraction of S7's governance.

- **d8 — S7: meaning, by embeddings fused with the keyword lists, only when S1–S6 leave a
  measured gap.** The first draft of this record stands as S7, unchanged in substance. The
  semantic list is the nearest neighbours of the query's embedding under ADR-0013 d4's per-tenant
  HNSW layout ([pgvector layout](../architecture/retrieval.md#pgvector-layout)), fused with the
  keyword lists by reciprocal rank. A held row the key may read enters through a fenced held
  embedding beside the fenced held full-text row, so it ranks where it would rank once promoted,
  and promotion moves the embedding without recomputing it. The model runs inside the deployment
  through `candle` by default; a provider, through an `oml-embed-*` plugin, only by the deployment
  owner's journalled choice; never on the client, where the server could not tell a computed
  vector from a planted one. One model per deployment, its identifier on every row; a change
  re-embeds the tenant while recall serves without the semantic list. The query is embedded in the
  request under its own timeout, and a timeout or a lagging indexer serves the keyword lists marked
  `degraded: "semantic_unavailable"`, never an error; query embeddings are neither stored nor
  logged. Its gate: the served S1–S6 measured on eval v2's paraphrase queries, at least 30, and S7
  taken only if hybrid beats them there by more than the bootstrap interval with no category
  falling. On version 1 the offline gap is 0.93 → 0.97 overall and 0.64 → 0.82 on paraphrase, and
  S6 closed the paraphrase part in scratch. This answers #153 at S7's acceptance; the model is
  chosen by #102.

- **d9 — Every hit says why it matched, and an empty or thin result explains itself.** A hit gains
  `matched`: `words`, the query words it contains, and `of`, how many the query had; `via`, each
  way a word matched (`exact`, `stem`, `part` of an identifier, `trigram` with the spelling it
  matched, `keyword` from S5, `semantic` with its rank, `reranked`); and `filters`, those it
  passed. No raw score is served, since scores do not compare across lists, stages or servers. A
  recall result gains `explain` when no hit has every word or nothing is served: the counts of
  readable rows with all the words and with some, the count the filters left out, and
  `did_you_mean`, spellings taken only from rows the key may read, as S3's trigram list found them
  ("no memory has all of these words; 4 match some of them; did you mean `PgBouncer`"). The counts
  are over the readable set the recall already scans, so they disclose nothing an any-word recall
  would not. The report shows why: an empty AND result "cannot be told apart from 'OML knows
  nothing about this'", and an any-word hit that matched one common word cannot be told apart from
  an answer without `matched`.

- **d10 — `oml_read` gains fields, never a tool.** In recall mode: `search` (`keyword`, `text`,
  `hybrid`), `match` (`any`, `all`), `rerank`, and d5's filters. `search: "keyword"` is today's
  statement, byte for byte, kept for comparison and for the hooks; `text` is S1–S3, and S4–S6 as
  they ship; `hybrid` is S7. The default is `keyword` until S1's served form passes its gate in CI,
  then `text`. The field is `search`, not `mode`, because every `oml_read` answer already carries
  `mode` naming get, recall or listing (ADR-0039 d4); the first draft's `mode` would have given one
  name two meanings. The plugin's two hooks under ADR-0038 d6 send `search: "keyword"`, so their AND
  gate holds unchanged; the agent's own recall and the librarian use the default, and ADR-0038 d3's
  line becomes "recall with the words a note would use, or a question". The HTTP recall and
  `GET /v1/memories` take the same fields. The three MCP tools stay three.

- **d11 — The budget is unchanged; the latency budget is stated before it is met.** `k`,
  `max_tokens`, `oml-tc/1` and the longest-prefix cut are unchanged
  ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d6, d7). An any-word recall fills `k` more
  often, so the plugin's recall keeps `k` small (the hooks' 3), and `matched` lets the agent stop
  reading at a hit with one word of five. Latency: the served recall measured 14.4 ms median and
  18.7 ms at p95 end to end on 200 rows. The proposed budget, chosen and not measured, is a `text`
  p95 at most twice `keyword`'s on the eval corpus, and a stated number at 10,000 and 100,000
  promoted rows per tenant under #150's harness before `text` is the public server's default; S6
  joins a default only inside it.

- **d12 — The eval becomes a CI gate, then grows, and real queries come only by consent.** The eval
  in `eval/recall/` joins CI as a job that boots a lead against a
  throwaway PostgreSQL, loads the set and runs every method the server serves. It fails when
  `search: "keyword"` does not reproduce `pg_simple_and` exactly, when a served stage falls below
  its gate in d1, or when a category falls by more than one query against the committed results.
  Version 2 of the data adds what version 1 lacks: unanswerable queries, to measure the false
  positives an any-word recall brings (#105); filter, multi-workarea, held and supersession
  queries for S4; memories with keywords for S5; at least 30 paraphrase queries for S7's gate; S6
  as a committed method; and labels pooled from every method's top 10, graded by someone who did
  not write the memories. Real traffic: the request log keeps no query text (ADR-0042 d8's
  plaintext rule), so no real-traffic set can be drawn from it, and this record does not change
  that. Two consented routes remain: a person on the user page marks a search that missed and
  names the memory it should have found, which writes the query and the label as a held memory in
  an eval workarea, reviewed like any claim; and the maintainer exports recall queries from their
  own sessions' transcripts on their own machine. The designed read log, not scheduled, would add
  text-free signals per query digest: whether a recall served nothing, and whether a served id was
  fetched next ([read log](../architecture/retrieval.md#the-read-log)).

- **d13 — The constitution holds at every stage, and this is how.** Visibility before ranking:
  every stage ranks only candidates the key may read; S2's rarity statistics and d9's counts are
  computed over those rows alone, S6's model sees only them, and S7's index is filtered inside the
  scan. Held rows: a held row is a candidate only for its author with `include_unreviewed`, or
  inside an ADR-0030 in-progress workarea, scored by the same function as a promoted row, through
  held-text containment extended to the stemmed, trigram and (S7) embedding rows. Review never
  reorders: no ranking term reads a verdict, a reviewer or `promoted_at`, and S5's keywords are
  authored and reviewed content, never added by a verdict. Steering: first, unranked, outside
  fusion and reranking, never cut. R1–R2: search writes nothing and reviews nothing. R4: every
  derived row (the stemmed and trigram index rows, S7's embeddings) is erased with its memory. No
  new tool, and `search: "keyword"` returns today's results exactly.

- **d14 — The build of S1–S3, each step closed by named tests.**
    1. **S1:** `recall_matches_any_word_and_ranks_by_ts_rank_cd`,
       `match_all_falls_back_to_any_word_and_says_so`,
       `the_postgres_and_in_memory_stores_return_the_same_rows_for_every_eval_query`,
       `search_keyword_returns_todays_results_byte_for_byte`,
       `the_hooks_send_search_keyword`.
    2. **S2:** `an_identifier_inside_a_dotted_name_or_path_is_found_by_its_parts`,
       `a_word_matches_its_stem_and_a_non_english_word_matches_exactly`,
       `rarity_is_computed_over_the_rows_the_key_may_read`,
       `a_row_the_key_cannot_read_never_changes_another_rows_rank`.
    3. **S3:** `a_misspelled_word_finds_its_memory_through_trigrams`,
       `did_you_mean_offers_only_words_of_readable_rows`,
       `a_held_row_enters_no_shared_trigram_row`.
    4. **Across them:** `every_hit_names_the_words_and_ways_it_matched`,
       `an_empty_result_counts_rows_with_all_and_some_words`, and d12's eval job passing S1–S3's
       gates.

- **d15 — Every new failure has a doctor line.** `oml doctor` (admin) names: a tenant partition
  missing the stemmed or trigram index; `pg_trgm` or `unaccent` absent; recall answering
  `fallback` or an empty `explain` above a share of the last hour's recalls; S6's model file
  missing or its p95 over budget; and, at S7, an indexer lagging behind the outbox, a tenant whose
  embeddings name more than one model, a provider plugin configured, recall answering `degraded`,
  and the HNSW iterative scan at its ceiling.

## Downsides: argument or opportunity

| Downside | Verdict | Reasoning, and what it implies |
|---|---|---|
| Today 83% of the eval's queries return nothing | solved by S1 | 0.83 → 0.05 zero results, on the index that exists (d2). |
| Any-word recall serves rows that share one common word | argument, bounded | S2's rarity weights push them down, `matched` shows "1 of 2", and eval v2's unanswerable queries measure it (#105). |
| Stemming corrupts non-English text | argument, answered | `simple` stays beside `english`, so the exact form always matches (d3); a per-workarea language stays #155. |
| Rarity statistics leak rows a key cannot read | solved by design | Computed per query over the readable rows only (d3), at a cost #102 measures. |
| A trigram index costs writes and storage | argument | Unmeasured at scale (#102); +0.07 success@5 on the eval is the case for paying it. |
| Hooks gated on AND would fire on loose matches | solved | They send `search: "keyword"` (d10). |
| `keyword` must be kept | argument, small | One statement, already written: the comparison baseline and the hooks' gate. |
| The test store and PostgreSQL disagree | opportunity | S1 makes them return the same rows and asserts it (d2). |
| An empty result says nothing | opportunity | `explain` and `did_you_mean` (d9) turn it into a next step. |
| Lexical search misses paraphrase | argument, staged | 0.64 at best lexically; S5, then S6 (0.82 in scratch), then S7 only if a gap remains. |
| A reranker adds 33–176 ms median on CPU | argument | Opt-in until it fits d11's budget (d7); `candle`'s cost is #102. |
| A hosted model is one more thing to run | argument | S6's model stores nothing and fails open to S1–S3's order; no fine-tuning, so no training pipeline. |
| Embeddings move text off the machine | argument, scoped | Inside the deployment by default (d8); a provider is the owner's journalled choice. |
| Semantic and reranked results are hard to explain | opportunity | `matched.via` names `semantic` and `reranked` (d9). |
| The eval is synthetic and small | argument | One model wrote it; the order of methods is more trustworthy than each gap. Eval v2 and consented real queries (d12). |
| "Latest" fails under every method | argument, structural | Supersession and `since`/`until` (d5), not a ranking signal. |
| Unproven on OML | answered in part | S1–S3 are measured offline on OML's eval; S4–S7 stay gated. |

**What must not degrade, and how it is protected.** R1–R4 as d13 states them. Ordinary recall:
`search: "keyword"` is today's statement exactly and stays available, and `text` becomes the
default only after its served form passes the eval in CI (d10, d12). The three MCP tools stay
three.

## Consequences

- **Issues.** #102 is narrowed: version 1 of the eval exists; the model, the fusion
  constant and eval v2 remain. #105 opens (false positives on unanswerable queries), #102 (S2's
  per-query statistics and S3's trigram index at stated corpus sizes) and #102 (S6's latency and
  CPU in `candle`). #102 stays, for S7. #153 is answered at S7's acceptance.
- **Pages to change at acceptance.** Retrieval (text handling: `simple` beside a stemmed
  configuration; the score: BM25-class over the readable rows; the fusion; `matched` and
  `explain`), the schema (the vector and trigram expressions, the filter columns), the HTTP API
  (which calls recall "FTS + vector" while no vector is served), openapi.yaml, the MCP plugin page
  and mcp-tools.json (`search`, `match`, `rerank`, the filters, `matched`, `explain`), the server's
  MCP instructions ("every word must match … 2-4 exact keywords"), the `oml-memory` skill,
  configuration (`search.fts_config_stemmed`, `search.rerank`), the read log's digest members
  (`search`, `match`, `rerank`, the filters), and each amended record's Status line.

## Open questions for the maintainer

| # | Question | Options | Recommended |
|---|---|---|---|
| 1 | Which release takes S1–S3? | a. v0.5.3; b. v0.6.0 | a: they need no ADR-0030 and add no tool; v0.6.0 already holds ADR-0030 and demotion |
| 2 | S1's default match? | a. any word, `all` on request with a flagged fallback; b. all words, falling back to any | a: equal success@5, better nDCG@10 on exact queries (d2) |
| 3 | S2's rarity weights? | a. BM25-class in SQL over the readable rows; b. ParadeDB `pg_search`; c. tantivy in the lead; d. a tenant-wide statistics table | a (d3; rejected alternatives) |
| 4 | Stemming? | a. `simple` and `english` both; b. `english` only; c. `simple` only | a: keeps the language-neutral match and gains 0.13 |
| 5 | The field's name? | a. `search`; b. `mode`, as the first draft had it | a: `mode` already names get, recall and listing in every answer |
| 6 | S6 before S7? | a. yes; b. S7 first; c. neither | a: the same paraphrase number on this set, no stored derived data |
| 7 | When does `text` become the default? | a. when its served form passes the eval in CI; b. after #150's 100,000-row measurement too | a for self-hosted deployments, b for the public server |
| 8 | Does `workareas` (several scopes) belong here? | a. yes, up to four; b. no | a: one budget across a branch and its repository (ADR-0047) |
| 9 | Is a consented query sample worth a page feature? | a. the user page's "this missed" form; b. the maintainer's own transcripts only; c. neither | a and b |
| 10 | Is a provider plugin allowed on the public server (S7)? | a. no; b. yes, disclosed | a: the public server's users chose no provider |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A fourth MCP tool, `oml_search`** | ADR-0039 d8: fields on `oml_read` carry it (d10). |
| **The ParadeDB `pg_search` extension for BM25** | Licensed "GNU Affero General Public License v3.0" ([ParadeDB](https://github.com/paradedb/paradedb)); not among the trusted extensions the owner role may create without a superuser ([PostgreSQL](https://www.postgresql.org/docs/current/contrib.html)); and an index's corpus statistics span the rows it holds, not the rows a key may read. Revisit if d3's SQL scoring misses #102's budget. |
| **A tantivy index in the lead** | A second store outside PostgreSQL, where row security does not reach; ADR-0013 d1 serves every read from PostgreSQL for that reason. |
| **A tenant-wide statistics table** | Lets rows a key cannot read move the ranks of rows it can, and needs upkeep at every promotion, supersession and erasure (d3). |
| **All words, then any, as the default** | Measured equal at success@5 and lower at nDCG@10 on exact queries (d2); kept as `match: "all"`. |
| **`english` in place of `simple`** | Gives up the exact match a non-English word needs (d3). |
| **A score threshold under any-word matching** | ADR-0038 d6 rejected one, and scores do not compare across queries; `matched` lets the caller judge. |
| **Reviewer-added index terms at promotion** | A verdict would change a row's rank, which R3 forbids; keywords are authored and reviewed content (d6). |
| **Ranking by recency or `promoted_at`** | Review's timing would reorder results; "latest" is supersession and `since`/`until` (d5). |
| **Graph or entity retrieval now** | mem0's graph variant lost to its flat one on single-hop (65.71 against 67.13) and multi-hop (47.19 against 51.15) at 1.8 times the p95 latency (2.59 s against 1.44 s) ([mem0 paper](https://arxiv.org/html/2504.19413), VENDOR); Zep, mem0 and Cognee scored 24.0, 21.1 and 20.6 against BM25's 41.5 on MemoryAgentBench; mem0 replaced its graph store with an entity boost ([mem0 docs](https://docs.mem0.ai/open-source/features/graph-memory)); OML's multihop category reaches 1.00 success@5 with trigram fusion. Revisit when ADR-0008's labelled links are served and an eval category that needs them falls behind. |
| **A hosted model that rewrites or expands the query** | The calling agent is a stronger model and rewrites its own query for nothing; Letta's file-system agent, searching with grep, scored 74.0 on LoCoMo (VENDOR). The eval shows why it did not help OML: under AND, an agent dropping stop words moved success@5 from 0.17 to 0.24 only. Under S1 a rewrite takes effect, so OML steers the agent instead: the tool's description, `explain`'s counts and `did_you_mean` (d9). |
| **A generative model that searches and answers** | It can state what no memory says, it would sit between agents and reviewed facts and weaken what review guarantees, and it must never read a row the caller cannot read; OML answers no questions ([retrieval](../architecture/retrieval.md#out-of-scope-grounded-qa); ADR-0013). |
| **Fine-tuning a search model for OML** | No training data exists: the request log keeps no query text (d12), and synthetic queries would share one model's bias with the eval that judges them. One model trained on several tenants' memories would carry one tenant's text into another's results, so a tuned model is per tenant or none. A CPU host pays to serve and retrain it. Revisit for S6's or S7's model only when an off-the-shelf one shows a measured domain gap on eval v2 and a consented per-tenant set exists. |
| **Semantic search alone** | Exact names and identifiers are what lexical matching finds best; the eval's lexical stack reached 0.93 and BM25 alone 0.92 against dense's 0.95. |
| **A separate vector database** | ADR-0013 d1, as above. |
| **Filtering after the scan** | Under-returns silently; retrieval already rejects it (d5). |
| **Boosting reviewed rows** | Review would reorder results, which R3 forbids. |
