<!--
id: adr/ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read
tier: engineering
status: proposed
answers: what recall searches today and what federated recall adds; which workareas are trusted sources and why no new flag; what an advertised workarea's descriptor holds, who writes and reviews it and how it is kept honest; why advertising grants nothing and whether a key can discover a workarea it cannot read; how a recall chooses its sources (candidates, scoring, threshold, cap) and why the hits are ranked as one list instead of merged per source; the `sources` argument and its default; the per-workarea and tenant policies; what each hit says about its source; what makes the answer repeatable; why source selection is ranking and never visibility; the cost, limits and latency budget; the eval; the three phases and their leakage tests; what the operator still has to answer
-->

# ADR-0072 — Recall searches the trusted workareas a key already reads, chosen per query by reviewed descriptors, and never widens what it may read

**Status:** proposed · 2026-11-08

**Amends:** ADR-0039 (d4: recall mode gains `sources`), ADR-0044 (d10: `universe` becomes one
trusted source among others when `sources` is not `scope`), ADR-0049 (d6: the answer says which
sources it searched and why)

## Context

**What recall searches today.** Read in `crates/oml-app/src/app.rs`, `App::recall_as` and
`App::served_nodes`: the hits of a recall are the served tips on subtree(scope) where reader is
in the key's effective access (`nodes_with`), less every node whose in-progress root differs from
the scope's (ADR-0030 d2). The scope's *ancestors* contribute steering only
(`steering_in_force` over the readable ancestry, ADR-0025 d8), never hits. The one exception is
ADR-0044 d10: a scope inside a user root also searches the `universe` node's own claims. So a
recall at `research-crew.experiments` never meets a reviewed claim in `ops-crew` or in
`research-crew.lit-review`, even when the key reads both: the agent must know the other workarea's
name and recall there. The scope is the agent's default workarea unless it names another
(ADR-0039 d4), and agents rarely name one.

**What the operator asked, on 2026-11-08.** "Recall will happen not only from one workarea or its
parents but from trusted production-ready, verified and reviewed workareas, and advertisement
workareas as well, for example with bm25 or with hypothetical question embedding, and based on
workarea metadata, its ontology etc, we will use it in recall call or not." Two corrections to
the premise, from the code above: hits come from the scope's subtree, not its parents; and
"production-ready" has no field today, so this record has to say which existing status means it.

**The forces.**

- **Access is the boundary, and nothing here may move it.** Effective access is computed per
  request from grants and denials and cached nowhere ([workareas](../architecture/workareas.md#effective-access));
  every retriever receives the node list the core computed and filters before its own `LIMIT`
  (ADR-0049 d2). Federation may only search *more of what the key could already read*.
- **R3.** The gate changes visibility, not ranking, and review never reorders results
  ([ADR-0001](ADR-0001-the-constitution.md) d3). A descriptor that is itself reviewed must not
  turn a verdict into a ranking input.
- **Occam.** Three tools ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d2); a new
  capability is a parameter before it is a tool (d8).
- **Repeatability.** The same key, query, approved set and time give the same answer (#176); the
  default pipeline is model-free at recall time ([ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) d9; #201's design constraints).
- **OML is not a federation of strangers.** Every workarea of a tenant sits in one PostgreSQL
  partition, under one postings table, with BM25 statistics kept per workarea and summed over the
  readable nodes at query time ([ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d5).
  The hard half of distributed retrieval, merging scores from engines that do not share
  statistics, does not arise here (d7).
- **Ontology is proposed, not on main.** Terms and `oml:about` edges are
  [ADR-0051](https://github.com/e8kor/open-memory-lab/pull/58) (proposed, PR #58), which unparks
  the edge of ADR-0027 (proposed on the unmerged `feat/ontology-crews` branch) and parks its crew
  kinds and classifier. Every use of terms below (d2's `about`, d10) depends on ADR-0051 landing;
  phases 1 and 2 work without it, a descriptor's `about` then being empty.

**What the literature says about choosing sources.** Fetched and quoted from the primary sources:

- Callan, Lu and Croft, *Searching Distributed Collections with Inference Networks*, SIGIR 1995
  (CORI): ranking collections uses "statistics that are analogous to tf and idf in normal document
  retrieval; for example, document frequency df (the number of documents containing the term) and
  inverse collection frequency icf (the number of collections containing the term)", and "it is
  possible to select subsets of the available collections for searching without affecting
  retrieval effectiveness". The same paper names the alternative OML can afford: "One approach
  would be to treat the distributed collections as a single, large, 'virtual' collection."
- Callan and Connell, *Query-Based Sampling of Text Databases*, ACM TOIS 19(2), 2001: CORI-class
  algorithms "can automatically select which text databases to search for a given information
  need, but only if given a set of resource descriptions that accurately represent the contents
  of each database."
- Si and Callan, *Relevant Document Distribution Estimation Method for Resource Selection*
  (ReDDE), SIGIR 2003: distributed retrieval has three sub-problems, "resource representation",
  "resource ranking" and "result-merging"; CORI "does not do well in environments with a mix of
  'small' and 'very large' databases", which ReDDE answers with "information about database sizes
  as well as database contents"; and, the warning this record's eval is built around, "better
  resource selection does not always produce better document rankings."
- Cormack, Clarke and Büttcher, *Reciprocal Rank Fusion outperforms Condorcet and individual
  Rank Learning Methods*, SIGIR 2009: "RRFscore(d ∈ D) = Σ_{r∈R} 1/(k + r(d)), where k = 60 was
  fixed during a pilot investigation and not altered during subsequent validation."
- Nogueira, Yang, Lin and Cho, *Document Expansion by Query Prediction*, arXiv 1904.08375, 2019
  (doc2query): "we propose a simple method that predicts which queries will be issued for a given
  document", the write-time mirror of a query-time hypothetical answer, which #201 adopts.

In OML's terms: a *resource description* is a workarea descriptor (d2) or the per-workarea
statistics ADR-0052 already keeps (d4); *resource ranking* is source selection (d4);
*result-merging* is unnecessary because every source shares one index (d7).

Decided, not built.

## Decision

- **d1 — A trusted source is a `committed` or `validated` workarea outside every in-progress
  subtree; no new flag.** Both statuses hold R2 and R3 without exception (ADR-0001 d5): every
  promoted row in them was reviewed by a non-author, and nothing in them is `self_signed`. A
  `development` workarea is never a source, even where the key reads it, because tenant policy
  may relax review there and its marked rows are "readable only inside that workarea" (ADR-0001
  d5); every node of an in-progress subtree is `development` by construction (ADR-0030 d1), so
  it is excluded twice. `universe` is permanently `committed` and is a source like any other,
  served under ADR-0044 d9 (no author, no source). A user root and the workareas under it reach
  `committed` with `same_user` review (ADR-0046 d1, d2) and are trusted for the only keys that
  read them, their user's. A tenant may narrow the set to `validated` alone (d5's
  `federation.min_status`), never widen it to `development`. A source is one **node**, judged on
  its own status: a `committed` parent with a `development` child is a source; the child is not.
  An *advertised* workarea is a trusted source with a promoted descriptor (d2) and `advertise` on
  (d5).
  The cost: "production-ready" is read as `committed`, the working state of a governed space,
  and not as `validated`, which is frozen; a tenant whose committed areas are drafts narrows to
  `validated`.

- **d2 — A descriptor is a reviewed claim of a new kind, `descriptor`, on the workarea it
  describes, one live per workarea.** Its claim key is fixed, `oml:descriptor`, and its format is
  `application/json` under ADR-0056's strict parse, with a closed schema:

    | Field | Written by | Holds |
    |---|---|---|
    | `summary` | the author | at most 600 characters: what the workarea holds and for whom |
    | `about` | the author | at most 8 terms, each a CURIE of a readable active term (ADR-0051 d2, d4), linked as `oml:about` edges ratified with the descriptor |
    | `questions` | the author | at most 8 questions the workarea's promoted rows answer (phase 3 adds generated ones, d10) |
    | `owner` | served, never authored | the workarea's crew record (ADR-0008 d7) or, in a user root, its user |
    | `status` | served | the workarea's ladder status |
    | `freshness` | served | the descriptor's promotion time and the workarea's latest promotion time |

  A writer key with writer access on the workarea writes it; a write naming `oml:descriptor` on
  any other workarea is refused `request.validation`, and a second live descriptor is a
  supersession, never a sibling (ADR-0050 d2). It is held, scanned (ADR-0065) and reviewed by a
  non-author dreamer on the same workarea, R2 against the lineage closure, like every claim; a
  validated workarea refuses a new descriptor as it refuses every write (ADR-0009 d1), so its last
  promoted descriptor stands. The descriptor is not a hit kind: it is never a recall candidate,
  never in `HIT_KINDS`, and it is served by the workarea read (`oml_workarea`) beside `brief`
  (ADR-0046 d9). The author-written fields are signed with the descriptor; the served fields are
  read at recall time and never signed, so a descriptor cannot lie about its own status or
  freshness.
  The cost: a new kind, one more row per advertised workarea, and a review per change. The brief
  (ADR-0046 d7) is not reused: it lists claims for an agent entering a workarea, and a descriptor
  describes the workarea to a query that has not entered it.

- **d3 — A descriptor is kept honest by review, by a check its reviewer sees, and by having
  nothing to gain.** Three layers, the third the one that holds:
  1. **Review.** The reviewer reads the workarea the descriptor describes (dreamer on it) and
     accepts or rejects the descriptor whole.
  2. **The answer check.** At hold time the server runs each `questions` entry as a `scope`
     recall inside the workarea under the tenant's default pipeline, and attaches an advisory
     finding, `descriptor.question_unanswered`, for each question with no promoted hit in its
     top 5, and `descriptor.term_unused` for each `about` term no promoted memory of the
     workarea is linked to. Findings never veto, as ADR-0050 d6's advisory findings do not; the
     reviewer sees them.
  3. **No gain from over-advertising.** Selection decides only which sources are searched (d4,
     d8). A selected source's rows then compete by their own scores under the normal pipeline,
     and no descriptor score enters a hit's score. A descriptor that claims topics its rows do not
     hold buys its workarea a search that returns nothing, and costs the caller one source slot.
     Telemetry measures that: `selection_yield`, per source, the share of selections that
     served a hit in the top `k`, for a tenant admin only (ADR-0054 d2's audience) and a doctor
     line below a floor the eval sets. Yield never feeds selection automatically.
  The cost: the answer check runs up to 8 recalls per descriptor version, off the request path,
  at hold time.

- **d4 — Source selection runs inside the core over the sources the key reads, and picks the top
  `max_sources` above a threshold, deterministically.** Per recall:
  1. **Candidates.** C = every node outside subtree(scope) where reader is effective for the key
     (the union over its reader grants' subtrees, `nodes_holding`), that is a trusted source (d1),
     accepts federated reads (d5), is not excluded by the scope's policy (d5), and passes the
     in-progress boundary rule (ADR-0030 d2). Computed per request from grants and denials, like
     every access decision, cached nowhere.
  2. **Score.** Each candidate gets a score from the readable statistics of the query's kept
     words, by mode:
     - `trusted` (phase 1): CORI's belief over ADR-0052 d5's per-workarea counts: for each kept
       word, `T = df / (df + K)` with `K = k · ((1 − b) + b · cw / avg_cw)`, `df` from
       `search_term_rows`, `cw` from `search_workarea_rows.total_len`, and `I` the inverse
       source frequency over C, summed over the words. `k` and `b` are set by the eval, unmeasured,
       #205. These are the counts the scope's own BM25 already sums, so the score reads nothing
       the key could not read.
     - `auto` (phase 2): BM25 over the descriptors of the advertised candidates, a descriptor one
       document, its `summary`, `questions` and the labels of its `about` terms one text, with
       the statistics of the candidates' descriptors alone (d8's index).
     - phase 3 adds two lists to `auto`, ontology overlap and generated questions (d10), fused
       with the descriptor list by reciprocal rank, k 60, as ADR-0049 d3 fuses retrievers.
  3. **Select.** The scope is always searched and never scored. The candidates are ordered by
     score descending, then by workarea name ascending (names are tenant-unique and immutable,
     ADR-0026), and the first `max_sources` with a score at or above `min_score` are selected.
     `trusted` has no threshold: it selects every candidate up to the cap, so with fewer
     candidates than the cap its score only explains.
  4. **Search.** The selected sources' nodes join the scope's served nodes in one
     `CandidateScope` (d7).
  The cost: a key whose grants reach many trusted workareas pays for a statistics read over all
  of them on every federated recall (d9).

- **d5 — Controls: `sources` on recall, default `scope`; policies per workarea and per tenant,
  each journalled.**
  - **`sources`** on `oml_read` recall and `POST /v1/memories/recall`: `scope` (today's answer
    exactly), `trusted` (d4, phase 1) or `auto` (d4, phase 2). Absent, it is the tenant's
    `federation.default_sources`, itself `scope` unless a tenant admin sets another, and a tenant
    default other than `scope` is admitted only for a mode that passed ADR-0049 d9's gate on the
    eval (d11). A mode the tenant has not enabled, or one not yet built, is refused
    `request.validation` with `details.field: /sources`, never answered as `scope`, as `terms` is
    refused today (`recall_inner`).
    The default is `scope` because it is the only one that changes nothing for any caller, keeps
    every existing eval number and every pinned expectation valid, and is the only mode with
    evidence today; `auto` becomes a default the way `bm25` did, by measurement.
  - **Per workarea**, served by the workarea read under `federation` beside `policy`, set by
    `POST /v2/workareas/{workarea}/federation` and `oml workareas federation`, journalled
    `workarea.federation_set {workarea_id, field, value, from}`, by a dreamer key `held_by:
    human` with dreamer access on the node or by a tenant admin (an agent key is refused
    `access.verb_ceiling`, reason `federation_is_human_only`, ADR-0030 d1's predicate):
    - `advertise` (default `false`): the descriptor may select this source under `auto`.
    - `accept_federated` (default `true` on a trusted source): recalls scoped elsewhere may
      search this workarea. Turning it off changes no access: a key that reads it still recalls
      there by naming it.
    - `exclude` (default empty, at most 64 workareas): sources never federated into a recall
      scoped at this workarea or below it.
  - **Per tenant**, `federation` in the tenant policy, journalled `federation.policy_set`, by a
    tenant admin: `enabled` (default `false`), `default_sources`, `min_status` (`committed` or
    `validated`), `max_sources` (default 8, unmeasured, #205), `min_score` (set by the eval,
    #206).
  The cost: three per-workarea fields and five tenant fields, each a refusal to name and a doctor
  line to keep.

- **d6 — Advertising grants nothing, and a key can never learn of a workarea it cannot read
  through recall; discovery without read is a directory workarea, not a new grant.** The
  candidate set of d4 is a subset of the nodes the key reads, so a descriptor, a statistic, a
  name or a count of an unreadable workarea never enters a score, a list or a response, and since the statements read only the candidates'
  rows, the work a recall does, and so its latency, depends on nothing unreadable. Leakage
  of an unreadable workarea's descriptor, by field:

    | Field | Leaks |
    |---|---|
    | name, path | the workarea's existence and often its project's name |
    | summary, `about`, `questions` | its topics, and sometimes its content in paraphrase |
    | owner | which crew or person works on it |
    | freshness, row counts | its activity and size |

  None of these is served to a key without reader on the workarea, in these three phases. A
  person who wants others to find a workarea they cannot read publishes a description as an
  ordinary reviewed claim in a **directory workarea** the audience reads (for instance a node
  `directory` under `universe` or under a crew): reader on the directory *is* the discover grant,
  its entries are reviewed text whose fields the publisher chose, and no live count or freshness
  is served from it. Reading the directory grants nothing on the described workarea; access is
  still a grant an admin issues. This needs no new role, no new grant attribute and no new rule,
  and it is buildable today.
  The cost: a directory entry is a copy that can go stale, and the federated recall does not use
  it to route (open question 3).

- **d7 — The selected sources are searched as one ranking with the statistics summed over the
  nodes searched; no per-source merge.** The scope's served nodes and the selected sources' nodes
  form one `promoted_nodes` list in one `CandidateScope`, and the tenant's pipeline runs once
  over it: BM25's `n`, `avgdl` and document frequencies are summed over exactly those nodes
  (ADR-0052 d5), so every hit's score is the score it would have in a single virtual collection
  of the searched sources, CORI's first approach, exact rather than approximated. Fusion across
  retrievers is ADR-0049 d3's and unchanged. Steering and guardrails stay structural, from the
  scope's ancestry (ADR-0025 d8): a federated source contributes hits only. Own held rows
  (`include_unreviewed`) and an in-progress scope's other authors' held rows stay on the scope's
  served nodes (ADR-0030 d3, d4); a federated source contributes promoted tips only. The cuts by
  `k` and `max_tokens` apply to the merged list as today (ADR-0025 d7).
  The cost: a scope hit can be outranked by a federated one, and scores of scope hits shift with
  the selected sources, since the statistics include them. Both are measured (d11), and a quota
  for scope hits is open question 1.

- **d8 — Explainability: the answer lists the sources it searched and why, and each hit says
  whether it came from the scope or a federated source.** The answer's `search` (ADR-0049 d6)
  gains `sources`: `{mode, considered, searched: [{workarea_id, workarea_path, via, score,
  matched}], cut}`, where `via` is `scope` or `federated`, `considered` is the size of C,
  `cut` the candidates above the threshold left out by the cap, and `matched` names the words,
  descriptor version and question or term that scored the source (`{words, descriptor_version,
  question, terms}`; empty for the scope). Each federated answer's hits gain `via`. Every name and number in it is
  of a node the key reads. Under `scope`, asked or defaulted, neither `sources` nor `via` is
  served, so today's answer is unchanged byte for byte.

- **d9 — Repeatability: the inputs are named, selection uses no clock, no randomness and no
  model, and ties break by name.** For fixed inputs — the key's grants and denials, the query's
  NFC text, `now`, the promoted rows and descriptors, the federation policies and settings, the
  pipeline and, in phase 3, the stored questions — the selected sources, their order and the hits
  are byte-identical across calls and processes, `recall_id` aside (#176). What changes the answer
  is listed on #176's page: a promotion, demotion, supersession or retirement in any searched or
  candidate source; a policy or setting change; a grant change; and, in phase 3, a question
  backfill under a new model or prompt version. `recall_events` gains `sources_mode` and
  `federated_workarea_ids`, so ADR-0054 d11's replay reproduces a federated recall; both fall
  under recall telemetry's erasure row (ADR-0054 d4).

- **d10 — Phase 3's two selection lists read reviewed content only.** **Ontology overlap**: the
  terms the query matches by ADR-0051 d6's label matcher, scored per candidate by the count of
  matched terms among its descriptor's `about` and among the `oml:about` edges of its promoted
  memories, from a per-workarea term count kept as ADR-0052 d5 keeps word counts. **Generated
  questions**: #201's questions, stored per promoted memory with model and prompt version, indexed
  as postings; a candidate scores by BM25 over its memories' questions, so a source is found by
  the questions its rows answer. No model runs at recall. The descriptor's own `questions` stay
  author-written and reviewed; generated questions are index data and never shown as a
  descriptor field (#201's "index data, not claims").

- **d11 — The recall eval gains a cross-workarea split, and every mode is measured against
  `scope` and against searching everything readable.** The 200-memory corpus is loaded as one
  `committed` workarea per `topic` (11, from `eval/recall/corpus.json`), and new labelled queries
  are scoped at one topic with relevant memories in others; the existing queries run scoped at
  their relevant memory's topic. Reported per mode (`scope`, `trusted`, `auto`, and the
  unselected union of every readable source as the upper reference): success@5, nDCG@10 and
  `unanswerable` false hits; selection precision (selected sources holding a relevant memory over
  selected) and selection recall (sources holding a relevant memory that were selected); and p95
  latency. The gate: a mode becomes a tenant default only if it beats `scope` on the cross split
  and no scope-answerable category falls by more than one query, ADR-0049 d9's rule; `auto` is
  kept only if it beats `trusted` or matches it at lower latency, since ReDDE's warning is that
  better selection need not rank better. A leakage suite runs with the eval: an unreadable
  workarea whose descriptor and rows match every query changes no byte of any answer.

- **d12 — Scale: a budget, a cap, an index already built, and no access cache.** Candidates per
  recall are bounded by the trusted nodes the key reads; descriptors per tenant by its
  workareas. Phase 1's score reads `search_term_rows` and `search_workarea_rows` by their primary
  key `(tenant_id, workarea_id, kind, memory_kind, token_id)` with `workarea_id = ANY($candidates)`
  and `token_id = ANY($words)`, leakproof equalities that serve the serving role (ADR-0052 d1);
  phase 2 indexes descriptor text as `search_postings` rows of memory kind 3, under the same
  partition, policy, trigger and erase, read with the candidates as the workarea list; phase 3's
  term counts and question postings follow the same shape. Nothing is cached beyond these
  materialised counts, so a revoked grant applies to the next recall. Budget: selection adds at
  most 10 ms p95 at 1,000 candidate sources, inside the 128 ms first-stage recall SLO
  ([operating](../operations/operating.md)); unmeasured, #205. `bm25-postings` measured 25.5 ms
  p95 at 99,721 readable memories at universe scope ([scale](../benchmarks/scale.md)), which is
  the shape of a federated search over many nodes. `max_sources` caps the search; above 1,000
  candidates a doctor line names the key and the count, and the cost is measured before a
  candidate cap is chosen (open question 5).

- **d13 — Three phases, each closed by its leakage tests.**

    | Phase | Builds | Issue | Leakage tests (each red first by breaking its clause) |
    |---|---|---|---|
    | 1 | `sources: trusted`, d1's trust set, d4's CORI score over existing counts, d5's policies, d7, d8, d9, the eval's cross split | #205 | an unreadable W changes no answer byte and is never named; a global and a per-grant deny each keep W out; a development or in-progress node is never a source; `scope` reproduces today byte for byte; `trusted` with federation off is refused |
    | 2 | kind `descriptor`, d3's check and yield, `advertise`, memory-kind-3 postings, `sources: auto` | #206 | a descriptor on an unreadable W changes no answer byte, `sources` list or score; a held, rejected or superseded descriptor selects nothing; hit scores are identical with and without an over-advertising descriptor |
    | 3 | d10's ontology and question lists, fused by RRF | #207 | a term or question held only by unreadable rows selects nothing and changes no count; questions exist for promoted memories only |

## Consequences

- `docs/architecture/retrieval.md` gains a section on source selection, and `workareas.md` the
  per-workarea `federation` policy, when phase 1 is built; `errors.md`, `openapi.yaml` and
  `http-api.md` gain `sources`, its refusals and `workarea.federation_set` then, not before
  (ADR-0039 d9).
- `oml-domain` gains the trust predicate as a pure function beside `effective_roles` and the
  in-progress boundary rule; `oml-app` computes candidates and selection; the store gains one
  statistics read (phase 1) and the descriptor postings (phase 2). No adapter decides a
  candidate (ADR-0049 d2).
- Two metrics with closed labels, `oml_recall_sources_total{mode}` and
  `oml_recall_federated_hits_total`, inside ADR-0054 d1's series budget. Doctor lines: federation
  enabled with `max_sources` 0; an `exclude` naming a workarea that no longer exists; an
  advertised workarea with no promoted descriptor; a descriptor with unanswered questions; a
  source whose `selection_yield` is below the floor; a key with more than 1,000 candidate sources.
- The constitution holds, row by row:

    | | R1 | R2 | R3 | R4 |
    |---|---|---|---|---|
    | `sources` | a read | — | searches readable rows only; no verdict is an input | — |
    | descriptor | a held write; the agent promotes nothing | a non-author dreamer on the workarea | invisible until promoted; once promoted it ranks sources by what it says, never by who approved it (ADR-0051 d11's argument) | superseded, never deleted |
    | policies | a person's or an admin's act, never an agent's | — | they change what is searched, never who reads | journalled |

- Issues: #205 (phase 1), #206 (phase 2), #207 (phase 3), each in the Later milestone and on
  #200's checklist; #176 gains federated recall's inputs; #201 gains d10 as a consumer.

## Rejected alternatives

| Decision | Alternative | Why not |
|---|---|---|
| d1 | **A new `production_ready` flag** | A second word for what the ladder already attests through a two-identity transition (ADR-0009 d3), and a field that can disagree with the status it would shadow. |
| d1 | **`validated` only** | Frozen areas are the minority; a governed working area is the source most recalls need. Kept as a tenant narrowing. |
| d1 | **Every readable workarea** | Brings `development` rows, including `self_signed` ones ADR-0001 d5 keeps inside their workarea. |
| d1 | **A source is a subtree** | A `committed` node can have `development` children; judging each node keeps the trust predicate one comparison. |
| d2 | **Descriptor as unreviewed metadata on the workarea row** | It would steer every reader's recall with text no second identity read, the one thing R2 exists to prevent. |
| d2 | **Descriptor in a central catalog workarea** | Its reviewers would not read the workarea it describes, so they could not judge it; d6 keeps that shape for discovery only. |
| d2 | **Reuse the brief (ADR-0046 d7)** | The brief is for an agent inside the workarea and is cut by a token budget; a descriptor is matched against queries from outside. |
| d2 | **A `memory` with a reserved key** | It would be a recall hit and compete with the rows it describes. |
| d3 | **Veto a descriptor that fails the answer check** | The check runs under one pipeline and could refuse an honest descriptor of a workarea whose vocabulary that pipeline misses; a reviewer judges. |
| d3 | **Feed `selection_yield` back into selection** | Ranking by usage is ADR-0054's bounded, opt-in pipeline; doing it here would add a second, unbounded usage loop. |
| d4 | **ReDDE instead of CORI in phase 1** | ReDDE estimates relevant documents from a centralised sample; OML has the complete counts, so CORI over exact `df` and `cw` needs no sample. ReDDE's size correction is what the eval checks CORI for (d11). |
| d4 | **A query-time model (HyDE, an LLM router)** | A model call per recall, latency and variance, against #176 and the model-free default; #201 moves the model to write time. |
| d4 | **Embedding similarity over descriptors** | ADR-0048 puts meaning last, gated on a measured paraphrase gap; phase 3's questions are the lexical attempt first. |
| d5 | **Default `trusted` or `auto`** | Changes every existing answer without evidence; ADR-0049 d9 moves a default only by the gate. |
| d5 | **A new tool, `oml_federated_recall`** | ADR-0039 d8: a parameter first. |
| d5 | **Silently answer `scope` when federation is off** | Fails open in the sense that matters: the caller believes it searched more than it did. Refused by name, as `terms`. |
| d5 | **Let an agent set the policies** | They change what other keys' recalls search; ADR-0030 d1 makes such marks a person's act. |
| d6 | **A `discover` grant role** | A new role in the ceiling matrix, a grant that reads something without reader, and a second visibility rule to audit; a directory workarea gives the same capability with reader alone. |
| d6 | **Route through descriptors of unreadable workareas and say "ask for access"** | Every field in d6's table leaks, and a route that names a workarea the key cannot read is a disclosure. |
| d7 | **Search each source separately and fuse by RRF** | RRF discards score magnitude, so a weak source's first hit ties a strong source's; N searches instead of one; and the per-source statistics would make the same row score differently by where it was searched. One shared index makes the single virtual collection exact. |
| d7 | **Per-source score normalisation (CORI merge)** | A heuristic for engines that do not share statistics; OML's do. |
| d7 | **Weight hits by their source's selection score** | The descriptor's author would then move hit scores, which d3's third layer forbids. |
| d12 | **Cache each key's candidate set** | Effective access is cached nowhere ([workareas](../architecture/workareas.md#grants-bind-to-node-ids)); a revoked grant must apply to the next recall. |

## Open questions for the operator

1. **A floor for scope hits.** Should a federated recall reserve part of `k` (for example half)
   for the scope's own hits, or let the one ranking decide? This record lets the ranking decide
   and measures crowding (d7, d11).
2. **`accept_federated` default.** On for every trusted workarea (this record), or off until an
   owner opts in? On changes nothing until a tenant enables federation and a caller asks for it.
3. **Discovery.** Is the directory workarea of d6 enough, or should a later phase route a recall
   to a directory entry and answer "a workarea you cannot read may hold this, ask its owner"? This
   record says no, because of d6's leakage table.
4. **Who writes a descriptor.** Any writer on the workarea (this record), or only a person's key,
   so that an agent never drafts what steers other crews' recalls? Review by a non-author holds
   either way.
5. **Candidate cap.** Above how many candidate sources should a federated recall refuse or
   narrow, once d12's budget is measured? And should a key's federated reach be limited to its
   default workarea's crew, its tenant, or anything it reads?
6. **Cross-tenant federation.** Out of scope here: every source is in the key's tenant
   ([workareas](../architecture/workareas.md#the-tree)). Confirm it stays out.
7. **`universe` under federation.** Should a non-user-root scope federate into `universe` (this
   record: yes, as a committed source), and should a user root's recall still search it under
   `scope`, as ADR-0044 d10 does today (this record: yes, unchanged)?
