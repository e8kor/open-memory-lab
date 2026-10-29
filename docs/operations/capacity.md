<!--
id: operations/capacity
tier: engineering
status: design draft for review · decided, not built
answers: what grows with load and by what formula; which resource binds each workload; how to size a deployment symbolically before FR-7/FR-18/FR-25 produce numbers
-->

# Capacity

**Status:** design draft for review · decided, not built — no OML measurement
exists; every quantity here is a formula over symbols, and the rows that
replace the symbols with measurements are
[FR-7](../product/roadmap.md), [FR-18](../product/roadmap.md) and
[FR-25](../product/roadmap.md).

This page declares the **growth drivers**: what each unit of work writes,
what accumulates, and which resource each workload exhausts first. It states
no number — not a throughput, not a byte ceiling, not a latency — because
none has been measured ([ADR-0018](../adr/ADR-0018-the-documentation-set.md)
d3). What it can state honestly are the *shapes*: the write amplification of
one submit is a design fact, the arithmetic of an HNSW index over d-dimensional
vectors is a format fact, and a formula whose inputs are symbols is checkable
against the design even before the rig of
[ADR-0024](../adr/ADR-0024-performance-foundation.md) runs.

## What one submit writes

The commit point ([storage](../architecture/storage.md)) makes the write
amplification of a single submit enumerable:

1. **Before the transaction**: one idempotency record, claimed in its own
   transaction ([HTTP API](../api/http-api.md)); the scan; DEK generation and
   KMS wrap; AES-GCM encryption; and — for bucket-placed bodies — one
   quarantine object PUT
   ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d1).
2. **Inside the one transaction**: the signed journal row (digests only,
   never body bytes), the fold-state rows (`memories` and friends,
   [postgres-schema](../data/postgres-schema.md)), and one outbox row —
   committed atomically, serialized per tenant on the chain lock.
3. **Downstream, asynchronously**: the lake-writer drains the outbox row into
   a bronze event row and a silver MERGE
   ([delta-tables](../data/delta-tables.md)); after promotion, the indexer
   writes one `tsvector` row and one embedding row
   ([retrieval](../architecture/retrieval.md)).

A memory's full life adds more journal events than the submission alone:
scan verdict, one event per review verdict, the ratification — each a journal
row plus an outbox row. Write with **E** for journalled events per admitted
memory (its value depends on the review policy's `k`,
[admission](../architecture/admission.md)); E is a small integer fixed by
policy, not by load.

Journal growth is therefore linear and uncompacted by design — the journal
table "grows without compaction"
([storage](../architecture/storage.md)) — and the question "how many events,
memories and tenants before one deployment's Postgres degrades" is exactly
[FR-7](../product/roadmap.md)'s exit criterion. Full-genesis fold time over
that growing journal, and commit latency under quorum-synchronous
replication, are split out as [FR-18](../product/roadmap.md).

## Ciphertext body sizing

Bodies are per-artifact-DEK AES-256-GCM ciphertext everywhere at rest
([ADR-0004](../adr/ADR-0004-bodies-and-attachments.md)). AES-GCM adds a
constant per-message overhead (nonce and authentication tag — protocol
constants, not measurements), so ciphertext size tracks plaintext size; no
compression is claimed anywhere in this design.

The multiplier that matters is **copy classes**, not encryption overhead. One
body's bytes exist, over time, in: the quarantine object; the lake copy
(silver `body_ciphertext` or its object-store twin, per
`bodies.placement_threshold` — open, [FR-15](../product/roadmap.md));
superseded parquet versions until `VACUUM`; object-store versions until
lifecycle expiry; and every backup inside its retention window. Each class is
named, with its lifetime bound, in the backup table of
[deployment](deployment.md) and the erasure inventory of
[audit and lineage](../architecture/audit-and-lineage.md). For sizing, write
**C** for the number of copy classes a deployment's windows keep live at
once: body storage ≈ admitted body bytes × C, and C is set by the operator's
declared windows ([configuration](configuration.md)), not by OML.

## pgvector memory

`memory_embeddings` is list-partitioned per tenant with a per-partition HNSW
index ([retrieval](../architecture/retrieval.md)). The raw vector arithmetic
is a format fact: one row stores **d × 4 bytes** of float32 (`vector`), so

```
embedding bytes per tenant ≈ promoted rows × d × 4 + HNSW graph overhead
```

where **d** is the embedding dimension — open until the embedding model is
chosen ([RS-1](../product/roadmap.md)) — and the HNSW graph adds per-row link
storage that scales with the index's connectivity parameters. The operative
capacity fact is not storage but **RAM ratio**: cited third-party measurement
found HNSW throughput varying several-fold across the tested
index-to-memory range ([ADR-0024](../adr/ADR-0024-performance-foundation.md),
which carries the citation), which is why recall latency at stated corpus
sizes and index-versus-RAM ratios is a required axis of
[FR-25](../product/roadmap.md)'s rig and the recall/latency behavior of
filtered HNSW is [FR-4](../product/roadmap.md). Per-tenant partitioning
bounds each index's build cost and blast radius, and makes the capacity
question per-tenant: the largest tenant's index against the page cache, not
the sum.

## Delta log growth versus vacuum cadence

Every lake micro-batch is one Delta commit per touched table, so log entries
grow with drain frequency, not with row count. The bounds are the maintenance
cadences ([delta-tables](../data/delta-tables.md)):

- **Checkpoints every 100 commits** bound log-replay cost on open.
- **Bin-pack OPTIMIZE, daily per tenant**, repairs the small files that
  micro-batches and daily bronze partitions produce by design — the stated
  shape is one small file per day per tenant at low volume.
- **VACUUM, daily, retention = the table's erasure bound**, deletes
  unreferenced parquet. Until it runs, superseded versions — including files
  rewritten by earlier OPTIMIZE runs — persist and are time-travel readable.
  Those versions hold ciphertext of erased bodies whose DEKs are already
  destroyed, so the window bounds tidiness, not exposure
  ([deployment](deployment.md)).

The sizing consequence: lake storage carries a tail proportional to
rewrite volume × vacuum retention, and Delta commit cost is known to grow
with version-history depth — one reason the lake write path is asynchronous
by design ([observability](observability.md)). The revisit trigger is a
measured table size at which one OPTIMIZE run can no longer rewrite the
working set in its window ([delta-tables](../data/delta-tables.md)).

## Read-log volume

Every read appends to `read_events`: a recall logs the query digest and the
ids served; a fetch logs the served memory
([retrieval](../architecture/retrieval.md)). Reads are expected to be orders
of magnitude more frequent than writes — the stated reason the read log sits
outside the journal chain — so `read_events` is the fastest-growing table in
a read-heavy deployment. Its bound is structural: partitioned, dropped on the
tenant's retention window, with only batch digests entering the journal. The
journal-side cost of reading is therefore one signed event per anchor batch
(cadence: `readlog.anchor_cadence`, [configuration](configuration.md)), not
one per read.

## What binds each workload

| Workload | Binding resource | Why |
|---|---|---|
| `oml-server` (lead), per tenant | fsync + quorum round-trip, held inside the chain lock | The lock holds read-head + hash + sign + one insert + commit and nothing else, so per-tenant single-submit throughput is on the order of the reciprocal of commit latency — arithmetic over cited context, measured at [FR-18](../product/roadmap.md)/[FR-25](../product/roadmap.md) ([ADR-0024](../adr/ADR-0024-performance-foundation.md)) |
| `oml-server` (lead), aggregate | group-commit amortization across tenants; then admission CPU and KMS wrap-call concurrency | Tenants do not share locks; the single-lead ingest ceiling is a named revisit trigger ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d9), measured next to [FR-7](../product/roadmap.md) as [FR-24](../product/roadmap.md) |
| `oml-server` (followers) | page cache — the index-versus-RAM ratio of the replica | Followers serve recall and get from local replicas; staleness equals replication lag, exposed as a metric ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d9) |
| `oml-lakewriter` | object-store PUT latency per Delta commit | Each micro-batch is a conditional-put commit per touched table; frontier lag, not CPU, is the signal that pages ([observability](observability.md)) |
| `oml-indexer` | embedding throughput | The model and its location are open ([RS-1](../product/roadmap.md)); the visible symptom of saturation is `oml_index_lag_seconds` — the gap between promoted and findable |
| `oml-reconciler` | journal scan rate against its pass deadline | Incremental passes are bounded; full-genesis fold time grows with the journal and is [FR-18](../product/roadmap.md)'s to measure |
| Postgres | everything above, eventually | The one store under the journal, folds, indexes, outbox and read log; its ceiling and the partition strategy past it are [FR-7](../product/roadmap.md) |

## A worked sizing example — symbolic by design

No input below is a number, because no measured number exists. The example is
the *procedure* an operator runs once
[FR-7](../product/roadmap.md), [FR-18](../product/roadmap.md) and
[FR-25](../product/roadmap.md) publish measured values for the symbols.

At **X** submits/s of **Y**-KiB bodies across **T** active tenants, with a
review policy producing **E** journal events per admitted memory, **R**
recalls/s, embedding dimension **d**, and **C** live copy classes:

- **Journal and outbox rows**: ≈ X × E rows/s each, plus fold-row churn in
  the same transactions. Journal bytes grow at X × E × (envelope size) —
  envelopes carry digests and attribution, never bodies, so journal growth is
  independent of Y.
- **Body bytes**: ≈ X × Y KiB/s × C across quarantine, lake and retained
  versions/backups; the AES-GCM delta is a constant per body.
- **Per-tenant write ceiling**: a single tenant cannot exceed ≈ 1 ÷ (commit
  latency under quorum replication) submits/s — the structural floor
  ([ADR-0024](../adr/ADR-0024-performance-foundation.md)); batch submit
  ([FR-12](../product/roadmap.md)) amortizes it by signing N events under one
  lock acquisition and one commit.
- **Aggregate write throughput**: scales with T up to group-commit and
  single-lead limits — the quantity [FR-7](../product/roadmap.md) and
  [FR-24](../product/roadmap.md) measure.
- **Embedding store**: grows at (promoted rate) × d × 4 bytes/s per tenant,
  plus HNSW overhead; serving degrades as the largest tenant's index outgrows
  its page-cache share — the [FR-25](../product/roadmap.md) scenario-4 axis.
- **Read log**: ≈ R rows/s, bounded by the tenant retention window; journal
  impact is one anchor event per cadence interval, not per read.

Each bullet names its measurement row deliberately:
[FR-7](../product/roadmap.md) replaces the store-ceiling symbols (events,
memories, tenants per deployment), [FR-18](../product/roadmap.md) replaces
the commit-latency and full-genesis-fold symbols, and
[FR-25](../product/roadmap.md) replaces the per-ring latency and throughput
symbols with measured p50/p95/p99, published with date, hardware and
settings. Until those rows close, any deployment sizing done from this page
is arithmetic over assumptions, and should be labelled as such wherever it is
repeated.

## What this page does not decide

Resource requests, node counts and Postgres sizing (deployment-site choices,
[deployment](deployment.md)); every threshold (unmeasured,
[observability](observability.md)); the partition strategy past the Postgres
ceiling (recorded with [FR-7](../product/roadmap.md)'s measurement, not
before it).
