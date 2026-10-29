# Delta tables

**Status:** design draft for review · decided, not built

This page is the column-level specification of the lake: every Delta table, its
Delta and Arrow types, its partitioning, its per-table commit protocol, and the
maintenance that keeps it compact and erasable. The decisions these tables
implement — Postgres as the sole authority (D1), the lake as derived body
lakehouse with a single lake-writer and the outbox protocol (D2), pure-Rust
delta-rs as the writer (D5), per-tenant isolation (D6) — are declared in
[architecture/storage.md](../architecture/storage.md) and
[architecture/security.md](../architecture/security.md) and only instantiated
here. The Postgres side of every row is in
[postgres-schema.md](postgres-schema.md); the journal events the lake mirrors
are in [event-catalog.md](event-catalog.md).

One rule governs everything on this page: **no zone serves client reads.**
Recall, search, fetch and graph reads are answered from Postgres
([architecture/retrieval.md](../architecture/retrieval.md)); the lake's
consumers are the reconciler, lineage and metadata analytics, and future
journalled per-audience exports. A consequence worth stating up front: Delta
time travel makes superseded row versions readable to anything holding lake
credentials until [VACUUM](#maintenance-optimize-vacuum-checkpoints) removes
them, which is one reason lake credentials are service-only and no client path
touches these tables.

> **delta-rs capability citations (D17).** Every delta-rs capability this page
> relies on — application transaction ids with check-and-record semantics,
> `MERGE`, `OPTIMIZE` (bin-pack and Z-order), `VACUUM`, explicit checkpoint
> creation, conditional put through `object_store` — is a claim about a
> third-party library and **must be cited from delta-rs documentation with a
> fetch date at writing time; as of this draft each is unverified.** Markers
> below read *(delta-rs — D17 note)*. One capability is cited negatively and
> is load-bearing: **deletion vectors are read-only in delta-rs**, so no table
> here assumes DV-based deletes — every retract and erase is a copy-on-write
> file rewrite. The exit criterion for the exactly-once claims is the delta-rs
> conformance-test FR in [product/roadmap.md](../product/roadmap.md).

## What each zone is for

| Zone / table | What it is FOR | What never enters it | Mutated by |
|---|---|---|---|
| `bronze.events` | Digest-only columnar mirror of the per-tenant journal: the substrate for event analytics and for cross-checking the lake against the chain without loading Postgres | Body plaintext, body ciphertext, held content of any kind — payloads are digests and ciphertext *references* only | Append only; never MERGEd |
| `silver.claims` | One row per promoted claim version with ciphertext body and full attribution: the reconciliation target ([architecture/reconciliation.md](../architecture/reconciliation.md)) and the materialized record of who wrote what, when, in which workarea | Held bodies (materialization is enqueued by promotion only); `self_signed` development rows (the constitution's development exception, [ADR-0001](../adr/ADR-0001-the-constitution.md)); plaintext anywhere | Append on promotion; MERGE on retract |
| `gold.lineage` | Ratified edges of the claim graph and the workarea relation graph, for lineage analytics and the evidence export | Pending edges (the promotion-time lineage closure over pending + ratified edges is computed in Postgres, never here) | Append on ratification; MERGE on retract |
| `gold.areas` | Current snapshot of the workarea tree — node, parent, path, status, crew metadata — so lineage and analytics can join claims to governance context | Grants, denies, keys (authorization state stays in Postgres only) | MERGE-maintained snapshot |
| `gold.curated_<table>` | One table per **validated** workarea: the frozen area's promoted claim versions, ciphertext-bodied, as a stable curated set; the substrate for future per-audience decrypted exports (deferred — FR in [product/roadmap.md](../product/roadmap.md)) | New writes, promotions, reassignments in — refused by the status ladder itself ([architecture/workareas.md](../architecture/workareas.md)); simulation-tainted rows (refused promotion into validated, [architecture/simulations.md](../architecture/simulations.md)) | Created at validation; thereafter MERGE on demote/erase only |

The medallion words name these physical zones and nothing else; the workarea
status ladder keeps its own words
([architecture/workareas.md](../architecture/workareas.md)).

## Type conventions

delta-rs writes through Arrow, so every column is specified in both systems.
The mapping used throughout:

| Delta type | Arrow type | Used for |
|---|---|---|
| `STRING` | `utf8` | ids (UUIDs as lowercase text), enums, paths, hex digests, JSON payload text |
| `LONG` | `int64` | sequence numbers, versions, sizes |
| `BINARY` | `binary` | chain hashes, signatures, signed envelope bytes, body ciphertext |
| `TIMESTAMP` | `timestamp[µs, UTC]` | all instants — microsecond precision, UTC; nanosecond Arrow inputs are cast down at the writer *(delta-rs — D17 note)* |
| `DATE` | `date32` | partition columns derived from an instant |
| `ARRAY<T>` | `list<T>` | taint id lists, attachment lists |
| `STRUCT` | `struct` | attachment descriptors |

Two deliberate choices with their costs:

- **Digests are lowercase hex `STRING`, not `BINARY`.** They read in any
  tool, and comparison against Postgres' `bytea` digest columns is a one-line
  `encode(digest, 'hex')` on the Postgres side; the cost is 64 bytes where 32
  would do, accepted for a column that is the lake's primary cross-system
  join key.
- **Statistics are disabled on `body_ciphertext` columns** (the writer
  restricts data-skipping stats to non-body columns — *delta-rs — D17 note*),
  so min/max fragments of ciphertext do not leak into the Delta log and its
  checkpoints. The erasure inventory still treats log and checkpoint files as
  ciphertext-bearing copy classes (defense in depth; see
  [maintenance](#maintenance-optimize-vacuum-checkpoints)) rather than relying
  on this exclusion being perfect.

## Per-tenant prefix layout

Each tenant gets its **own physical Delta tables** under its own prefix — the
lake is not one shared table partitioned by tenant:

```text
oml-lake/<tenant_id>/bronze/events/
oml-lake/<tenant_id>/silver/claims/
oml-lake/<tenant_id>/gold/lineage/
oml-lake/<tenant_id>/gold/areas/
oml-lake/<tenant_id>/gold/curated/<workarea_id>/
```

The benefit is mechanical tenant isolation: a tenant's lake is a prefix, its
erasure bound is a per-table property, per-tenant storage credentials can be
attached when storage is externalized (D6), and no query can cross tenants by
forgetting a filter. The cost is table-count proportional to tenants (and to
validated workareas, for curated tables) and maintenance that must iterate
tenants — accepted because the lake-writer already iterates per tenant under
an RLS-scoped session role
([architecture/security.md](../architecture/security.md)). Every table still
carries `tenant_id` in-row, redundantly with the prefix, so the key-less
reconciler can detect a row written under the wrong prefix as a finding rather
than trusting path layout.

### Conditional put on S3/MinIO

Delta commits are files named by version; two writers putting the same commit
file is the lost-update failure that breaks the log. On S3 and MinIO, **every
commit therefore requires conditional put (put-if-absent)** through the
`object_store` layer *(delta-rs — D17 note)*. The deployment pins a MinIO
version that supports `If-None-Match`
([operations/deployment.md](../operations/deployment.md)); an object store
without conditional put is not a supported lake target, full stop — the
design refuses the "mostly single writer, probably fine" posture because the
lake-writer leader lease can and will overlap during failover, and conditional
put plus the per-table transaction-version conflict below is exactly the fence
that makes that overlap safe.

## The commit protocol, instantiated per table

[architecture/storage.md](../architecture/storage.md) declares the outbox
protocol; this section fixes how it lands on each physical table. The
application transaction id *(delta-rs — D17 note)* is recorded on every commit:

| Table | txn `appId` | txn `version` | Skip rule on replay |
|---|---|---|---|
| `bronze.events` | `oml-lakewriter` | `batch_end` (global outbox `seq`) | skip if `batch_end ≤` last committed version for the appId |
| `silver.claims` | `oml-lakewriter` | `batch_end` | same |
| `gold.lineage` | `oml-lakewriter` | `batch_end` | same |
| `gold.areas` | `oml-lakewriter` | `batch_end` | same |
| `gold.curated_<id>` | `oml-lakewriter` | `batch_end` | same; a table that does not yet exist is treated as version 0 |

Notes that only make sense at this level:

- The check is **per physical table**, which with per-tenant prefixes means
  per *(tenant, table)*. The global outbox `seq` is monotonic across tenants,
  so one version counter per table stays monotonic no matter which tenants a
  batch touches.
- The commit's transaction action also records the batch bounds
  `(frontier, batch_end)`; recovery reconstructs the identical batch from the
  outbox by seq range, never by time or size. The Postgres frontier column is
  a cache; **the Delta log's transaction version is the sole recovery
  authority.**
- A `gold.curated_<id>` table is **created by the batch that carries the
  area's validation event**: table creation and the initial population commit
  carry that batch's `batch_end` as their transaction version, so a crash
  between creation and population replays deterministically like any other
  half-applied batch.
- Reconciler repairs re-enter through the outbox as ordinary operations
  carrying a journal **fold seq**; the lake-writer refuses an operation older
  than the artifact's last applied `fold_seq`
  ([architecture/reconciliation.md](../architecture/reconciliation.md)), which
  is why `silver.claims` carries that column.

## bronze.events

Append-only, digest-only mirror of the journal, one row per journal event.
Its job is to make the event stream queryable in columnar form — event
analytics, lag and volume studies, and keyless cross-checking of the chain —
without ever holding content. Because payloads are digest-only by D1, bronze
**never participates in erasure**: the D4 inventory lists it as "never needs
touching", and no retract ever MERGEs it. A retract is visible in bronze the
same way everything else is: as another event row.

| Column | Delta | Arrow | Null | Meaning |
|---|---|---|---|---|
| `tenant_id` | STRING | utf8 | no | Owning tenant; redundant with the prefix, kept for cross-checks |
| `journal_seq` | LONG | int64 | no | Per-tenant chain sequence |
| `event_id` | STRING | utf8 | no | Journal event UUID |
| `event_type` | STRING | utf8 | no | From the [event catalog](event-catalog.md) |
| `at` | TIMESTAMP | timestamp[µs, UTC] | no | Chain timestamp (real time; simulated time is a payload field, never this column) |
| `event_date` | DATE | date32 | no | `at` truncated to UTC date — **partition column** |
| `prev_hash` | BINARY | binary | no | Chain predecessor hash |
| `hash` | BINARY | binary | no | This event's chain hash |
| `sig` | BINARY | binary | no | ed25519 signature over `signed_bytes` |
| `signed_bytes` | BINARY | binary | no | The signed digest/ciphertext envelope — lets the key-less reconciler re-verify chain segments from bronze alone |
| `payload` | STRING | utf8 | no | Canonical JSON, digest/ciphertext references only — the same bytes the journal holds, never more |
| `principal` | STRING | utf8 | no | Acting principal |
| `api_key_id` | STRING | utf8 | no | Key that performed the act |
| `artifact_id` | STRING | utf8 | yes | Extracted for analytics when the event concerns a claim |
| `version` | LONG | int64 | yes | Claim version, when applicable |
| `workarea_id` | STRING | utf8 | yes | Workarea node id, when applicable |
| `simulation_id` | STRING | utf8 | yes | Indelible simulation provenance (D10) |
| `provenance` | STRING | utf8 | yes | `agent` \| `human` \| `connector` \| `simulation` |
| `outbox_seq` | LONG | int64 | no | Outbox row that carried this event — joins bronze to the frontier |

**Partitioning:** by `event_date`. Reconciliation and analytics scans are
time-bounded, so daily pruning pays for itself; the cost is one small file
per day per tenant at low volume, repaired by bin-pack OPTIMIZE rather than
avoided.

## silver.claims

One row per **promoted claim version**. The row exists because a promotion
event was drained from the outbox — held bodies never reach the lake, and
`self_signed` development rows never enter silver at all (the constitution's development exception, [ADR-0001](../adr/ADR-0001-the-constitution.md)).
This table is the commissioning requirement made physical: pure memory data,
who and when wrote it, what space — with bodies encrypted at rest.

| Column | Delta | Arrow | Null | Meaning |
|---|---|---|---|---|
| `tenant_id` | STRING | utf8 | no | Owning tenant |
| `artifact_id` | STRING | utf8 | no | Claim artifact UUID — MERGE key, with `version` |
| `version` | LONG | int64 | no | Version within the `claim_key` lineage — MERGE key |
| `claim_key` | STRING | utf8 | no | Groups versions of one claim |
| `kind` | STRING | utf8 | no | `memory` \| `derived_fact` \| `steering` \| … |
| `state` | STRING | utf8 | no | `promoted` \| `demoted` \| `erased` \| `held` — `held` is reachable **only** by the reassign-to-stricter-policy retract below, never by admission |
| `workarea_id` | STRING | utf8 | no | Stable workarea node id; updated by reassign |
| `workarea_path_at_write` | STRING | utf8 | no | ltree path as derived at write time — history, not identity; **immutable**, moves are read from bronze and `gold.areas` |
| `writer_identity` | STRING | utf8 | no | Writing identity |
| `principal` | STRING | utf8 | no | Acting principal |
| `producer` | STRING | utf8 | yes | Producing agent/crew identifier, when declared |
| `provenance` | STRING | utf8 | no | `agent` \| `human` \| `connector` \| `simulation` — indelible |
| `simulation_id` | STRING | utf8 | yes | Set on every simulation row (D10) |
| `sim_derived_ids` | ARRAY&lt;STRING&gt; | list&lt;utf8&gt; | yes | Simulation ids in the row's lineage closure — the taint record |
| `plaintext_digest` | STRING | utf8 | no | SHA-256 of plaintext, lowercase hex — survives erasure; proves what the erased row was about |
| `ciphertext_digest` | STRING | utf8 | no | SHA-256 of ciphertext — what the key-less reconciler verifies |
| `body_ciphertext` | BINARY | binary | yes | Per-artifact-DEK ciphertext of the claim body; NULL when `body_ref = object`, and **nulled by the erase MERGE**; no column statistics collected |
| `body_ref` | STRING | utf8 | no | `lake` \| `object` — where the ciphertext body lives; attachments are always `object` ([architecture/storage.md](../architecture/storage.md)) |
| `attachments` | ARRAY&lt;STRUCT&gt; | list&lt;struct&gt; | yes | Per attachment: `plaintext_digest` STRING, `ciphertext_digest` STRING, `media_type` STRING, `size_bytes` LONG |
| `created_at` | TIMESTAMP | timestamp[µs, UTC] | no | Submission time |
| `promoted_at` | TIMESTAMP | timestamp[µs, UTC] | no | Ratification time |
| `retracted_at` | TIMESTAMP | timestamp[µs, UTC] | yes | Set by the retract MERGE |
| `applied_seq` | LONG | int64 | no | Outbox `seq` of the last operation applied to this row — the per-row frontier column |
| `fold_seq` | LONG | int64 | yes | Journal fold seq of the last reconciler repair; the lake-writer refuses operations older than it |

**Partitioning:** none. The tempting partition column, `workarea_id`, is
updated by the reassign retract — a MERGE that changes a partition column is a
cross-partition row move, which turns the cheapest retract into the most
expensive one. Instead the table is kept unpartitioned (it is already
per-tenant) and clustered by periodic `OPTIMIZE` with Z-order on
`(workarea_id, artifact_id)` *(delta-rs — D17 note)*, which serves both the
reconciler's per-artifact lookups and per-workarea analytics scans. Row
volumes are unmeasured (D17); the revisit trigger is a measured table size at
which a single OPTIMIZE run can no longer rewrite the working set within its
maintenance window.

## gold.lineage

Ratified edges only, from both graphs: claim↔claim
(`derives_from`, supersession) and workarea↔workarea labelled relations
(`relates_to`, `continues`, `derived_from`, `mirrors` — zero access conferred,
[architecture/workareas.md](../architecture/workareas.md)). Pending edges stay
in Postgres: the gate changes visibility, and an unratified edge is not
visible anywhere, the lake included. The promotion-time lineage closure (which
spans pending **and** ratified edges) is computed in Postgres at promotion
time; this table is for after-the-fact lineage analytics and the evidence
export, not for enforcement.

| Column | Delta | Arrow | Null | Meaning |
|---|---|---|---|---|
| `tenant_id` | STRING | utf8 | no | Owning tenant |
| `edge_id` | STRING | utf8 | no | Edge UUID — MERGE key |
| `graph` | STRING | utf8 | no | `claim` \| `workarea` |
| `edge_label` | STRING | utf8 | no | `derives_from`, `supersedes`, `relates_to`, `continues`, `derived_from`, `mirrors`, … |
| `src_id` | STRING | utf8 | no | Source artifact or workarea node id |
| `src_version` | LONG | int64 | yes | Claim version; NULL for workarea endpoints |
| `dst_id` | STRING | utf8 | no | Destination id |
| `dst_version` | LONG | int64 | yes | As above |
| `state` | STRING | utf8 | no | `ratified` \| `demoted` |
| `proposer` | STRING | utf8 | no | Proposing key id |
| `ratifier` | STRING | utf8 | no | Ratifying key id — never the proposer (R2 extends to edges) |
| `simulation_id` | STRING | utf8 | yes | Set when either endpoint is simulation-tainted |
| `proposed_at` | TIMESTAMP | timestamp[µs, UTC] | no | |
| `ratified_at` | TIMESTAMP | timestamp[µs, UTC] | no | |
| `applied_seq` | LONG | int64 | no | Per-row frontier column |

**Partitioning:** none; edge volume is bounded by claim volume and the table
is per-tenant. MERGE key is `edge_id`.

## gold.areas

MERGE-maintained **current snapshot** of the workarea tree, one row per node.
History — status transitions, re-parenting, crew-metadata updates — is not
versioned here; it is read from `bronze.events`, where every one of those acts
is a journalled event. A snapshot plus an event mirror is cheaper and harder
to get wrong than a slowly-changing-dimension table, at the cost that
"what was the tree on date X" is a bronze replay, not a gold query.

| Column | Delta | Arrow | Null | Meaning |
|---|---|---|---|---|
| `tenant_id` | STRING | utf8 | no | Owning tenant |
| `workarea_id` | STRING | utf8 | no | Node id — MERGE key |
| `parent_id` | STRING | utf8 | yes | NULL only for the tenant's `universe` root |
| `path` | STRING | utf8 | no | Current derived ltree path; rewritten by the re-parent MERGE |
| `status` | STRING | utf8 | no | `development` \| `committed` \| `validated` |
| `crew_name` | STRING | utf8 | yes | Recorded, not operated (D7) |
| `crew_charter_uri` | STRING | utf8 | yes | |
| `crew_external_manager` | STRING | utf8 | yes | External manager identifier |
| `created_at` | TIMESTAMP | timestamp[µs, UTC] | no | |
| `updated_at` | TIMESTAMP | timestamp[µs, UTC] | no | Last applied change |
| `applied_seq` | LONG | int64 | no | Per-row frontier column |

**Partitioning:** none; workarea counts are governance-bounded.

## gold.curated_&lt;table&gt;

One Delta table per **validated** workarea, at
`gold/curated/<workarea_id>/`, created by the batch that carries the area's
validation event. Its contents at creation: the area's `silver.claims` rows in
state `promoted` (simulation-tainted rows cannot exist in a validated area —
promotion into validated refuses them, D10). Its schema is **identical to
`silver.claims`** plus one column:

| Column | Delta | Arrow | Null | Meaning |
|---|---|---|---|---|
| `curated_at` | TIMESTAMP | timestamp[µs, UTC] | no | When the validation batch materialized this row into the curated table |

Keeping the schema identical means every tool, every reconciler comparison and
every maintenance job written for silver runs unmodified against curated
tables; the cost is carrying columns that are constant within one table
(`workarea_id`) or vestigial after freeze. After creation the only operations
the status ladder admits for a validated area are demote and erase
([architecture/workareas.md](../architecture/workareas.md)), so the only
writes this table ever sees again are those two MERGEs — applied to **both**
the curated table and `silver.claims` by the same outbox operation, under the
same transaction version, so the two can never disagree for longer than one
in-flight batch (and the reconciler reports it if they do).

Bodies here are ciphertext, exactly as in silver. The decrypted per-audience
export of a curated table is a deferred, journalled feature — the FR row in
[product/roadmap.md](../product/roadmap.md) — and until it exists, nothing
reads a curated table but the reconciler and metadata analytics.

## Retracts: MERGE semantics per table

A retract is a Delta `MERGE` *(delta-rs — D17 note)* applied **in strict
outbox seq order, in the same single stream as materialization** — no separate
retract lane, so ordering is free and head-of-line blocking is the accepted
cost ([architecture/storage.md](../architecture/storage.md)). Because
deletion vectors are read-only in delta-rs, every MERGE is copy-on-write: the
parquet files containing matched rows are rewritten, and the superseded files
become unreferenced — which is what the erasure section below is about.

| Retract | Trigger event | `silver.claims` | `gold.curated_<id>` | `gold.lineage` | `gold.areas` | `bronze.events` |
|---|---|---|---|---|---|---|
| **Demote** | demotion journalled | MERGE on (`artifact_id`, `version`): `state = demoted`, `retracted_at` set | Same MERGE when the row's area is validated | — | — | New event row |
| **Erase** | crypto-shred journalled (D4) | MERGE: `state = erased`, `body_ciphertext = NULL`, `attachments = NULL`, `retracted_at` set; digests and attribution remain | Same | — | — | New event row (digest-only; nothing to erase) |
| **Simulation end** | simulation-end journalled | MERGE demoting every row with that `simulation_id`; taint closure (`sim_derived_ids` containing it) demoted or flagged for re-review as the event records | Cannot match (no sim rows in validated areas) | MERGE: `state = demoted` on edges carrying the `simulation_id` | — | New event rows |
| **Reassign** | `lifecycle.reassign` (both policy snapshots journalled) | MERGE: `workarea_id = destination`; `workarea_path_at_write` untouched (history); when the move records re-entry into held (stricter destination policy), `state = held` | Reassignment into a validated area is refused upstream; out of one is refused by freeze | — | — | New event row |
| **Re-parent / status / crew update** | journalled admin act | — | — | — | MERGE on `workarea_id`: path rewrite, status, crew fields | New event row |

Two honesty notes on this table:

- **`state = held` in silver exists only via reassign.** The invariant "no
  held body ever reaches the lake" is about admission: a held body never
  *enters*. A promoted body whose claim is moved into a stricter-policy
  workarea re-enters held in Postgres; its silver row already lawfully exists,
  so the retract marks it `held` rather than pretending it is still promoted
  or silently deleting it (R4). Re-promotion MERGEs it back to `promoted`.
- **An erase that dead-letters leaves ciphertext in the live table version**
  until the journalled re-enqueue repairs it. The reconciler reports
  `dead-lettered` as a named finding class and `oml doctor` surfaces the
  count; the erasure SLO clock does not stop for a dead-lettered erase — it
  is a breach in progress and is treated as an operator condition
  ([architecture/reconciliation.md](../architecture/reconciliation.md)).

## Maintenance: OPTIMIZE, VACUUM, checkpoints

All lake maintenance runs **inside the lake-writer process** (D5) — same
leader lease, same conditional-put fence, no second writer identity to reason
about. The cost is that maintenance competes with the drain loop for the
singleton's time; the schedule below is sized for that, and every cadence here
is a configuration default of an unbuilt system, not a measured optimum (D17).

| Job | Tables | Default cadence | What it does |
|---|---|---|---|
| `OPTIMIZE` bin-pack *(delta-rs — D17 note)* | all | daily, per tenant | Coalesces small files (micro-batches and daily bronze partitions produce them by design) |
| `OPTIMIZE` Z-order *(delta-rs — D17 note)* | `silver.claims` on `(workarea_id, artifact_id)`; `gold.lineage` on `(src_id)` | weekly | Clusters for reconciler lookups and lineage scans |
| Checkpoint creation *(delta-rs — D17 note)* | all | explicit, every 100 commits per table | Bounds log-replay cost on open; delta-rs checkpointing is invoked by the writer, not assumed automatic |
| `VACUUM` *(delta-rs — D17 note)* | all | daily, retention = the table's erasure bound (below) | Deletes unreferenced parquet files older than the retention window |
| Log cleanup | all | with checkpointing, retention = same bound | Expires old Delta log entries and superseded checkpoints |

### The erasure-SLO interaction

Erasure is crypto-shredding: the journalled erase event destroys the
artifact's DEK, and from that instant the bytes are unreadable everywhere
([architecture/audit-and-lineage.md](../architecture/audit-and-lineage.md)
owns D4 and the full copy-class inventory). What this section bounds is the
narrower, honest remainder: **how long ciphertext copies of an erased body
persist in the lake before cleanup removes them.** The copy classes here:

1. **The live table version** stops holding the ciphertext when the erase
   MERGE applies (it rewrites the row's files with `body_ciphertext = NULL`).
   Bound: outbox drain lag, normally seconds to minutes; unbounded only in the
   dead-letter case called out above.
2. **Superseded parquet files** — every pre-erase version of the row's files,
   including files rewritten by earlier OPTIMIZE runs — persist, time-travel
   readable, until `VACUUM` with an expired retention window deletes them.
3. **Delta log entries and checkpoints** are treated as ciphertext-bearing
   until log cleanup expires them, even though body-column statistics are
   disabled — the inventory does not rest on a stats configuration being
   airtight.

One tenant-policy parameter, the **lake erasure bound** (default: 7 days),
sets both the VACUUM deleted-file retention and the log retention on every
body-bearing table (`silver.claims`, every `gold.curated_<id>`). The
documented erasure SLO for lake ciphertext is therefore:

> drain lag + lake erasure bound + one maintenance cadence (the daily
> VACUUM/cleanup run that first fires after the bound expires).

With defaults: roughly eight days from erase event to no ciphertext copy in
the lake, stated as such in the evidence export rather than rounded down.
Shortening the bound tightens the SLO and shrinks the time-travel window the
reconciler can use for named-version comparisons
([architecture/reconciliation.md](../architecture/reconciliation.md)); that
trade is the tenant's to make, journalled as policy. `bronze.events` needs no
bound: it is digest-only by construction and the D4 inventory marks it "never
needs touching". VACUUM and log cleanup on bronze exist purely for storage
hygiene.

## Decision record

This page instantiates D2, D5 and D6 at column level; the owning declarations
are [architecture/storage.md](../architecture/storage.md) and
[architecture/security.md](../architecture/security.md), and the register is
[adr/index.md](../adr/index.md) (ADR-0003, ADR-0006, ADR-0007).

Lake write latency, maintenance cost and table-size ceilings are unmeasured,
and no number is asserted for these tables until one is measured (D17).
