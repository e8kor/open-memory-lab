<!--
id: architecture/retrieval
tier: engineering
status: design draft for review · decided, not built
answers: where reads are served from and in what order; how bodies are decrypted; how reads are logged; which read-shaped features are out of scope; which steering applies and how hits are ordered; how own held rows join recall; how held claims inside an in-progress workarea join recall and its indexes; how a recall budget is counted and cut
-->

# Retrieval: reads are served from Postgres, journalled

**Status:** design draft for review · decided, not built

This page is the single declaration site for the read path (D12): where reads
are served from, how the serving indexes are laid out, what order results come
back in, how bodies are decrypted, how reads are logged, and which read-shaped
features are out of scope. The storage decisions it builds on — the commit
point, the outbox, the lake — are declared in [storage.md](storage.md); the
schema of the tables named here is in
[data/postgres-schema.md](../data/postgres-schema.md); the simulation fence
whose read predicate this page applies is declared in
[simulations.md](simulations.md); the reader role whose reads are logged is
declared in [roles-and-keys.md](roles-and-keys.md).

## The rule: Postgres serves, the lake never does

Every client read — recall, fetch, FTS, vector search, graph and lineage
reads — is served from PostgreSQL. **Delta Lake serves no client read.** The
lake is a derived body store for reconciliation, lineage, metadata analytics
and future journalled exports ([storage.md](storage.md)); putting it on the
read path would make a derived copy an authority, and the design refuses that.
The cost is accepted plainly: Postgres must hold everything serving needs —
promoted bodies reachable for decryption, `tsvector` entries, embeddings — and
the Postgres scale ceiling that follows is a tracked FR in
[product/roadmap.md](../product/roadmap.md) (Postgres scale ceiling and
partition strategy), not a solved problem.

Reads return **promoted rows only**, within the key's effective access
([workareas.md](workareas.md)). Held rows are visible solely to their author —
the calling key ([own](roles-and-keys.md#own)) — through own-work status, get,
and recall with `include_own_held` ([own held rows in
recall](#own-held-rows-in-recall)); demoted and erased rows are not served. This
is R3 made operational: the gate changed visibility, and the read path is where
that visibility is enforced. Under the proposed
[ADR-0030](../adr/ADR-0030-in-progress-workareas.md), binding only on its
acceptance, the one place another key reads a held row is
inside an [in-progress workarea](workareas.md#in-progress-workareas), for the
keys the [permission matrix](roles-and-keys.md#permission-matrix) allows, each
row marked `reviewed: false`; a read scoped outside that subtree serves nothing
from it, promoted rows included, until its commit
([below](#held-claims-inside-an-in-progress-workarea)).

`self_signed` rows — the constitution's bounded development exception
([workareas.md](workareas.md)) — are served only to reads whose scope node is
their own workarea: a recall, its steering segment or a listing scoped
anywhere else, at an ancestor or at a descendant, never returns them, and an
events tail shows their events only when its `workarea_id` is that workarea
([events tail](../api/http-api.md#events-tail)). Get resolves no workarea and
takes a row's own workarea as its implicit scope, so a non-own `self_signed`
version is readable by id by a key with reader effective access on its own
workarea, and by no other key
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d3, d8).

## The serving tier

Two indexes over promoted rows, both in Postgres, both under tenant RLS:

- **Full-text search**: `memories_fts`, a `tsvector` index over promoted claim
  text.
- **Vector search**: `memory_embeddings`, embeddings over promoted claim text.

Of the rows they admit, the indexes hold every promoted version, superseded
ones included. Which of them recall serves is the served-tip predicate of the
[hits rule](#the-recall-ordering-contract-steering-rows-first-unranked), not
index membership: it keeps superseded versions, retirement versions and every
version of a retired claim
([audit-and-lineage.md](audit-and-lineage.md#supersession-chains-claim_key-and-the-tip))
out of the hits, so a ratified retirement, or a later supersession of one,
changes no index row. Superseded versions are readable history under the same
access, so the predicate hides nothing a reader may not see. Held rows enter
neither index, except, under the proposed ADR-0030, inside an in-progress
subtree: elsewhere a held row is
matched only through the author-fenced read of
[held text containment](#held-text-containment), and inside one the indexes
hold held and promoted rows alike behind a fence keyed by the subtree
([the in-progress fence](#the-in-progress-fence)).

Two kinds are excluded from both shared indexes by default: vocabulary rows
(`term`, `term_mapping`, `vocab_release` — a 50k-term vocabulary must not
drown a tenant's organic memories in every hybrid recall; a vocabulary
workarea's journalled policy may opt in, and its vocabulary rows are then
[hits](#the-recall-ordering-contract-steering-rows-first-unranked),
[ADR-0021](../adr/ADR-0021-ai-human-ontologies.md))
and, when ADR-0020 lands, imported rows under its `index_policy` default. The
exclusions mirror each other deliberately: reference data is fetch- and
filter-served unless a journalled policy prices it into the shared indexes.

Recall accepts a `terms` filter (CURIE annotations, ADR-0021) and
`expand_terms: descendants`, which widens that filter through the
**materialized per-release `is_a` closure** before retrieval — a lookup, never
a graph walk at query time. Expansion resolves per CURIE against the release
pinned by the vocabulary that owns the expanded prefix; steering order, `k`
and the [token budget](#token-budgets) are untouched by expansion. A server
that does not yet serve `terms` or `expand_terms` refuses them by name with
`request.validation`, never ignores them
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d16).

### Text handling

`memories_fts` rows are built with the Postgres text-search configuration named
by the deployment setting `search.fts_config`
([configuration](../operations/configuration.md)), **default `simple`**. The
default is chosen for honesty, not quality: `simple` stems nothing and
language-detects nothing, so it never silently corrupts tokens of a language
the configuration did not expect — a multilingual corpus under an
English-stemming configuration degrades non-English recall without any signal
that it has. The cost is stated plainly: with no stemming, morphological
variants do not match — a query for "deploy" does not find "deployments" — and
FTS recall for morphology-rich languages is lower than a language-specific
configuration would give. How much lower is unmeasured, like every retrieval
number (FR-4).

Two consequences of the setting's scope:

- **Changing it implies a full FTS reindex**: every `tsvector` row was built
  with one configuration, and rows built under different configurations do not
  compare. The setting is deployment-level in v1 for exactly this reason; a
  per-tenant (or per-workarea) FTS configuration is a tracked deferral in
  [product/roadmap.md](../product/roadmap.md) (per-tenant FTS configuration),
  not a quiet gap.
- **Input is already canonical**: claim text is NFC-normalized at admission,
  before digesting and scanning ([admission](admission.md) owns the mechanics),
  so the index never holds two byte spellings of one string.

### pgvector layout

`memory_embeddings` is **list-partitioned by `tenant_id`**, with a
**per-partition HNSW index** and RLS on top of the partitioning. The
partitioning is a performance and blast-radius choice layered under the
security boundary, not a substitute for it: RLS is the isolation authority
(declared in [security.md](security.md)); the per-tenant partition means one
tenant's index build, vacuum or bloat never degrades another's graph, and an
HNSW graph never spans tenants. Under the proposed ADR-0030 each tenant
partition is itself list-partitioned on `in_progress_root_id`, so an
in-progress subtree's vectors sit in a graph of their own that no recall scoped
outside it scans before its commit ([the in-progress fence](#the-in-progress-fence)).

Two query-shape decisions:

- **Iterative scan is enabled.** HNSW with a post-filter (RLS plus any
  workarea predicate) can return fewer than `k` rows when filtered candidates
  are discarded; iterative scan keeps pulling candidates from the graph until
  the limit is satisfied or the configured scan ceiling is reached
  (`hnsw.max_scan_tuples` and the scan-memory bound), trading extra index
  traversal for far less silent under-returning. The ceiling and its tuning are
  part of FR-4's measurement — under-return is reduced, not eliminated. The
  trade is still the point: a filtered search that quietly returns 3 of 10
  requested hits is a correctness bug wearing a latency costume.
- **Subtree filtering is a filter column, not a join.** Each embedding row
  carries a workarea-path filter column (the derived `ltree` path of its
  workarea — paths are derived, node ids are the authority; see
  [workareas.md](workareas.md)). A subtree-scoped query filters on path prefix
  directly against the index scan rather than joining through `workareas` per
  candidate. The cost is that the column must be maintained when a subtree is
  re-parented; how that maintenance is scheduled is an implementation choice
  inside the journalled re-parent act declared in
  [workareas.md](workareas.md).

### What is unmeasured, said plainly

Nothing has run. **Recall quality, query latency, and the exact-vs-approximate
behaviour of filtered HNSW at our data shapes are unmeasured** — tracked as
the pgvector recall/latency FR in [product/roadmap.md](../product/roadmap.md),
with measurement as the exit criterion. The choice of embedding model and
where it runs is an open RS on the same page. No vector-search number is
asserted anywhere in this design until that measurement runs (D17).

### Indexing

Embeddings are computed by `oml-indexer`, a separate workload running under
its own **key-less** credentials per tenant — it holds no signing key and no
verdict authority; it reads promoted text on the serving side of decryption,
and the held text of claims inside an in-progress subtree, which `oml-server`
decrypts from its placement and serves to it
([the in-progress fence](#the-in-progress-fence)), and writes embedding rows. It
holds no quarantine credential and no decrypt grant. Everything it produces is plaintext-derived and
therefore in the erasure inventory: on erase, embedding and `tsvector` rows
are removed from live tables, and their persistence in base backups and WAL
until the named backup retention window expires is stated honestly where the
inventory is declared ([audit-and-lineage.md](audit-and-lineage.md)).

## The recall-ordering contract: steering rows first, unranked

A recall result has two segments, in this order:

1. **Steering rows** of the queried scope — every applicable steering row,
   **unranked**. Steering rows are not hits; they are standing instructions
   the workarea's governance promoted (a steering row is held and
   non-author-reviewed like any claim; see [admission.md](admission.md)). They
   precede hits precisely because they must not compete with hits for rank: a
   steering row that relevance-scoring could bury is a steering row that
   fails exactly when an agent most needs the instruction.
2. **Hits**, ranked by the query (FTS rank, vector distance, or the fused
   order of a combined query — fusion weighting is an implementation choice
   inside this contract and is unmeasured like the rest of the tier).

Membership and order of both segments are fixed
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d8):

- **Applicable steering** for scope node N is every promoted, served-tip row
  of kind `steering` that is not part of a retired claim, whose workarea is N
  or an ancestor of N, within the key's reader effective access. A
  descendant's steering belongs to another crew and does not apply at N. A
  `self_signed` steering row applies only when N is its own workarea; it never
  applies at a descendant scope.
  Segment 1 is ordered structurally, never by the query: depth from the
  `universe` root ascending, then `promoted_at` ascending, then `artifact_id`
  ascending. Steering never appears among the hits, and the `kinds` filter
  applies to hits only.
- **Hits** are served tips (tip, state `promoted`) of kind `memory` or
  `derived_fact` — plus, in a workarea whose journalled policy opts the
  vocabulary kinds into the shared indexes
  ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)), served tips of kind
  `term`, `term_mapping` or `vocab_release` — never retirement versions, never
  versions of retired claims, plus the caller's own held rows when it asks for
  them ([below](#own-held-rows-in-recall)), plus, at a scope inside an
  in-progress subtree, the held claims of other keys under `include_unreviewed`
  ([below](#held-claims-inside-an-in-progress-workarea)). A `self_signed` version is a hit
  only when the scope node is its own workarea. The `kinds` filter names only
  `memory` and `derived_fact`, as ADR-0021 d9 keeps it: a recall that sends it
  narrows the hits to those kinds, and one that omits it also receives
  opted-in vocabulary hits. The order is total: score descending,
  then `artifact_id` ascending in byte order. The score is the store's fused
  score; where a store ranks by full text alone, as the PoC does, it is to be
  `ts_rank(to_tsvector('simple', body), plainto_tsquery('simple', query))`
  with normalization 0, compared as `float4`; in the in-memory test store it
  is the count of distinct query terms matched. Equal order across stores is
  not asserted. A superseded version is never a hit; it stays fetchable by id
  as history.
- **Served tip**, in both bullets above: for a key in a simulation's
  audience, a claim's served tip is its overlay tip for that simulation where
  one exists and shadows the default tip
  ([audit-and-lineage.md](audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)),
  and the default tip it shadows is then neither a hit nor applicable steering
  for that key; for every other key the served tip is the default tip
  (`memories_served_tip`), and overlay tips are never served
  ([the simulation read predicate](#the-simulation-read-predicate)).
- **Determinism** holds over a fixed candidate list: the same candidates
  always come back in the same order. Whether approximate vector search fixes
  the candidate list for a given query is part of FR-4's measurement, not a
  property this page claims.

The contract is part of the API surface ([api/http-api.md](../api/http-api.md)
and the MCP `recall` tool): clients may rely on segment order, and nothing a
client sends re-ranks segment 1.

How the result is assembled is a hot-path rule, not an implementation accident
([ADR-0024](../adr/ADR-0024-performance-foundation.md) d4): steering rows, hits
and bodies hydrate in **one or two prepared statements, fetched set-wise, never
one query per hit**; fusion of the FTS and vector candidate lists is computed
in one pass, never a third query round; and the server-side
[token budget](#token-budgets) is a performance feature as well as an
ergonomics one — rows the budget cuts are never decrypted and never cross the
wire.

## Own held rows in recall

A recall with `include_own_held: true`
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d4) also takes
as hit candidates the calling key's [own](roles-and-keys.md#own) rows that
are:

- in state `held`;
- of kind `memory` or `derived_fact`, and not retirement versions;
- in a workarea within subtree(scope) ∩ the key's reader effective access,
  minus its global denials;
- when a simulation row or sim-derived, only while the key is in that
  simulation's audience ([own](roles-and-keys.md#own));
- matched by the query;
- admitted by the request's `kinds` filter when one is sent, as promoted hits
  are.

They are ranked by the same total order as promoted hits and interleaved in
`hits`, each with `state: held` and `tip: false`, and they count against `k`
and the [token budget](#token-budgets) exactly like promoted hits. A held
successor carries `replaces`; the promoted tip it would replace stays among
the hits until ratification, and the two are never deduplicated. Own held
steering and own held retirement versions never enter recall — neither is in
force — and own-work status and get reach them. `include_own_held` defaults to
`false`, which is exactly the prior behaviour; no other key's result changes,
and the recall ceiling stays reader on the scope, so a writer-only key reads
its held rows through own-work status and get. An own held row inside an
[in-progress subtree](workareas.md#in-progress-workareas) joins only a recall
scoped inside that subtree: a recall scoped outside skips the subtree whole.

Held rows have no embedding, so a held row's score is the fusion function
evaluated with its full-text rank and no vector term: the vector component
contributes exactly what an absent candidate list contributes under the fusion
in use. Held and promoted candidates are then merged under the one total order
— score descending, then `artifact_id` ascending in byte order — before the
`k` cut, so, given the same scored candidates, the interleaving, the `k` cut,
the budget cut and the read-log ids are a pure function of them: on one
server, over a fixed candidate list, the same request always serves the same
rows. Across servers they agree only where the scores agree, since fusion is
an implementation choice. In the PoC both scores are to be
`ts_rank` under the same `simple` configuration and `plainto_tsquery`, so they
compare directly. Because a held row has no vector term, under any fusion in
which an absent list contributes nothing and a vector match never lowers a
score, it scores no higher than it would once promoted, so own held rows rank
at or below their promoted rank; this affects only the author's own result,
and how far below is unmeasured ([FR-4](../product/roadmap.md)). In cluster
mode the request is an own-held read
([reads in cluster mode](#reads-in-cluster-mode)).

## Held claims inside an in-progress workarea

Proposed in [ADR-0030](../adr/ADR-0030-in-progress-workareas.md), binding only on
its acceptance: this section is the single declaration site for how recall and its indexes
treat an [in-progress workarea](workareas.md#in-progress-workareas)
(ADR-0030 d8, d9, d10). Which keys
may read the held claims of other keys there is declared once, in the
[permission matrix](roles-and-keys.md#permission-matrix) (footnote 6); this
section calls that answer, for a node, the key's **unreviewed access** there.

**`include_unreviewed`.** `RecallRequest` and `oml_recall` take
`include_unreviewed` (boolean). Its default is `true` when the key has
unreviewed access on the scope node, and `false` otherwise.

- **True, at a scope inside an in-progress subtree.** The hit candidates also
  include every held version, of any author, of kind `memory` or
  `derived_fact`, never a retirement version, that is in a workarea within
  subtree(scope) where the key has unreviewed access, and that the query and
  the `kinds` filter admit, under the simulation predicate and global denials.
  The key's own held rows in those workareas are among them; `include_own_held`
  adds its own held rows elsewhere in the subtree, matched as the fence section
  below declares, and nothing is served twice.
  A workarea in the subtree where a reader grant blocks contributes
  its promoted hits only — absence, as a per-grant deny is in any subtree read.
- **True, at a scope node inside an in-progress subtree where a reader grant
  effective on it carries `unreviewed: block`.** Refused with
  `access.grant_refused`, details `{workarea_id, needed_role: "reader",
  unreviewed: "block"}`: the request names rows that exist and that the key's
  grants withhold, and narrowing silently would be coercion. A recall that
  leaves the field to its default never asks for them, so it never refuses.
- **True, at a scope outside every in-progress subtree.** Nothing is added and
  nothing is refused, whatever a reader grant effective there carries: no held
  claim of another key is readable there (R3).
- **False.** Promoted hits only, plus own held rows under `include_own_held`.

The added hits merge with the promoted hits under the
[one total order](#the-recall-ordering-contract-steering-rows-first-unranked)
before the `k` cut and the [budget](#token-budgets), each with
`state: "held"`, `tip: false` and `reviewed: false`. A held successor carries
`replaces` and is listed beside the promoted tip it would replace, never
merged with it. Applicable steering stays promoted steering only, so a held
steering row never enters segment 1, and a held retirement version never takes
a claim out of recall; get and the listings reach both. Absent, the field
gives exactly the prior answer at every scope that could exist before
ADR-0030, since no in-progress subtree existed.

**Outside.** A recall scoped outside an in-progress subtree — at any ancestor,
up to `universe` — serves nothing from it, the caller's own held rows included,
until its commit: no steering, no hits, and no candidate counted in `k`, the
budget's `hits_omitted` or `next_hit_tokens`. The serving indexes cannot match
those rows at such a scope, and the subtree's vectors sit in a graph that a
recall scoped outside does not scan (below), so what the subtree holds changes
neither which hits an outside recall returns nor how many.

### The in-progress fence

- **Designed form (proposed, not built).** Every non-simulation held and promoted version in a
  workarea inside an in-progress subtree has the `memories_fts` row and the
  `memory_embeddings` row it would have once promoted outside one, a held one from its capture
  on, so a held claim and the same claim once promoted score alike: ratification changes no
  index row and no score, and review never reorders results (R3). Each such row carries
  `in_progress_root_id`, the in-progress workarea whose subtree holds it, and so does the
  version's `memories` row, stamped by the capture's round trip two and never changed, since no
  version crosses the boundary while its root is in progress
  ([data/postgres-schema.md](../data/postgres-schema.md#full-text-search-and-embeddings)).
  A restrictive row-level security policy on each index table serves a row whose root is still
  `development` only to a transaction whose `oml.in_progress_root` names that root; its
  `WITH CHECK` equals its `USING`, so an insert or update of such a row passes only there, and
  on `memory_embeddings` it is created on the parent and on every partition below it:
    - **the subtree's own vector graph.** Each tenant partition of `memory_embeddings` is itself
      list-partitioned on `in_progress_root_id`: one partition `FOR VALUES IN (NULL)` holds the
      rows with no root, and each in-progress root has a partition of its own with its own HNSW
      index, which `workarea.create` with `in_progress: true` creates as a table and attaches in
      its own transaction, before round trip one, so the chain lock holds none of that DDL. The
      attach takes `SHARE UPDATE EXCLUSIVE` on the tenant partition, and `ACCESS EXCLUSIVE` on a
      default partition only if one exists, which it then scans
      ([ALTER TABLE](https://www.postgresql.org/docs/current/sql-altertable.html), fetched 2026-10-31); no `DEFAULT` partition exists, so the attach locks and scans no
      other partition, and an insert whose root has no partition fails rather than landing in a
      shared graph. A recall scoped inside
      scans its root's partition, which holds every embedded version of subtree(scope). A recall
      scoped outside scans the no-root partition and the partitions of committed roots only,
      never one whose root is `development`, so held writes inside use up no part of an outside
      scan's ceiling and change neither which hits it returns nor how many. After the commit an
      asynchronous move takes a committed root's rows into the no-root partition, clearing their
      `in_progress_root_id`, and drops the root's partition once it is empty and no indexer work
      item names that root; until then an outside recall reads both and deduplicates by
      `(artifact_id, version)`, so no row is served nowhere at any moment and nothing moves under
      the chain lock. A unique or primary key constraint on a partitioned table must include every
      partition key column ([Table Partitioning](https://www.postgresql.org/docs/current/ddl-partitioning.html), fetched 2026-10-31), and this key must admit null, so
      `memory_embeddings` carries no primary key: each leaf partition
      carries a unique index on `(artifact_id, version)`, and each version's row is inserted
      once, under its root, and moved, never copied. The DDL's cost and the move's cost are
      unmeasured ([FR-25](../product/roadmap.md));
    - **simulation rows.** A simulation row keeps its per-simulation index. One inside an
      in-progress subtree is indexed there from its capture on, carrying `in_progress_root_id`
      under the same policy, so its score does not change at ratification and an audience key's
      recall scoped outside skips it too. The per-simulation index's DDL is undeclared, an open
      detail named on the
      [Postgres schema](../data/postgres-schema.md#full-text-search-and-embeddings);
    - **the root.** A version's root is the nearest ancestor-or-self of its
      workarea with `in_progress = true` and `status = 'development'`, or none.
      `oml-server` computes it in round trip one's statement, beside
      `pg_advisory_xact_lock` and the chain-head read
      ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d1), and sets
      `oml.in_progress_root` to it with `set_config(…, true)`,
      transaction-local, never from the client;
    - **the row lock comes before the chain lock.** A rejection, demotion or erasure locks the
      version's `memories` row `FOR UPDATE` in a sub-select that round trip one's advisory-lock
      call consumes, for example
      `SELECT pg_advisory_xact_lock($tenant), <chain-head read> FROM (SELECT 1 FROM memories WHERE tenant_id = $t AND artifact_id = $a AND version = $v FOR UPDATE) r`,
      so its wait behind an `oml-indexer` insert that holds the row `FOR SHARE` happens before
      the chain lock is held, and a statement that returns no row aborts the transaction;
    - **placement is decided under the chain lock.** Round trip two's
      statement, which runs once the lock is held, reads the root's status
      again: a capture or a reopen into `held` inserts a `memories_fts` row stamped with that
      root only while the root is still `development`, and otherwise a `held_fts` row with no
      root, so a capture serialized after a commit is an ordinary held row. A reassignment
      between two nodes of one subtree, into `held` or not, inserts nothing: it is the checked
      `UPDATE` of the existing rows' workarea and path. The `tsvector` is still computed before
      `BEGIN`, since it does not depend on the placement;
    - **reads.** `oml-server` sets the setting, transaction-local, at the start
      of every read transaction to the resolved scope node's root, and leaves it
      empty at a scope outside;
    - **writes and removals.** Every statement that writes or removes a fenced
      index row runs under the version's own root, set the same way: the
      capture insert, `oml-indexer`'s insert, the reassignment `UPDATE` of the
      rows' workarea and path between two nodes of one subtree, and the removals
      at rejection, demotion and erasure;
    - **re-parenting.** A re-parent rewrites the `workarea_path` of fenced `memory_embeddings`
      rows once per in-progress root in the moved subtree, each `UPDATE` preceded by
      `set_config('oml.in_progress_root', <root>, true)`, plus one `UPDATE` with the setting
      empty for the unfenced rows. The sum of the affected counts is checked against the count
      of the moved subtree's embedding rows, counted under the same settings, and any mismatch
      aborts with `system.internal`;
    - **the checks.** For a version whose state at the transaction is `held` or `promoted` (the
      rejection of a held version, the demotion of a promoted one, the erasure of a held or
      promoted one) the `memories_fts` removal is checked to affect exactly one row, and so is
      the reassignment `UPDATE`; any other count aborts the transaction with `system.internal`.
      An erasure of a rejected or demoted version inside runs no `memories_fts` or
      `memory_embeddings` statement, since its rows were removed at that transition. Both index
      rows sit behind one fence under one setting, so a passing check shows the embedding
      statement ran where its row is visible. No `held_fts` statement runs for a version
      inside, and none is checked
      ([held text containment](#held-text-containment));
    - **other reads.** The scoped listing, the graph and lineage reads, get and the attachment
      fetch read `memories` through one SQL function, `served_memories(scope_root)`, that
      applies the boundary on `memories.in_progress_root_id`, and no read path selects
      `memories` directly for another key's row. The events tail applies the
      [boundary condition](../api/http-api.md#events-tail) in `events_with_subjects`. Outside
      the indexes the boundary is a predicate held by the port-contract tests of the PoC form
      below, not a fence, a residual [security](security.md) states.

    The policy reads the root's status, so the commit, which changes one
    `workareas` row, makes every one of those rows matchable at scopes outside
    in the same transaction, with no index or lake row rewritten. The commit's held test is the
    one statement under the chain lock that the subtree reaches: one probe of the partial index
    `memories_held_by_root`, unmeasured ([FR-25](../product/roadmap.md);
    [ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d12). A query of `memories_fts` or
    `memory_embeddings` scoped outside that forgets the boundary sees nothing behind it, and its
    scan of each outside graph is unchanged by what an inside graph holds — containment by
    construction at the serving indexes, the stance of
    [ADR-0013](../adr/ADR-0013-reads-served-from-postgres.md) d3. The policy's
    per-row cost is unmeasured ([FR-4](../product/roadmap.md)).

    **The indexer.** `oml-server`, never the indexer, chooses the root. The
    indexer's work item carries `(artifact_id, version, in_progress_root_id)`,
    written by the transaction that made the version indexable with the root
    that transaction placed it under. `oml-server` decrypts the held body from
    its placement ([admission](admission.md#quarantine-placement)) and serves the
    plaintext to the indexer, which holds no quarantine credential and no
    decrypt grant. The indexer's insert is one `INSERT … SELECT` over the
    version's `memories` row, taken `FOR SHARE`, that writes the embedding with
    the row's current `workarea_path` only while the row's state is `held` or
    `promoted`, in a transaction that sets `oml.in_progress_root`,
    transaction-local, to the root its work item carries. A rejection, demotion
    or erasure whose transaction ends first leaves that insert nothing to write;
    one that arrives while the insert holds the row waits for it, before taking the chain lock,
    and then removes the embedding it wrote. The indexer's Postgres role holds exactly the
    privileges [security](security.md) lists for it.

    **Inside, the fence does not separate authors.** It filters by root alone,
    and the index rows carry no state and no author. A key whose grants block, a
    recall with `include_unreviewed: false` and a node a per-grant deny masks are
    kept from other keys' held rows by the recall predicate, which joins each
    candidate to its `memories` row for state and author — not by the fence. An
    own held row inside is matched the same way: `memories_fts` joined to
    `memories` on `author_key_id` under the caller's [own](roles-and-keys.md#own)
    key set, since `held_fts` holds none there.
- **PoC form (proposed, not built).** The PoC's two stores, `PgStore` and the `oml-app` double
  `MemStore`, have neither `memories_fts` nor `memory_embeddings`: its migrations search
  `memories` through a GIN index on `to_tsvector(claim)`. Its form is a port change.
  `recall_fts` gains `unreviewed: &[Uuid]`, the nodes of subtree(scope) where
  [footnote 6](roles-and-keys.md#permission-matrix) holds, and `own_key_ids: &[Uuid]`; on
  `unreviewed` it matches held versions of any author, and on `readable` promoted served tips.
  The use case computes both sets, and at a scope outside it excludes every node of a
  `development` in-progress subtree from both. `list_memories`' `ListingFilter` gains the same
  `unreviewed` set, and `events_with_subjects`, and the graph read once one is built, take the
  same exclusion. Port-contract tests run against both stores are to assert, whatever the
  query: that a read scoped outside never returns a row inside, on each of those reads; that a
  blocked key's recall never returns another key's held row inside; and that a capture
  serialized after the root's commit is returned to another key by no recall, listing or tail
  scoped at a node of the former subtree, and to its author only under `include_own_held`. The
  code and README are to state the gap to the designed form.

## Held text containment

Outside every in-progress subtree, held text is matched only through a read
fenced to its author, never through the shared serving indexes
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d5) — the same
containment-by-construction stance as the
[simulation read predicate](#the-simulation-read-predicate): a forgotten
predicate fails toward invisibility.

- **Designed form (decided, not built).** A `held_fts` table holds one
  `tsvector` row per held version, keyed by author key and workarea
  ([data/postgres-schema.md](../data/postgres-schema.md#full-text-search-and-embeddings)).
  Every transition into held inserts the row, in the transaction that makes
  the transition: the capture transaction, with the `tsvector` computed from
  the NFC plaintext before `BEGIN`; a [reopen](admission.md#reopening-a-rejection),
  and a reassignment that re-enters held, with the `tsvector` computed on the
  serving path under the same decrypt rule as get. Every transition out of held
  — promotion, rejection and erasure among them — removes it in the transaction
  that makes the transition. A reassignment updates the row's `workarea_id` in
  its own transaction, so the read-access filter of
  [own held rows in recall](#own-held-rows-in-recall) never reads a stale node,
  and a reassignment that re-enters held removes the version from
  `memories_fts` and `memory_embeddings` in that same transaction. Besides tenant
  RLS the table carries an author policy over the caller's own key set,
  `author_key_id = ANY (string_to_array(current_setting('oml.own_key_ids'), ',')::uuid[])`:
  `oml-server` sets `oml.own_key_ids` from the authenticated key and its
  journalled `rotated_from` chain at the start of every transaction, never
  from the client, so a session reads only the held rows of its
  [own](roles-and-keys.md#own) keys and a rotated key still matches its
  predecessor's. Under that policy a row the setting does not match is
  skipped silently, so a forgotten predicate fails toward invisibility on
  reads but toward retention on removals. Every transaction that moves a
  version out of held, or reassigns a held version, therefore checks that its
  `held_fts` DELETE or UPDATE affected exactly one row, and the capture insert
  checks that it inserted one; any other count aborts the transaction with
  `system.internal`, so the transition fails rather than leaving held text
  behind its held state. Its rows are `tsvector` entries, and the
  [erasure inventory](audit-and-lineage.md#the-erasure-inventory) lists the
  table on its own row.
  `memories_fts` and `memory_embeddings` stay promoted-only outside every
  in-progress subtree, and `held_fts` holds no row inside one: those rows are
  indexed under [the in-progress fence](#the-in-progress-fence). For a version
  whose workarea lies inside an in-progress subtree no `held_fts` statement
  runs and none is checked. Capture, reopen and a reassignment that re-enters
  held insert no `held_fts` row there; promotion, rejection, demotion, erasure
  and reassignment delete or update no `held_fts` row there. The exactly-one
  check moves to the `memories_fts` statement of the in-progress fence
  ([ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d9). Which table a
  held version takes is decided under the chain lock, as the fence declares.
- **PoC form (decided, not built).** The PoC, which stores bodies inline, is
  to match held text through one port method whose SQL hard-codes the state
  `held` and the author key set; a port-contract test run against both stores
  is to assert that it never returns another key's row, whatever the query,
  and the code and README are to state the gap to the designed form.

## Token budgets

This section is the single declaration site for recall budgets
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d6, d7).

**The budget token.** The recall budget unit is the **budget token** under
counter `oml-tc/1`: tokens(row) = ⌈`body_bytes` ÷ 4⌉, where `body_bytes` is the
UTF-8 octet length of the row's NFC-normalized body — the clear integer the
submission event journals: `submission.remember`, `submission.learn`, or
`simulation.write` for a simulation row
([event catalog](../data/event-catalog.md#submission-family)).
Only the body text is counted, never ids, paths or metadata. The divisor 4 is a
convention, not a measured ratio, and the count is not any model's tokenizer
count; how far the two differ for real text is unmeasured —
[FR-30](../product/roadmap.md). Clients convert with their own ratio from the
per-row counts. Changing the function mints a new counter id; `oml-tc/1` never
changes meaning.

**Per-row counts.** Every served row carries `tokens`, whether or not a budget
was sent: steering items, hits, get results and own-work listing items.

**The parameter.** `max_tokens` is an integer from 1 to 2⁵³ − 1. The upper
bound is the [read log](#the-read-log)'s: the request digest canonicalizes
`max_tokens` under [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785), which
serializes an integer exactly only below 2⁵³
([chain format](../data/chain-format.md#the-signed-fields)), so a larger value
is refused with `request.validation` rather than digested lossily. Absent,
there is no budget and the response carries no `budget` object. `max_tokens`
together with `include_bodies: false` is refused with `request.validation`:
counting bodies that are not sent gives the caller a meaningless budget.

**The cut.** Over the steering counts S, the `k`-capped ranked hit counts H
(own held rows included when asked) and the budget M:

1. s = ΣS. Steering is never cut.
2. remaining = max(M − s, 0).
3. The hits served are the longest prefix H[0..j) whose sum is at most
   remaining. The first hit that does not fit ends the list: no skip-ahead to a
   smaller later hit, and no partial body.
4. truncated = (j < |H|).

**The budget object.** `{max_tokens, counter: "oml-tc/1", steering_tokens: s,
used_tokens: s + ΣH[0..j), truncated, hits_omitted: |H| − j, next_hit_tokens}`,
where `next_hit_tokens` is tokens(H[j]), or null when nothing was cut.
`used_tokens` exceeds `max_tokens` only when steering alone exceeds it, and the
object shows that; steering past the budget is served whole, never refused,
and only hits of 0 budget tokens — empty bodies — can follow it.

**Worked example.** Steering [120], hits [300, 250, 90], M = 500: s = 120,
remaining = 380; the first hit (300) fits, the second (250) does not, so the
list ends there and the 90-token hit is not served. The answer serves one hit,
with `used_tokens` 420, `truncated` true, `hits_omitted` 2, `next_hit_tokens`
250.

**What the cut costs.** `k` caps the candidates before the budget applies;
counts come from `body_bytes` without decryption; rows the budget cuts are
never hydrated, decrypted or sent, and the served rows hydrate in one set-wise
statement ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d4). A strict
prefix can serve zero hits when the top hit exceeds what remains of the
budget; the budget object says so.

## Decrypt on serve

Bodies are stored as per-artifact-DEK ciphertext everywhere at rest —
Postgres, lake, object store ([storage.md](storage.md)). **Plaintext exists
only on the serving path**: `oml-server` unwraps the artifact's DEK and
decrypts per request, after access and the simulation predicate have passed.
No other workload decrypts — the lake-writer is keyless, moving
admission-produced ciphertext verbatim, and the reconciler verifies
ciphertext digests keylessly
([reconciliation.md](reconciliation.md)) — so crypto-shredding (D4, declared
in [audit-and-lineage.md](audit-and-lineage.md)) has exactly one decryption
locus to reason about. The cost is per-read decryption work on the server;
caching decrypted bodies anywhere outside that path is refused by design, and
any serving-path cache is itself a copy class in the erasure inventory.

One serving-path cache is named rather than left generic: the **DEK serving
cache** ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d3). Without
it, a recall with k hits pays k per-artifact key unwraps against the KMS —
a network call each — on every request; with it, the serving path holds
**plaintext keys of erase-eligible artifacts**, which is why it is a declared
design object and not an optimization detail. Its rules: it is a named copy
class in the erasure inventory
([audit-and-lineage.md](audit-and-lineage.md)); erase purges it
**synchronously**; it is TTL-bounded, and that TTL composes into
[FR-17](../product/roadmap.md)'s end-to-end erasure bound. Whether it caches
per-artifact DEKs or an unwrapped per-tenant KEK — different KMS-call
profiles, blast radii and erasure reasoning — is the open decision recorded
as [RS-14](../product/roadmap.md).

## Reads in cluster mode

In cluster mode ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md))
a follower's hot path is recall and get, only — writes forward to the lead —
and its reads follow one routing policy
([ADR-0024](../adr/ADR-0024-performance-foundation.md) d9):

- **Own-held reads route to the lead and the primary.** A read of the caller's
  own held rows — a recall with `include_own_held: true` among them — and any
  submit-then-get sequence, is served from the primary:
  read-your-writes on an agent's own work is the one place staleness visibly
  breaks an agent's loop, and held rows are author-only anyway, so one routing
  rule buys the guarantee with no extra machinery.
- **A recall that serves held claims of other keys inside an in-progress
  subtree** routes as promoted-row recall does, below: those rows are not the
  caller's own writes, so replication lag bounds them the same way. A caller
  that needs its own writes at once also sends `include_own_held: true`, which
  makes the request an own-held read.
- **Promoted-row recall and get serve from the follower's local replica**, with
  staleness stated honestly: the bound is **replication lag, exposed as a
  follower metric** — never hidden, never assumed zero. One consequence said
  plainly: a supersession ratified on the lead may briefly serve the old tip
  from a lagging follower; the stated staleness bound covers exactly that
  window.
- **The escape hatch is named, not built:** an LSN fence — the submit response
  carries the commit LSN, and a follower read can wait for replay to pass it —
  exists in the design vocabulary if read-your-writes recall is ever demanded,
  and is not part of v1.

## The read log

Every read is logged: key, role, workarea, the memory ids served, timestamp,
and for a recall the request digest. A recall logs exactly the artifact ids
served after the `k` and [budget](#token-budgets) cuts — own held ids included,
since an author reading its own rows is still a read — never the candidate list
before the cuts. `query_digest` is SHA-256 over the RFC 8785 JSON of an object
whose members, listed below, are always present, with defaults applied before
digesting:

| Member | Value digested |
|---|---|
| `query` | the NFC-normalized string |
| `k` | the integer, default 8 applied |
| `kinds` | sorted ascending by Unicode code point (equivalently UTF-8 byte order), duplicates removed; `[]` when absent — a sent empty array is refused, so `[]` stands only for absence |
| `include_bodies` | the boolean, default `true` applied |
| `include_own_held` | the boolean, default `false` applied |
| `include_unreviewed` | the boolean, the scope's default applied: `true` when the key has [unreviewed access](#held-claims-inside-an-in-progress-workarea) on the scope node, else `false` |
| `max_tokens` | the integer, `null` when absent |
| `terms` | sorted ascending by Unicode code point (equivalently UTF-8 byte order), duplicates removed; `[]` when absent — a sent empty array is refused, so `[]` stands only for absence |
| `expand_terms` | the string, default `"none"` applied |
| `scope` | the resolved node id |

So two requests share a digest only when they differ solely in the order or
repetition of `kinds` or `terms` entries, and one request gets one digest on
every server; the served ids, logged beside it, record what each read
returned ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d9).
Design commitments:

- **Outside the hash chain.** Read events go to `read_events`, a partitioned,
  retention-bounded log — not to the journal. Reads are orders of magnitude
  more frequent than writes, and a chain serialized per tenant (D1,
  [storage.md](storage.md)) must not be throttled by read volume.
- **Anchored by batch digests.** Per-batch digests of the read log are written
  into the journal as ordinary signed events. Tampering with a logged read
  therefore breaks an anchored digest: the log sits outside the chain but is
  not outside its protection. What anchoring does **not** give is
  per-read-event chain membership; that boundary is stated here, once, rather
  than implied away.
- **Follower reads log asynchronously.** `read_events` lives on the primary, so
  a follower serving recall writes its read events upstream as an
  **asynchronous batched insert**, never a synchronous write inside its read
  path ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d9). The honest
  cost: follower buffering enlarges the unanchored tail — a crashed follower
  loses its unflushed read-event buffer — which is stated where
  [commitments](../product/commitments.md) states the read-log bound; who
  batches and anchors the follower-written rows is the open decision
  [RS-13](../product/roadmap.md)
  ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)).
- **Retention is tenant policy.** The retention window of `read_events` is a
  journalled per-tenant policy, not a constant. Evidence exports scope their
  read-log section to that window, and `read_events` appears in the
  retention/erasure inventory ([audit-and-lineage.md](audit-and-lineage.md)).

The formulation this design commits to, everywhere the read log is described:
**a read log that answers who read what, when an auditor asks** — within the
tenant's retention window, with batch digests anchored in the journal. No OML
page, message or export names a regulator's requirement unless it cites the
regulation and article; there is no such citation today, so there is no such
claim (D17).

## The simulation read predicate

Every read path — FTS, vector, fetch, graph — applies one predicate:

> `provenance ≠ simulation` **OR** `key ∈ simulation audience`

Simulation rows ([simulations.md](simulations.md)) are never in the shared
FTS and vector indexes; they are indexed per simulation, so the shared indexes
are structurally incapable of leaking a simulation row to a key outside the
audience — the predicate on the shared path is a second fence, not the only
one. The cost is an index per live simulation and a split query plan for
audience members; the design accepts it because containment by construction
beats containment by filter discipline.

Tip resolution follows the same fence: **taint never captures the default
tip.** Tip computation for a key outside a simulation's audience ignores
simulation-tainted versions — `provenance: simulation` rows and sim-derived
claims alike — so the default tip of every claim is bit-for-bit what it would
be if the simulation did not exist. A promoted tainted supersession of a real
claim is served to audience keys as an **audience-scoped overlay tip** that
shadows the default tip inside the fence; the shared tip index
([audit-and-lineage.md](audit-and-lineage.md) owns tip semantics) never
records it, and any act that would make a tainted version the default served
tip is refused (`simulation.tip_refused`, [errors](../api/errors.md)). When a
simulation ends, its overlay tips demote with its closure; the default tip
never moved.

## Out of scope: grounded Q&A

OML answers no questions. Clients are agents that recall and reason
themselves; the provider serves memories, never synthesized answers. The
decision is ADR-0013 in the [decision register](../adr/index.md), which
declines the feature for v1 rather than deferring it silently. If a
grounded-answer surface ever returns, it enters as a new decision with its own
review, not as a quiet extension of recall.

## Decision record

This page's decisions are registered as ADR-0013
([decision register](../adr/index.md)); own held rows in recall, held text
containment, token budgets, the segment membership and order, and the read-log
content are the proposed [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md)'s,
which would amend ADR-0013 d1, d2 and d5. Held claims inside an in-progress
workarea, `include_unreviewed` and the in-progress fence are the proposed
[ADR-0030](../adr/ADR-0030-in-progress-workareas.md)'s, which would amend
ADR-0013 d1 and d4 and ADR-0025 d4, d5, d8 and d9.
