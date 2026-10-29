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
([workareas.md](workareas.md)). Held rows are visible solely to their author
through own-work status; demoted and erased rows are not served. This is R3
made operational: the gate changed visibility, and the read path is where that
visibility is enforced.

`self_signed` rows — the constitution's bounded development exception
([workareas.md](workareas.md)) — are served only to reads scoped within their
own workarea; no query scoped outside it returns them.

## The serving tier

Two indexes over promoted rows, both in Postgres, both under tenant RLS:

- **Full-text search**: `memories_fts`, a `tsvector` index over promoted claim
  text.
- **Vector search**: `memory_embeddings`, embeddings over promoted claim text.

### pgvector layout

`memory_embeddings` is **list-partitioned by `tenant_id`**, with a
**per-partition HNSW index** and RLS on top of the partitioning. The
partitioning is a performance and blast-radius choice layered under the
security boundary, not a substitute for it: RLS is the isolation authority
(declared in [security.md](security.md)); the per-tenant partition means one
tenant's index build, vacuum or bloat never degrades another's graph, and an
HNSW graph never spans tenants.

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
verdict authority; it reads promoted text on the serving side of decryption
and writes embedding rows. Everything it produces is plaintext-derived and
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

The contract is part of the API surface ([api/http-api.md](../api/http-api.md)
and the MCP `recall` tool): clients may rely on segment order, and nothing a
client sends re-ranks segment 1.

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

## The read log

Every read is logged: key, role, workarea, the memory ids served (or the query
digest for searches), timestamp. Design commitments:

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
([decision register](../adr/index.md)).
