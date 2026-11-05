<!--
id: adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve
tier: engineering
status: accepted design · 2026-11-05
answers: why no text index serves a recall under row security and what does; the postings table's shape, partitioning, indexes and fence; how token ids are made, whether a tenant's words reach another tenant, and why no dictionary table exists; one table for term frequencies and learned sparse weights; how the row count, mean length and document frequencies stay those of the readable rows; which words are pruned, by what rule, and when the scan answers instead; how postings follow write, promotion, supersession, demotion, retirement and erasure; what a write costs and how a live database is filled; the retriever kinds and pipelines it adds, and the evidence that moves the default; what a hit says about its matched words; what it costs at 1,000,000 memories; the rejected roads; the open questions; the test plan
-->

# ADR-0052 — Text search reads integer postings, which row security lets an index serve, and says when it pruned a word or scanned

**Status:** accepted design · 2026-11-05 · proposed and accepted on 2026-11-05 with release v0.5.6

**Release:** introduced in v0.5.6 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0049 (proposed, on `feat/search-v2`): d4 (the pipeline table gains `bm25-postings`,
later `sparse` and `bm25-sparse`), d6 (the answer's `search` gains `access` and `pruned`; each hit's
`matched.via` gains `bm25_postings` and `sparse`), d12 item 3 and its rejected alternative (per-workarea
counts are kept, for this retriever alone, for the reason in d5 below). ADR-0005 d4 (the erasure
inventory gains the postings and the counts). ADR-0034 d11 (the owner's partition step creates a
postings partition beside each memories partition).

## Context

**Under row security no text index serves a recall.** The scale lane (`feat/scale-bench`,
`bench/scale/REPORT.md`, measured 2026-11-04 on PostgreSQL 18.6, 8 vCPU) found that every full-text and
trigram operator PostgreSQL ships (`@@`, `%`, `<%`, `ts_rank`) has `proleakproof = false`, so the
planner keeps it above the `tenant_isolation` policy's predicate and never turns it into an index
condition. Each text statement is an index scan on `(tenant_id, workarea_id, …)` and a per-row filter
over every readable row. Over a stored `simple` vector, an every-word match measured a p95 of 6.3 /
56.6 / 1,400 ms at 10,000 / 100,000 / 1,000,000 rows per tenant; BM25 in SQL over stored vectors,
52 / 677 / ≥ 5,000 ms. The same statements without the policy used the GIN index (today's every-word
recall: 34.7 ms p95 at 1,000,000). ADR-0049 d12 item 8 states the consequence for `bm25`: its latency
grows with the readable rows, and it is not solved.

**Integer equality is leakproof.** `int2eq`, `int4eq`, `int8eq` and `uuid_eq` are marked `LEAKPROOF`
(`pg_proc.proleakproof`, checked 2026-11-05), so a predicate `token_id = ANY($n)` stays an index
condition under the policy. The lane's added case 2 measured an inverted table `(memory_id, token_id int, weight real)` with a btree on `token_id`, 60
Zipf-distributed postings per memory, 10 query tokens, as the serving role under row security:
`Index Scan using …_token … Index Cond: (token_id = q.token_id)`. Unpruned, the common tokens' postings
made it linear (p95 97 / 328 / 735 ms at 30,000 / 100,000 / 200,000); with the 100 most frequent token
ids dropped from each query, p95 4.9 / 15.3 / 29.6 ms, about 137 ms at 1,000,000 by a log-log
extrapolation (60,000,000 postings did not fit the build host), at about 4.4 GiB of heap and 0.35 GiB
of btree per 1,000,000 memories (extrapolated). The same lane showed that per-workarea document
frequencies summed over the readable workareas equal the frequencies counted under the policy for
every term checked (157, 158 and 163 terms at three scales), and that a tenant-wide count differed for
95 / 115 / 104 of them: a tenant-wide statistic ranks by rows the key cannot read.

**The operator's choice, 2026-11-05.** Of the roads the lane named (a leakproof marking, a
`SECURITY DEFINER` search function, an integer token table, an engine per tenant), the operator chose
the integer token table. The algorithm research of 2026-11-04 recommends an inference-free learned
sparse encoder, `opensearch-neural-sparse-encoding-doc-v3-distill` (Apache-2.0; model card: SciFact
0.708 and NFCorpus 0.345 nDCG@10 against BM25's 0.665–0.679 and 0.322–0.325; "for queries, it just
use a tokenizer and a weight look-up table"), whose document weights take the same table shape.

**What must hold.** R3: the gate changes visibility, not ranking ([ADR-0001](ADR-0001-the-constitution.md)
d3). R4: no silent deletion, and erasure removes every plaintext-derived copy it lists
([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d4). ADR-0049 d2: the core computes the candidate
scope, adapters rank inside it, and `a_row_outside_the_scope_never_changes_a_candidates_score`.
Row security stays the fence ([ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d1,
d11). Decided, not built.

## Decision

- **d1 — One postings table, partitioned by tenant, fenced by the same policy, keyed by integers.**
  Migration 0021 creates `search_postings`, list-partitioned by `tenant_id` with one partition
  `search_postings_t_<tenant id, 32 hex>` per tenant, `ENABLE` and `FORCE ROW LEVEL SECURITY` and the
  `tenant_isolation` policy that `memories` carries:

    | Column | Type | Holds |
    |---|---|---|
    | `tenant_id` | `uuid` | the partition key and the policy's column |
    | `memory_id` | `uuid` | the version (`memories.id`) the posting derives from |
    | `workarea_id` | `uuid` | that version's workarea, copied at write; a version never changes workarea |
    | `kind` | `smallint` | `1` term frequency of a `oml_search_vector` lexeme (d3); `2` a `doc-v3-distill` weight (d3) |
    | `memory_kind` | `smallint` | `1` `memory`, `2` `derived_fact`, copied at write; a request's `kinds` narrows the scope, so the counts of d5 are kept per memory kind too (amended 2026-11-05) |
    | `token_id` | `bigint` | d2 |
    | `weight` | `real` | kind 1: the lexeme's term frequency; kind 2: the encoder's weight |
    | `doc_len` | `integer` | kind 1: the version's `search_len`, BM25's row length; kind 2: `0`. `integer`, not `smallint`: nothing bounds a claim's lexemes below 32,767 (amended 2026-11-05) |
    | `state` | `smallint` | `0` held, `1` served (a promoted tip that is no retirement version), `2` off |

    Only versions of the hit kinds (`memory`, `derived_fact`) get postings, as only they are
    recalled (ADR-0025 d8); a steering or guardrail row, and a retirement version, get none.

    Two indexes per partition (amended 2026-11-05, open question 1): `search_postings_served`,
    `(kind, token_id, workarea_id) INCLUDE (memory_kind, memory_id, weight, doc_len, tenant_id)
    WHERE state = 1`, the access path every query takes, and `(memory_id)`, for the own held rows,
    the state flips, the erase and d6's rescoring. The served index holds the served postings alone,
    ordered so that a recall reads its own workareas' postings of a token and nothing else, and
    carries every column a score needs, so the read is index-only once vacuum has marked the pages
    visible. The first reading, `(kind, token_id)` over every posting with `workarea_id` and `state`
    filtered from the heap and `fillfactor` 90 to keep a flip heap-only, measured 3 to 18 times
    slower (open question 1); a flip into or out of served now touches the served index, which d8's
    write cost includes. Only leakproof equalities reach the indexes (`kind`, `token_id`,
    `workarea_id`, `memory_id`, `tenant_id`), so they serve the serving role. The owner
    creates each partition in the same owner transaction that ADR-0034 d11 runs for `memories`, before
    the attach, under the same lock timeout; 0021 creates one for every existing tenant. The serving
    role holds `SELECT`, `INSERT`, `UPDATE` and `DELETE` on it, as on `search_vocabulary`. The cost,
    carried openly: one more partition per tenant, and a second copy of each version's words in a
    shape the planner can index (d10 sizes it).

- **d2 — No dictionary: a lexeme's id is a hash, a model token's id is the model's.** For kind 1,
  `token_id` is the first eight bytes of SHA-256 over the lexeme's UTF-8 bytes, read as a signed
  big-endian `bigint`: `oml_token_id(text)` in SQL (`IMMUTABLE`, `SECURITY INVOKER`, over the built-in
  `sha256`) and the same function in `oml-pg`, held equal by a law (d12). For kind 2, `token_id` is the
  encoder's vocabulary index (0 to 30,521 for `doc-v3-distill`'s BERT vocabulary), which is public and
  the same for every tenant. So no table maps strings to ids: a write assigns nothing, two writers
  never contend for a new word, and no row of any shared table holds a tenant's word. **A token string
  does not cross tenants**: a hash is computed, never looked up, and the postings that hold it sit in
  the tenant's own partition. A hash is still plaintext-derived (a word list inverts it), which is why
  d7 erases postings and counts. Collisions: with 1,000,000 distinct lexemes in one tenant the expected
  number of colliding pairs is about n²/2⁶⁵ ≈ 3 × 10⁻⁸, so no collision check is built. Because the id
  is computed, a query word no row holds and a query word only unreadable rows hold are answered the
  same way, by a zero readable count (d5), with no posting read in either case. The cost: eight bytes
  per id where the measured case had four, about 5% of a posting row; an `int4` id would need a
  dictionary (rejected alternatives).

- **d3 — One table for both kinds, separated by `kind`.** Term frequencies and learned weights have
  the same scope columns, the same lifecycle, the same erase and the same partition step; one table
  gives one trigger, one erase statement and one partition per tenant, and `kind` leads the access
  index, so each kind's postings are contiguous and a query reads one kind. Kind 1 rows are written by
  a trigger from the stored `search_vector` in the write's transaction (d8). Kind 2 rows are written by
  the lead's encoder (d8) and appear after the row's commit. A new encoder, or a new version of one, is
  a new `kind` value and a new pipeline; its predecessor's rows are dropped only by a migration that
  names that kind, since they are derived and the journal folds the rows they derive from.

- **d4 — The access path: read the kept words' postings, then rescore the candidates exactly.** For
  kind 1 the statement takes ADR-0049 d12's query words (`SEARCH_WORDS`, English stop words dropped
  when the query has others), each word's forms (the exact word and its `english_stem`), and
  `oml_token_id` of each form. It then: (1) reads the readable counts (d5) for every form; (2) prunes
  (d6); (3) reads the postings of the kept forms, `kind = 1 AND token_id = ANY($forms)`, keeping a
  posting when `state = 1 AND workarea_id = ANY($promoted_nodes)`, or `state = 0 AND workarea_id =
  ANY($held_nodes)` with the version's `author_key_id` among the own held keys, read from `memories`
  for those postings alone; (4) scores BM25 exactly as ADR-0049 d12's `bm25` statement does (k1 1.2,
  b 0.75, a word scores its best form, the row length from `doc_len`), with the readable `n`, `avgdl`
  and document frequencies; (5) for the best `k` × 4 candidates, adds the pruned forms' contributions
  by the `(memory_id)` index, so every returned row's score is its full BM25 score. Under `match:
  "all"` it reads only the rarest kept form's postings, since every answer must hold it, and checks the
  other words per candidate, so `all` loses nothing to pruning. Kind 2 reads the query's
  word-piece ids with the encoder's shipped IDF weights and scores `sum(query weight × document
  weight)`, which needs no corpus statistic at all. The candidate scope is the core's
  `CandidateScope`, unchanged; the core re-checks every served row, as before (ADR-0049 d2).

- **d5 — The statistics are the readable rows', kept per workarea and summed at query time.** Two
  tables, maintained by the same trigger in the write's transaction and fenced like
  `search_vocabulary` (row security, `SET STATISTICS 0` per FR-45's rule): `search_term_rows
  (tenant_id, workarea_id, kind, memory_kind, token_id, rows)`, the served versions in that workarea
  holding the token, and `search_workarea_rows (tenant_id, workarea_id, kind, memory_kind, rows,
  total_len)`; a recall sums them over the memory kinds of its scope as well as its nodes. A recall sums them
  over the promoted nodes of its scope, and, when `include_unreviewed` asked, adds the own held rows'
  counts read from their postings and versions (few, by the `memories_author` index). The sum equals
  the count under the policy, as the scale lane measured for 157–163 terms per scale; a tenant-wide
  count is never kept. This is the alternative ADR-0049 d12 item 3 rejected for the scan, where it
  saved 4% because the scan reads every candidate's lexemes anyway; here it buys what the scan never
  needed, a document frequency known **before** any posting is read, which d6's pruning and budget
  require and which no tenant-wide figure may supply. The cost: about one counter row per distinct
  token per workarea (the lane's maintained table: 1.22M rows, 190 MiB, at 1,000,000 memories in 451
  workareas, built in 26 s) and about 1 ms more per promotion (0.5 → 1.1 ms at 10,000 rows, 0.9 → 2.0
  ms at 100,000, one client). Concurrent promotions in one workarea update the same rows of its common
  tokens; within one tenant every commit already serialises on the chain head
  ([ADR-0002](ADR-0002-postgresql-system-of-record.md); FR-86), so the counts lengthen that section
  rather than add a new one. That length is unmeasured, tracked as FR-102.

- **d6 — A word is pruned by its readable count, the postings read are bounded, and the scan answers
  when nothing is left.** While the query forms' summed readable frequencies fit `POSTING_BUDGET`,
  nothing is pruned and every form is read (amended 2026-11-05, below). Past it, a **word** is
  pruned whole, every form of it (amended 2026-11-05 on review): a word is **frequent** when one of
  its forms has a readable document frequency above `FREQUENT_SHARE` of the readable row count;
  frequent words are pruned, then, while the kept words' distinct forms sum above
  `POSTING_BUDGET`, the kept word with the most postings is pruned too. Pruning by form instead,
  as first built, could drop a word's stem and keep its exact form, losing the rows that hold only
  another inflection while `search.pruned` stayed `[]`; whole words make `pruned` name every word
  whose rows may be missing, so `pruned: []` means `bm25`'s list. Law:
  `a_pruned_list_is_bm25s_over_the_rows_its_unpruned_words_reach`. Both
  decisions read d5's sums alone, so which word is pruned depends only on rows the key may read
  (law in d12). The two are constants in `oml-pg`, not settings: `FREQUENT_SHARE` 0.1 and
  `POSTING_BUDGET` 10,000, set on 2026-11-05 by the scale lane and the external sets (open
  question 2), whose runs are committed. The best `4 × k` candidates by the kept forms are rescored
  (d4 step 5). *Amended 2026-11-05:* candidates come only from the postings of the forms read, and
  the rescoring reorders them and adds none, so a row holding only pruned forms is never found. The
  first rule pruned frequent forms whatever the budget; on LoCoMo, where a query reads one
  conversation of about 590 turns and 99% of queries had a frequent word, that cost R@100 0.813 →
  0.767 and R@20 0.6962 → 0.6935, served, for no latency the budget needed. Pruning only past the
  budget returns `bm25`'s lists on SciFact, NFCorpus and LoCoMo (R@20 0.8656 / 0.1840 / 0.6971,
  R@100 0.928 / 0.248 / 0.813, as `bm25`), and at 165,000 memories the universe p95 measured 41.7
  ms on a busier host (`bm25` 841 ms there), against 29.9 ms for the first rule; leaf and project
  scopes rose from 9.2 / 12.1 to 14.6 / 24.0 ms. Law:
  `a_scope_whose_postings_fit_the_budget_is_read_whole`. A pruned word still counts for the candidates it reaches (d4 step 5); what pruning loses
  is the rows that hold **only** pruned words. When every form is pruned, the retriever runs ADR-0049
  d12's `bm25` statement over the stored lexemes instead, which ranks identically (d12), and says so:
  the answer's `search.access` is `scan`. This fallback costs what the scan costs (1.4 s p95 at
  1,000,000 readable rows for the cheapest form), so its rate is counted
  (`oml_recalls_total{pipeline, access}`) and doctor warns when it exceeds the share FR-100 sets. A
  frequency fixed in advance (a shipped stop list or the most frequent ids of the corpus) was weighed
  and is not the rule: a tenant's own common words ("deploy" in an operations team) are on no shipped
  list, and the corpus's are tenant-wide (rejected alternatives).

- **d7 — Postings follow the version: state flips on promotion, supersession, demotion and
  retirement, and only an erase deletes.** A version's postings are written with it (`state` 0, held)
  and never deleted while its claim is held anywhere. Promotion sets `state` 1 and adds to the counts;
  supersession sets the replaced tip's to 2 and the successor's to 1 in the supersession's own
  transaction; demotion sets 2; retirement inserts a retirement version, which gets no postings (no
  search reads a retirement version, ADR-0025 d8), and sets the retired tip's to 2; a rejected held
  version's go to 2. Each change out of `state` 1 subtracts from the counts, and a count reaching zero
  deletes its row, so a word no served version says leaves `search_term_rows` as it leaves
  `search_vocabulary`. This keeps R4: demotion, supersession and retirement keep the bytes and the
  words derived from them. **An erase deletes the erased versions' postings and their counts in the
  erase event's transaction**, since they are plaintext-derived: ADR-0005 d4's inventory gains
  `search_postings` and `search_term_rows` beside "tsvector entries", with the same disposition
  (removed at erase; present in base backups and WAL until the retention window ends). The erase
  itself is journalled, so nothing is silent. Erase is not built (`oml-pg` writes no `erased` status
  yet); this is the rule its build follows. The cost: off postings stay and grow with versions, not
  tips (open question 3).

- **d8 — What a write pays, and how a live database is filled.** Kind 1: the trigger
  `memories_search_postings`, `AFTER INSERT OR UPDATE OF status, is_tip, retirement OR DELETE`, inserts
  one posting per lexeme of the stored `search_vector` at insert (about `search_len` rows; the lane's
  60-posting write took 1.8 / 2.7 / 5.8 ms p50 at 30,000 / 100,000 / 200,000), and flips and counts on
  the status changes of d7. No text is parsed twice: the lexemes come from the generated column.
  Kind 2: the lead's encoder (a `candle` port of `doc-v3-distill`, in the deployment, no provider)
  encodes each committed version from a bounded queue, keeps the weights above a floor, at most
  `SPARSE_MAX_TOKENS` per version, and inserts them in a transaction that reads the version's current
  state `FOR SHARE`, so a flip cannot interleave. A version not yet encoded is absent from the `sparse`
  list only; `readyz` and doctor name the backlog. The encoder's CPU per write is unmeasured, FR-101.
  **Migration and backfill:** 0021 creates the function, the table and its partitions, the two count
  tables and the trigger, then fills kind 1 postings and counts for every existing version under each
  tenant's context, as 0020 fills `search_vocabulary`, reading the stored vectors and no text. It holds
  the trigger's lock on `memories` for its fill, so writes wait for it; the fill time at 100,000 and
  1,000,000 rows is measured before the release that carries 0021 (FR-100); a batched, resumable fill is required before any tenant reaches 1,000,000 rows (open question 4, FR-103). Kind
  2 is filled by the encoder after start, oldest first. `bm25-postings` enters the build disabled
  (ADR-0049 d10) and becomes the default when d9's evidence holds, as it did on 2026-11-05; `sparse` and `bm25-sparse` are refused at start while the backlog is above zero.

- **d9 — Two retriever kinds behind the `Retriever` port, three named pipelines, and the default moved
  by evidence.** `RetrieverKind` gains `Bm25Postings` and, with the encoder, `Sparse`. `PgStore`
  serves them; the in-memory store does not, and a composition over it that enables either is refused
  at start (ADR-0049 d7). `Retrieved` gains `access` (`postings` or `scan`) and `pruned` (the query
  words whose every form d6 pruned). The pipelines:

    | Pipeline | Retrievers | Fusion | Enters |
    |---|---|---|---|
    | `bm25-postings` | `bm25_postings` | single | with 0021, disabled |
    | `sparse` | `sparse` | single | with the encoder, disabled |
    | `bm25-sparse` | `bm25_postings` and `sparse` | reciprocal rank, 60 | with the encoder, disabled |

    `bm25-postings` becomes the default in place of `bm25` when all three hold, measured on one build
    and committed: (1) it passes `bm25`'s recall-eval gate (ADR-0049 d9: success@5 ≥ 0.86, success@1 ≥
    0.67); (2) on SciFact, NFCorpus and LoCoMo its nDCG@10 is at most 0.005 below served `bm25`'s, which
    bounds what pruning costs; (3) on the scale lane its p95 at 100,000 readable rows, universe scope,
    is at most 50 ms and below `bm25`'s. `bm25` stays enabled, since it is the fallback statement and
    the comparison. *Met on 2026-11-05, on the build that ships it (`66979b4`, whole words, budget
    first):* (1) success@1 0.826 and success@5 0.934, `bm25`'s own (a load that draws other ids
    measured 0.835: ties move one query); (2) nDCG@10, R@20 and R@100 equal to `bm25`'s on SciFact
    (0.6851, 0.8656, 0.928), NFCorpus (0.3208, 0.1839, 0.247) and LoCoMo (0.5035, 0.6968, 0.813);
    (3) 25.5 ms p95 at 99,721 readable rows (165,000 memories), universe scope, against `bm25`'s
    485 ms on the same run, load average 2.3 to 4.7. 100,000 memories were not re-measured for
    this build. `bm25-postings` is its default.
    `sparse` or `bm25-sparse` replaces the default only when it passes the same gate
    and ranks above the default on the external sets (ADR-0049 d12 item 6's rule), at a p95 that meets
    (3). Each step is a change to `search.default`, no code change.

- **d10 — What 1,000,000 memories cost, as far as it is measured.** *Measured on 2026-11-05*
  (`bench/scale/REPORT.md`, 10,000 to 165,000 memories, about 36 postings each): 29.9 ms p95 at
  99,721 readable rows, universe scope; heap, both indexes and the counts about 7 times the
  memories heap, 1.1 GiB per 165,000 memories, about 6.7 GiB per 1,000,000 by linear
  extrapolation; 0021's fill about 0.4 s per 1,000 memories. 1,000,000 itself was not measured:
  the build host lacked the disk. The paragraph below is the estimate this record was proposed
  with. *Latency:* about 137 ms p95 with
  the 100 most frequent tokens pruned, universe scope, extrapolated from 200,000; leaf scopes cost
  less, since the scope filter drops their postings after the fetch. That misses 50 ms at 1,000,000;
  `POSTING_BUDGET` can hold the bound at the price of more pruning, and what that price is in nDCG is
  FR-100's measurement. Past it, the next step is an engine's (block-max or impact-ordered postings),
  not a `GROUP BY` (open question 5). *Space:* the measured case's row was about 76 bytes
  (4,369 MiB / 60,000,000); this shape adds 8 bytes for a `uuid` memory id, 4 for the `bigint` token and
  6 for `kind`, `doc_len` and `state`, about 94 bytes, so 60 postings per version come to about
  5.3 GiB of heap per kind, plus the `(kind, token_id)` index (0.35 GiB measured for an `int4` key,
  more for this one) and the `(memory_id)` index, both unmeasured at this size. Kind 1 writes
  `search_len` postings per version, not 60; OML's claims are one to a few sentences, so kind 1 is
  expected below the measured case, and the figure is FR-100's. Off versions add their postings.
  Against that, `memories` itself was 0.46 GiB at 1,000,000 rows, 1.18 GiB with two stored vectors.
  *Counts:* about 190 MiB for kind 1 (measured, maintained table). *Writes:* d8.

- **d11 — A hit says which words matched, which were pruned, and what the encoder added.** The answer's
  `search` gains `access` (`postings` or `scan`) and `pruned` (query words, `[]` when none). A hit's
  `matched.terms` holds the query words any of whose forms it holds, pruned words included when d4
  step 5 found them; `matched.via` names `bm25_postings` or `sparse`. For `sparse`, a hit gains
  `matched.expanded`: the query words it matched only through tokens the encoder added to the document
  and its text does not hold, so a match by expansion is never presented as a word the memory says.
  Word pieces are not shown; the per-token contributions are ADR-0048 d9's `explain` counts, not built
  in this step. The request line gains `search_access`.

- **d12 — The laws, and where each runs.** In `oml-port-contracts`, run against PostgreSQL by
  `crates/adapters/oml-pg/tests/pg_store.rs` for both new kinds where served:
  `a_retriever_never_returns_a_row_outside_the_scope` and
  `a_row_outside_the_scope_never_changes_a_candidates_score` (ADR-0049 d7) extended to them;
  `bm25_postings_ranks_as_bm25_does_when_nothing_is_pruned` (same ids, same order, scores equal to
  1e-5 relative, over a fixture and every `k`); `a_pruned_list_scores_each_of_its_rows_as_bm25_does`;
  `which_words_are_pruned_depends_on_the_readable_rows_alone` (adding unreadable rows that make a word
  tenant-frequent changes nothing); `a_query_of_only_frequent_words_is_answered_by_the_scan_and_says_so`;
  `a_word_only_unreadable_rows_hold_is_answered_as_a_word_no_row_holds`;
  `the_counts_summed_over_readable_nodes_equal_the_postings_counted_in_scope`;
  `a_superseded_demoted_or_retired_version_keeps_its_postings_and_leaves_the_counts`;
  `an_erased_version_leaves_no_posting_and_no_count` (with erase's build);
  `oml_token_id_in_sql_equals_the_adapters`; and, for 0021,
  `a_v0_5_5_database_migrated_through_0021_ranks_as_one_written_after_it`. Doctor: postings or counts
  missing for a tenant whose partition exists; a sampled count that disagrees with its postings (the
  repair is a re-fill, the cause a defect); the encoder's backlog; the `scan` share above FR-100's
  bound. *Built 2026-11-05 (`feat/token-postings`), but for the encoder's backlog, which waits for
  kind 2:* doctor reads `oml_recalls_total{pipeline, access}` from the metrics listener
  `OML_METRICS_URL` names (ADR-0042 d11), never from `/readyz`, which answers anyone and so may
  not carry every tenant's traffic (amended on review), and warns when the scan answered more than
  5% of `bm25-postings`'s recalls, over at least 20; read by an admin key, `GET
  /v2/search` carries the tenant's postings health, and doctor warns on a missing partition, on
  versions with no postings (the fill incomplete), on served versions with no served postings, and
  on per-workarea or sampled word counts that disagree with the postings, each line naming the
  repair, the owner's `oml_search_postings_refill(tenant)`, which is 0021's fill for one tenant.
  The refill is the owner's alone (`EXECUTE` revoked from `PUBLIC`) and takes that tenant's chain
  advisory lock, so it holds that tenant's writes and no other's; `oml_search_count` is granted to
  the serving role alone, whose writes' trigger calls it. A missing postings partition, as a lead
  built before 0021 leaves a tenant it creates, makes every write of that tenant fail; every start
  creates and fills it after migrating, and doctor says so meanwhile. The health check samples its
  200 word counts from a random token each read. A recall's statements share one `REPEATABLE READ,
  READ ONLY` snapshot. The parent table keeps no statistics of its own (FR-45); each partition
  keeps its tenant's. Lock order: every count write locks its word rows in token order, then its
  workarea row, and a take-back locks before it changes anything, so writes that skip the chain
  lock neither deadlock nor drive a count below zero. Laws:
  `the_postings_health_is_clean_where_the_trigger_kept_it_and_names_each_fault_until_a_refill`,
  `the_postings_health_samples_word_counts_across_the_whole_range`,
  `the_serving_role_may_not_run_the_postings_refill`,
  `a_refill_holds_its_own_tenants_writes_and_no_other_tenants`,
  `a_tenant_without_a_postings_partition_gets_one_at_the_next_start_and_its_writes_resume`,
  `concurrent_count_updates_without_the_chain_lock_never_deadlock`, `readyz_names_no_recall_counts`,
  `doctor_warns_on_the_scan_share_and_each_postings_fault_and_is_quiet_when_all_is_well`.

## Consequences

- **Pages changed when this record is accepted**: [retrieval](../architecture/retrieval.md) (the
  access path and pruning), the PostgreSQL schema page (`search_postings`, the two count tables,
  `oml_token_id`, migration 0021), [configuration](../operations/configuration.md) (the three pipeline
  names), the HTTP API, MCP plugin and `openapi.yaml` (`search.access`, `search.pruned`,
  `matched.expanded`), ADR-0005's inventory table, ADR-0034 d11's partition step, ADR-0049's pipeline
  table, and the benchmark pages.
- **Roadmap rows** opened: FR-100 (the postings at 100,000 and 1,000,000 rows measured, not
  extrapolated: latency by scope, the prune constants, the `scan` share, space per kind, the fill
  time), FR-101 (the encoder's CPU per write and the kind 2 backfill rate), FR-102 (the count rows
  under concurrent promotions in one workarea), FR-103 (a batched, resumable fill before any tenant
  reaches 1,000,000 rows; added 2026-11-05). Numbered after FR-99, which `feat/ontology-metadata`
  holds.
- **Costs**: a second copy of every version's words, larger than the table it indexes (d10); a
  trigger and counts on every write and state change (d5, d8); a migration whose fill holds writes
  (d8); a fallback that costs what the scan costs (d6); an encoder in the lead for kind 2 (d8).
- **What stays**: the `bm25` statement and the stored vectors of ADR-0049 d12, as fallback and
  comparison; the GIN and trigram indexes, which still serve no serving-role read; `search_vocabulary`
  and `did_you_mean`, unchanged.
- **Numbering**: ADR-0050 and ADR-0051 are taken on `feat/remember` and `feat/ontology-metadata`, so
  this record is ADR-0052, and its migration is 0021, the next after 0020 on any branch.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A `SECURITY DEFINER` search function** | It moves the fence from the policy to one function's code: it would run as a role that row security does not filter, pin the tenant from `oml.tenant_id` and take the readable workareas from its caller, so one defect in it, or one injection that reaches it, reads every tenant. ADR-0034 d5 refuses a definer function in `public` at start (`definer_function`) for that reason, and the lane named the same risk. Its latency would be the no-policy column. |
| **Marking `ts_match_vq` and the trigram operators `LEAKPROOF`** (ADR-0034 d13) | Only a superuser can, which adds a step outside the migrations to every install and is unavailable on most managed PostgreSQL. It does not make BM25 fast either: with `ts_match_vq` so marked, the `bm25` statement's p50 went from 615 to 442 ms at 51,830 rows (ADR-0049 d12 item 5), because common words still make most rows candidates. |
| **A search engine per tenant (Tantivy), fed from the outbox** | A second store of every tenant's words outside row security, which ADR-0013 d1 rejects for reads; the readable-workarea filter and readable statistics would be re-implemented inside it; a promoted row would be invisible until the feed caught up; the erase inventory gains a store with its own snapshots. The `Retriever` port keeps it open if postings in PostgreSQL stop scaling (open question 5). |
| **OpenSearch, rejected for now (settled 2026-11-05)** | Today it would hold every tenant's words outside row security for a need nothing has shown: postings in PostgreSQL meet the 50 ms bound to the scale this record measures, and no deployment asks for a hosted scale-out. **Revisit** when a tenant nears 300,000 readable rows (where d10 expects the pruned postings to cross 50 ms) or a hosted deployment needs search to scale out beyond one PostgreSQL. **Conditions on any revisit**, each a requirement, not a preference: OpenSearch only proposes candidates, and PostgreSQL re-checks each under row security before it is ranked or served (the core's re-check of ADR-0049 d2 stays the fence); one index per tenant, never a shared index filtered by a tenant field; only legs that need no corpus statistic, so learned sparse with fixed model weights and vectors, never OpenSearch's BM25, whose document frequencies are the index's and so count rows the key cannot read (d5); fed from the outbox, with the feed's lag shown on `/readyz` and by doctor, and a row promoted but not yet indexed still found by the PostgreSQL legs; and its indexes and snapshots entered in ADR-0005 d4's erasure inventory, an erase deleting the version's documents in the erase's own flow. |
| **The status quo: the stored-lexeme scan** | Linear in the readable rows: 1,400 ms p95 for the cheapest every-word form and ≥ 5,000 ms for BM25 at 1,000,000 rows, 677 ms for BM25 at 100,000. It stays as the fallback (d6). |
| **A per-tenant dictionary with `int4` ids from a per-tenant sequence** | Four bytes less per posting, but a write must look up or insert each new word, two writers of one new word wait on each other, the dictionary holds the tenant's words as plaintext, which erase must then prune when a word's last posting goes, and each tenant needs a sequence beside its partition. A hash needs none of it (d2). |
| **A global dictionary** | One table of every tenant's words, which no tenant policy can fence, and whose ids, assigned in order, tell a tenant when other tenants first wrote a word. |
| **Two tables, one per kind** | Two triggers, two erase statements, two partitions per tenant for one lifecycle; `kind` leading the index gives the same contiguity. Reopen if one kind's size or vacuum needs differ enough to want separate storage settings. |
| **Counting document frequencies at query time from the postings** | Exact, but only after the postings are read, and the reading is the cost pruning exists to avoid; a capped count would decide by how many postings an index range holds, tenant-wide, and leak. |
| **Pruning by a frequency fixed in advance** (a shipped stop list, or the tenant's most frequent ids) | A shipped list misses the tenant's own common words; the tenant's most frequent ids are a tenant-wide statistic, so the rows a key cannot read would decide which of its words count (law in d12). |
| **pgvector `sparsevec` with an HNSW index** | Its `ORDER BY` uses the index under the policy, as dense HNSW does, but the index takes at most 1,000 non-zero elements, ranks approximately, needs `relaxed_order` to keep recall under narrow scopes (the lane: recall@10 0.09 at a 1% share without it), and cannot express BM25 with readable-scoped statistics. |
| **Denormalising nothing: join `memories` for every posting's scope and state** | One index lookup per posting read, where the measured case filtered on columns of the posting itself; the copied `workarea_id` never changes, and the `state` flip is a heap-only update. |

## Open questions, settled 2026-11-05

The operator settled each question on 2026-11-05. Where the answer is a measurement, the build lane
(`feat/token-postings`) takes it and records the value here in the same change as the code.

1. **`workarea_id` in the access index** — yes, measured on `feat/token-postings`
   (`bench/scale/REPORT.md`, 2026-11-05): the served covering index of d1 against `(kind,
   token_id)` over every posting, warm p95 leaf / project / universe 9.2 / 12.1 / 29.9 ms against
   148 / 137 / 138 ms at 165,000 memories (99,721 readable at universe scope), and 5.7 / 12.8 /
   26.6 against 105 / 95.6 / 89.4 ms at 100,000. The price is size: 455 MiB against 42 MiB at
   165,000, beside a 568 MiB heap.
2. **The prune constants and the `scan` share** — `FREQUENT_SHARE` 0.1 and `POSTING_BUDGET`
   10,000 (d6). On SciFact, NFCorpus, LoCoMo and the recall set, through the adapter
   (`eval/bench/prune_sweep.py`, `results/prune-sweep.json`), a share of 0.1 cost at most 0.0005
   nDCG@10 and 0.05 cost 0.0019; the budget did not bind below 6,000 rows. At 165,000 memories,
   universe scope, the budget decides the p95: 46.7 ms at 20,000, 29.9 ms at 10,000, 14.1 ms at
   5,000, with 98.7%, 97.5% and 94.5% of `bm25`'s top 10 kept. The scan answered 0 of 330
   queries at 100,000 and 165,000, 2 of 330 at 10,000, and at most 0.4% on the external sets;
   doctor's bound on the `scan` share is 5%, a line not yet built (FR-100).
3. **Off postings** — deferred. Superseded, demoted and retired versions keep their postings
   (d7); no compaction is built. Reopen on FR-100's space figures.
4. **A fill too long for one migration** — 0021's fill, which holds writes while it runs, is
   enough for now; its time at 10,000 and 100,000 rows is measured in the build lane. A batched, resumable fill that the
   lead runs after migrating, with `bm25-postings` refused until it completes, is required before
   any tenant reaches 1,000,000 rows: FR-103.
5. **Past about 300,000 readable rows** — deferred until a tenant nears 300,000 readable rows;
   then impact-ordered postings, block-max evaluation, or the OpenSearch road under the conditions in
   the rejected alternatives, decided on FR-100's numbers at that size.
6. **Kind 2's model** — deferred to the learned-sparse offline experiment, a separate lane, which
   reports where the model comes from, the weight floor, `SPARSE_MAX_TOKENS` and its reach beyond
   English before any kind 2 code is built.
7. **Own held rows at scale** — measured (`bench/scale/REPORT.md`): a key holding 1,000 own held
   rows adds about 30 ms to a universe recall with `include_unreviewed` (59.2 against 29.9 ms p95
   at 165,000 memories); 10,000 put it at 266 ms, still below `bm25`'s 654 ms there. The rows are
   counted at query time (d5), so the cost follows how many the key holds in scope. Left as
   measured: the case is a bulk import awaiting review (FR-87), and a count kept per key would
   need a counter per author beside the per-workarea ones.

## Test plan

- **Laws first** (d12), red before the code: `oml-port-contracts` and `pg_store.rs`, on CI's pinned
  PostgreSQL, with `OML_TEST_REQUIRE_SPLIT=1` so the statements run as the serving role under the
  policy, never as the owner.
- **The recall eval** (`eval/recall/served.sh`, CI job `recall-eval`): `bm25-postings` beside `bm25`;
  its committed `served_bm25-postings.json`; the gate of d9 (1).
- **The external sets** (`eval/bench/`, SciFact, NFCorpus, LoCoMo; served through `oml_read`): nDCG@10,
  success@1, p50 and p95 for `bm25`, `bm25-postings` at the candidate prune constants, and
  `bm25-postings` with pruning off, which must equal `bm25` (the agreement law, measured end to end);
  the gate of d9 (2). A readable-half run: a key that reads half the workareas gets, for every query,
  the scores a database holding only that half gives.
- **The scale lane** (`bench/scale/`, `feat/scale-bench`): a new approach, h) `bm25-postings` through
  migration 0021's own table and trigger, at 10,000, 100,000 and 1,000,000 rows per tenant measured
  (the disk the 1,000,000 point needs is provisioned, not extrapolated), leaf / project / universe
  scopes, warm and cold p95, the share answered by `scan`, postings touched, heap and both indexes'
  size, the write (insert, promotion, supersession) p50 and p95, the 0021 fill time; and the counts'
  equality with the policy's count, as the lane's case d) checked. The gate of d9 (3) and FR-100.
- **Contention** (FR-102): the benchmark loader's 8 writers and 6 reviewers into one workarea, with and
  without the trigger, the chain-head retries and promotion p95 compared.
- **Kind 2, later**: `doc-v3-distill` encoded offline in Python over SciFact and NFCorpus first,
  checked against the model card's 0.708 and 0.345; then the `candle` encoder's output held equal to
  the Python encoder's on a fixture; then the served `sparse` and `bm25-sparse` through the same three
  harnesses, with the encoder's CPU per write recorded (FR-101).

## Mockup: the answer

```json
{
  "search": {
    "pipeline": "bm25-postings", "match": "any",
    "terms": ["idempotency", "key", "retry"],
    "access": "postings", "pruned": ["key"],
    "fallback": null, "did_you_mean": []
  },
  "hits": [
    {"id": "…", "score": 7.41,
     "matched": {"terms": ["idempotency", "key", "retry"], "of": 3, "via": ["bm25_postings"]}}
  ]
}
```

`key` was pruned as frequent among the readable rows; this hit holds `idempotency`, so it was a
candidate, and its score counts `key` too. A memory saying only "key" was not a candidate.
