---
status: Stable
verified: 2026-11-06
answers: "where the transactional commit point is; how the per-tenant signed hash chain, the outbox and the derived lake fit together; why journal payloads carry digests and no body text; where held bodies live"
---

# Storage: commit point, chain, outbox, lake

PostgreSQL is the system of record. The journal's hash chain makes its history tamper-evident. The lake is a copy that nothing reads online. This page explains how those three fit together. Column-level detail is in the [Delta tables](../data/delta-tables.md) and the [event catalog](../data/event-catalog.md). The byte-level chain rules are in the [chain format](../data/chain-format.md). Erasure and copies are in [audit and lineage](audit-and-lineage.md), and the read path that never touches the lake is in [retrieval](retrieval.md).

## The commit point

PostgreSQL is the only transactional commit point. One Postgres transaction commits three things atomically:

- the signed journal event, appended to `journal`;
- the metadata rows the event implies, which are the fold state;
- a durable `outbox` row for everything the lake must eventually see.

A write is durable if and only if that transaction commits. There is no second commit point. The lake, the full-text and vector indexes, and every export derive from what this transaction made durable. So "did it happen?" has one answer from one authority.

The cost: every write pays Postgres transaction latency. The journal table grows without compaction.

**Durability needs synchronous replication.** Journal durability across a failover requires `synchronous_commit = on` or `remote_apply` with quorum synchronous standbys. Without them, a failover can lose committed events. A single-node install has no standbys, so its durability is one machine's disk.

## The per-tenant hash chain

The journal is hash-chained per tenant, with one chain-head row for each tenant. Each event gets a per-tenant sequence number (`seq`), the hash of the previous event (`prev_hash`) and an Ed25519 signature. A transaction-scoped advisory lock on the tenant's chain head serializes this assignment. The lock is held until the transaction commits. So commit order equals `seq` order per tenant, and no observer sees event *n+1* durable while event *n* is in flight. The outbox insert happens inside the same transaction, so the outbox inherits that order.

The cost: each tenant's writes serialize through one lock, so per-tenant write throughput is bounded by it. Tenants do not contend with each other.

Provider-level acts have a chain of their own, under a reserved provider tenant. Each new tenant's chain opens with a genesis event that references the provider chain's `tenant.created` event. The deployment therefore folds out of per-tenant chains with no global sequence.

OML enforces append-only mechanically. It revokes `UPDATE` and `DELETE` on `journal` and adds a guard trigger.

**What the signature means.** Only the lead loads the signing key. The signature attests the deployment: one signer process at a time, with each takeover journaled as `system.lead_acquired`. The chain proves the deployment's key signed these events in this order. It does not prove which process signed beyond the lead record. It does not protect against an attacker who holds the key ([security](security.md)).

**Fork check.** Before it accepts writes, `oml-server` verifies that the stored chain head matches the last signed head. This detects a failover that lost committed events and would otherwise fork the chain. The server refuses to write onto a forked head.

## Digest-only journal payloads

Journal payloads never carry body text. An event references content only by digest: the plaintext digest, which identifies the content, and the ciphertext digest of the stored encrypted object. `oml-server` encrypts the body under the memory's key at admission, so both digests exist when it signs the event. Every later copy is those same ciphertext bytes. The signature covers the digest envelope and not the bodies.

This has three effects.

- **Chain verification survives erasure.** Erasing a memory destroys its encryption key. The body becomes unrecoverable, and the signed bytes stay untouched. The chain still verifies end to end.
- **Comparison needs no keys.** Anyone can check stored bytes against the signed ciphertext digest without decrypting.
- **The journal alone cannot reproduce content.** Reading history back needs the body stores.

The chain also keeps identity metadata, such as the writer and principal. Erasing attribution is not available.

## Time

The Postgres primary's clock, read inside the committing transaction, supplies every event's `at`. No application clock takes part. One clock per deployment makes timestamps comparable without asking which machine stamped them.

Chain order is `seq` and only `seq`. A failover or a clock step can make `at` go backward between consecutive `seq` values without breaking any guarantee. Verification orders by `seq`.

Run NTP on the host that runs Postgres. Without it, chain integrity does not degrade, but everything compared with wall-clock time drifts silently: serve expiry, read-log windows, and retention and backup horizons.

## The lake and the outbox

Delta Lake is a derived copy for analysis. It serves no client read. Recall, search and get all answer from Postgres. When the journal and the lake disagree, the journal wins. The benefit is that the lake's write path can be asynchronous and batched without a serving outage. The cost is a second storage system whose only readers are batch jobs. See [the lake](../operations/lake.md) for how to run it.

**What reaches the lake.** No held body ever reaches the lake. `bronze.events` mirrors journal events with digest-only payloads. Silver materialization is enqueued by approval only. Lake bodies are ciphertext. Lake rows carry the artifact id, workarea id, writer, principal, timestamps, both digests and state.

**How the writer drains the outbox.** One `oml-lakewriter` runs at a time under a leader lease. It reads outbox rows in sequence order, in micro-batches.

1. It cuts a batch from the stable prefix only. Outbox sequence numbers are assigned at insert, but a row becomes visible when its transaction commits. A smaller number can therefore appear after a larger one. The writer reads only rows older than every transaction still active at scan time. A long-running transaction holds this back and shows up as lag, never as loss.
2. It groups rows by target Delta table.
3. For each table, it checks the last transaction version it committed. If the table already holds this batch, it skips the table.
4. It applies each table's rows in sequence order and commits with an application transaction id that records the batch end.
5. It advances the frontier in Postgres only after every table committed. The Delta log is the recovery authority. The frontier is a convenience for restart and lag metrics.

After a crash, the writer rebuilds the same batch from the recorded bounds. It never re-cuts by time or size. Tables that committed skip it, and the rest apply it.

**Retracts keep order.** A retract, such as a demotion or reassignment, is a Delta `MERGE` applied in the same sequence order as materialization. A promotion at sequence 100 and its demotion at 140 end in the demoted state, even in one batch. The cost is head-of-line blocking: a slow materialization delays the retracts behind it.

**A failing row is dead-lettered.** After a bounded retry budget, the writer journals a `materialize-skipped` event, moves the row to the `dead_letter` table and moves on. The lake is then knowingly incomplete until repair. A stalled frontier would hide the same problem inside unbounded lag. `oml-lakewriter doctor` reports the count.

Not measured yet: lake writer throughput and Delta commit latency.

## Where held bodies live

OML holds captured bodies in Postgres or in a quarantine bucket, `oml-quarantine/<tenant>/<digest>`, reachable only by service credentials. A held body never goes to the lake. The quarantine store sits outside every consumer-readable zone and outside the outbox stream. A held body has no path to any reader other than its own author and the reviewer it is served to. Approval alone releases a body to the lake, and the outbox carries it ([admission](admission.md)).
