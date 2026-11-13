---
status: Stable
verified: 2026-11-06
answers: "every lake table column by column: Delta types, partitioning, merge keys, what each table holds and never holds, the commit protocol, and the maintenance that keeps the lake compact and erasable"
---

# Delta tables

This page lists the tables that `oml-lakewriter` writes: their columns, partitioning and merge keys, and
the maintenance that keeps them compact. To run the writer, see [Run the lake writer](../operations/lake.md).
The journal events the lake mirrors are on the [event catalog](event-catalog.md).

No client reads the lake. PostgreSQL answers recall, search and get. A reader with lake credentials can
read earlier versions of a row through Delta time travel until `VACUUM` removes them. So lake credentials
belong to services and operators only.

## Where the tables are

Each tenant has its own physical tables under its own prefix. The lake is not one shared table partitioned
by tenant.

```text
<lake>/<tenant id>/bronze/events/
<lake>/<tenant id>/silver/claims/
<lake>/<tenant id>/gold/lineage/
<lake>/<tenant id>/telemetry/recall_daily/
<lake>/_provider/lake/batches/
<lake>/_provider/bench/results/
```

A tenant's lake is a prefix. No query crosses tenants by forgetting a filter, and per-tenant storage
credentials can be attached. The cost is one set of tables per tenant. Every tenant table also carries
`tenant_id` in each row.

| Table | One row per | Never holds | Written by |
|---|---|---|---|
| `bronze.events` | Journal event | Body text, body ciphertext, held content | Append only |
| `silver.claims` | Memory version that was approved | Held bodies on first write, development-home (`self_signed`) rows, plaintext | Append on approval, merge on demote or erase |
| `gold.lineage` | Ratified edge, and each version's derives-from or supersedes link | Pending edges | Append on ratification, merge on change |
| `telemetry.recall_daily` | Day, pipeline and access path | Query text, version ids | Rewritten from the last 30 days |
| `lake.batches` | Drained batch | Tenant data | Append only |
| `bench.results` | Metric of a loaded benchmark file | Tenant data | Merge by run |

A "claim" in a table name is the stored word for a memory.

## Column types

| Delta type | Used for |
|---|---|
| `STRING` | Ids (UUIDs as lowercase text), enums, paths, hex digests, JSON payload text |
| `LONG` | Sequence numbers, versions, sizes |
| `BINARY` | Chain hashes, signatures, signed bytes, body ciphertext |
| `TIMESTAMP` | Every instant, in microseconds, UTC |
| `DATE` | Partition columns derived from an instant |
| `BOOLEAN`, `DOUBLE` | Flags, benchmark values |

Digests are lowercase hex `STRING`, so they read in any tool. A hex digest takes 64 bytes where 32 would
do. That is the cost of making the digest the lake's cross-system join key.

## `bronze.events`

A digest-only mirror of the journal. It allows querying the event stream in columnar form and cross-check
the lake against the chain without loading PostgreSQL. It never holds content, so erase never touches it.
Partitioned by `event_date`.

| Column | Type | Null | Meaning |
|---|---|---|---|
| `tenant_id` | STRING | no | Owning tenant |
| `journal_seq` | LONG | no | Per-tenant chain sequence |
| `event_id` | STRING | no | Journal event UUID |
| `event_type` | STRING | no | From the [event catalog](event-catalog.md) |
| `at` | TIMESTAMP | no | Chain timestamp |
| `event_date` | DATE | no | `at` as a UTC date. The partition column. |
| `prev_hash` | BINARY | no | Chain predecessor hash |
| `hash` | BINARY | no | This event's chain hash |
| `sig` | BINARY | no | ed25519 signature over `signed_bytes` |
| `signed_bytes` | BINARY | no | The signed envelope, so chain segments can be re-verified from bronze alone |
| `payload` | STRING | no | Canonical JSON with digests and references only |
| `principal` | STRING | no | Acting principal |
| `api_key_id` | STRING | yes | Key that performed the act |
| `artifact_id` | STRING | yes | The memory version, when the event concerns one |
| `version` | LONG | yes | Version, when applicable |
| `workarea_id` | STRING | yes | Workarea, when applicable |
| `kind` | STRING | yes | Memory kind, when applicable |
| `subject_name` | STRING | yes | Name of the subject the event names, when applicable |
| `same_user` | BOOLEAN | yes | The event's `same_user` flag, when it carries one |
| `outbox_seq` | LONG | no | The outbox row that carried this event |

## `silver.claims`

One row per approved memory version. The row exists because a promotion event was drained from the outbox.
Held bodies do not enter, and development-home (`self_signed`) rows never enter. Bodies are encrypted at rest
under a key of their own. The merge key is `artifact_id`, which names the version. Unpartitioned.

| Column | Type | Null | Meaning |
|---|---|---|---|
| `tenant_id` | STRING | no | Owning tenant |
| `artifact_id` | STRING | no | The version's id. The merge key. |
| `version` | LONG | no | Version within the `claim_key` lineage |
| `claim_key` | STRING | no | Groups the versions of one memory |
| `kind` | STRING | no | The memory's kind |
| `state` | STRING | no | `promoted`, `superseded`, `demoted`, `held` or `erased` |
| `workarea_id` | STRING | no | The workarea |
| `writer_identity` | STRING | no | The writing key |
| `principal` | STRING | no | Acting principal |
| `same_user` | BOOLEAN | yes | The `same_user` flag of the promotion event, when it carries one |
| `retirement` | BOOLEAN | no | Whether the version proposes retiring the memory |
| `replaces_artifact_id` | STRING | yes | The version this one replaces |
| `plaintext_digest` | STRING | no | SHA-256 of the plaintext. It survives erase and proves what the erased row was about. |
| `ciphertext_digest` | STRING | yes | SHA-256 of the sealed body. Null once erased. |
| `body_ciphertext` | BINARY | yes | The body sealed under the version's own key. Null once erased. |
| `body_ref` | STRING | no | Where the body lives. Today always `lake`. |
| `kek_id` | STRING | yes | The key-encryption key that wraps the version's key |
| `body_bytes` | LONG | no | Size of the plaintext body in bytes |
| `created_at` | TIMESTAMP | no | Submission time |
| `promoted_at` | TIMESTAMP | no | Approval time |
| `retracted_at` | TIMESTAMP | yes | Set when the state is not `promoted` |
| `applied_seq` | LONG | no | Outbox sequence of the last operation applied to this row |

## `gold.lineage`

Ratified edges and each version's own links, for lineage analytics. Pending edges stay in PostgreSQL. The
gate changes visibility, and an unratified edge is visible nowhere, the lake included. The merge key is
`edge_id`. Unpartitioned.

| Column | Type | Null | Meaning |
|---|---|---|---|
| `tenant_id` | STRING | no | Owning tenant |
| `edge_id` | STRING | no | Edge id. The merge key. |
| `graph` | STRING | no | `claim` or `brief` |
| `edge_label` | STRING | no | `derives_from`, `supersedes`, another edge label, or `brief` |
| `src_id` | STRING | no | Source memory version |
| `dst_id` | STRING | no | Destination memory version, or the workarea for a `brief` edge |
| `state` | STRING | no | `ratified` |
| `proposer` | STRING | yes | Proposing key |
| `ratifier` | STRING | yes | Ratifying key. Never the proposer. |
| `created_at` | TIMESTAMP | no | When the edge was created |
| `applied_seq` | LONG | no | Outbox sequence of the last operation applied |

## `telemetry.recall_daily`

Daily recall counts. The writer rewrites the table from the 30 days that PostgreSQL keeps, so a day older than
30 leaves the lake as it leaves PostgreSQL. It holds counts only. Partitioned by `day`.

| Column | Type | Meaning |
|---|---|---|
| `tenant_id`, `day`, `pipeline`, `access` | STRING, DATE, STRING, STRING | The group |
| `recalls`, `hits`, `empty_recalls` | LONG | Recalls, hits returned, recalls with none |
| `with_query_text` | LONG | Recalls whose query text was recorded |
| `outcome_get`, `outcome_derived`, `outcome_superseded` | LONG | What readers did next |
| `outcome_feedback_useful`, `outcome_feedback_not_useful` | LONG | Feedback on a hit |
| `outcome_requery`, `outcome_no_use` | LONG | A repeated query, and no follow-up |

## Provider tables

`lake.batches` has one row per drained batch: `batch_start`, `batch_end`, `tenants`, `events` and
`committed_at`. Its `batch_end` is the writer's frontier. `bench.results` has one row per metric and category
of each loaded `oml-bench/1` file. Its merge key is `run_digest`, `category` and `metric`. It is partitioned by
`run_date`.

## Commit protocol

The writer drains the outbox in batches. Every Delta commit is a put-if-absent of the next log entry. It
records an application transaction id, `appId` `oml-lakewriter`, with `version` equal to the last outbox
sequence applied to that table. A second writer either loses the conditional put or finds the version already
advanced and skips the batch. The outcome is a retried, deterministic batch, never a duplicate.

The Delta log's transaction version is the recovery authority. The frontier in PostgreSQL is a cache. An object
store without conditional put is not a supported lake target.

## Retracts

A demote or an erase is a Delta `MERGE` on `silver.claims`, applied in outbox order with materialization.
Merges are copy-on-write. They rewrite the files that hold matched rows and leave the old files unreferenced.

| Retract | `silver.claims` | `bronze.events` |
|---|---|---|
| Demote | `state` becomes `demoted`, `retracted_at` is set | A new event row |
| Erase | `state` becomes `erased`, `body_ciphertext` and `ciphertext_digest` become null, `retracted_at` is set. Digests and attribution remain. | A new event row |

An erase that cannot be applied is a dead letter. The old ciphertext stays in the live table version until it is
repaired, and `oml-lakewriter doctor` reports `lake.dead_letter`.

## Maintenance

Maintenance runs inside the lake writer, once a day by default (`--maintain-every-s 86400`). Each pass, per
table, compacts small files with a bin-pack `OPTIMIZE`, writes a checkpoint, removes expired log entries, and
runs `VACUUM`. Delta also checkpoints every 100 commits.

The **lake erasure bound** sets both the `VACUUM` retention and the log retention on every table. The default is
7 days (`--retention-hours 168`). After an erase, old files that hold the sealed body open under no key. They
persist until the first daily pass after the bound expires. Shortening the bound tightens that window and
shrinks the time-travel history that can be read.
