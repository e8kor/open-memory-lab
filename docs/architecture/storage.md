---
status: Stable
verified: 2026-11-14
answers: "where the transactional commit point is; how the per-tenant signed hash chain, the outbox and the derived lake fit together; why journal payloads carry digests and no body text; where held bodies live"
---

# Storage: commit point, chain, outbox, lake

PostgreSQL is the system of record. The journal's hash chain makes its history tamper-evident. The lake is a copy that nothing reads online. This page explains how those three fit together. Column-level detail is in the [Delta tables](../data/delta-tables.md) and the [event catalog](../data/event-catalog.md). The byte-level chain rules are in the [chain format](../data/chain-format.md). Erasure and copies are in [audit and lineage](audit-and-lineage.md), and the read path that never touches the lake is in [retrieval](retrieval.md).

## The commit point

PostgreSQL is the only transactional commit point. One Postgres transaction commits three things atomically:

- the signed journal event, appended to `journal`;
- the metadata rows the event implies, which are the fold state;
- a durable `outbox` row for everything the lake must eventually see.

A write is durable if and only if that transaction commits. There is no second commit point. The lake, the search indexes, and every export derive from what this transaction made durable. So "did it happen?" has one answer from one authority.

The cost: every write pays Postgres transaction latency. The journal table grows without compaction.

**Durability needs synchronous replication.** Journal durability across a failover requires `synchronous_commit = on` or `remote_apply` with quorum synchronous standbys. Without them, a failover can lose committed events. A single-node install has no standbys, so its durability is one machine's disk.

## The per-tenant hash chain

The journal is hash-chained per tenant, with one chain-head row for each tenant. Each event gets a per-tenant sequence number (`seq`), the hash of the previous event (`prev_hash`) and an Ed25519 signature. A transaction-scoped advisory lock on the tenant's chain head serializes this assignment. The lock is held until the transaction commits. So commit order equals `seq` order per tenant, and no observer sees event *n+1* durable while event *n* is in flight. The outbox insert happens inside the same transaction, so the outbox inherits that order.

The cost: each tenant's writes serialize through one lock, so per-tenant write throughput is bounded by it. Tenants do not contend with each other.

Each tenant's chain opens with a `tenant.genesis` event that carries the tenant's id and name. A provider chain with `tenant.created` events, under a reserved provider tenant, is designed and not built. The deployment folds out of per-tenant chains with no global sequence.

OML enforces append-only mechanically. It revokes `UPDATE` and `DELETE` on `journal` and adds a guard trigger.

**What the signature means.** Only the lead loads the signing key. The signature attests the deployment, not a process: nothing enforces a single signer process, and no takeover event is journaled. The chain proves the deployment's key signed these events in this order. It does not prove which process signed. It does not protect against an attacker who holds the key ([security](security.md)).

**Fork check.** Before it accepts writes, `oml-server-lead` verifies every stored chain end to end against its stored head, under the current signing key. This detects a failover that lost committed events and would otherwise fork the chain. The server refuses to write onto a forked head.

## Digest-only journal payloads

Journal payloads never carry body text. An event references content by its plaintext digest, which identifies the content. The lead stores the body unencrypted in PostgreSQL, so the payload's `ciphertext_digest` is null. The lake writer seals each lake copy under a per-version data key of its own. The signature covers the digest envelope and not the bodies.

This has three effects.

- **Chain verification survives erasure.** Erasure, designed ([ADR-0005](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0005-erasure-by-crypto-shredding.md)) and not built, would destroy a memory's encryption key while the signed bytes stay untouched, so the chain still verifies end to end. In this build only the lake copy has such a key; the PostgreSQL copy is plain text.
- **Comparison needs no secret.** Anyone who can read a body can check it against the signed plaintext digest.
- **The journal alone cannot reproduce content.** Reading history back needs the body stores.

The chain also keeps identity metadata, such as the writer and principal. Erasing attribution is not available.

## Time

The lead's system clock supplies each event's `at` when it signs the event, before the transaction opens. Only the lead signs, so one clock stamps a deployment's events.

Chain order is `seq` and only `seq`. A failover or a clock step can make `at` go backward between consecutive `seq` values without breaking any guarantee. Verification orders by `seq`.

Run NTP on the lead's host. Without it, chain integrity does not degrade, but everything compared with wall-clock time drifts silently: serve expiry, read-log windows, and retention and backup horizons.

## The lake and the outbox

Delta Lake is a derived copy for analysis. It serves no client read. Recall, search and get all answer from Postgres. When the journal and the lake disagree, the journal wins. The benefit is that the lake's write path can be asynchronous and batched without a serving outage. The cost is a second storage system whose only readers are batch jobs. See [the lake](../operations/lake.md) for how to run it.

**What reaches the lake.** No held body ever reaches the lake. `bronze.events` mirrors journal events with digest-only payloads. Silver materialization is enqueued by approval only. Lake bodies are ciphertext. Lake rows carry the artifact id, workarea id, writer, principal, timestamps, the plaintext digest, the lake ciphertext's digest and state.

**How the writer drains the outbox.** One `oml-lakewriter` runs at a time under a leader lease. It reads outbox rows in sequence order, in micro-batches.

1. It cuts a batch from the stable prefix only. Outbox sequence numbers are assigned at insert, but a row becomes visible when its transaction commits. A smaller number can therefore appear after a larger one. The writer reads only rows older than every transaction still active at scan time. A long-running transaction holds this back and shows up as lag, never as loss.
2. It groups rows by target Delta table.
3. For each table, it checks the last transaction version it committed. If the table already holds this batch, it skips the table.
4. It applies each table's rows in sequence order and commits with an application transaction id that records the batch end.
5. It advances the frontier, the transaction version of the lake's own `lake.batches` table, only after every table committed. The Delta log is the recovery authority. The frontier is a convenience for restart and lag metrics.

After a crash, the writer rebuilds the same batch from the recorded bounds. It never re-cuts by time or size. Tables that committed skip it, and the rest apply it.

**Retracts keep order.** A retract, such as a demotion or reassignment (neither built yet), is a Delta `MERGE` applied in the same sequence order as materialization. A promotion at sequence 100 and its demotion at 140 end in the demoted state, even in one batch. The cost is head-of-line blocking: a slow materialization delays the retracts behind it.

**A failing row is dead-lettered.** A row a lake table cannot take is recorded in the `lake_dead_letter` table with a reason, and the writer moves on. No journal event is written: the lake writer holds no signing key. The lake is then knowingly incomplete until repair. A stalled frontier would hide the same problem inside unbounded lag. `oml-lakewriter doctor` reports the count.

Lake writer throughput and Delta commit latency have not been measured.

## Where held bodies live

OML holds captured bodies inline in the PostgreSQL `memories` table, unencrypted, under the tenant's row security. There is no object store. A held body never goes to the lake. Its readers are its author, the reviewer it is served to, the readers of an in-progress workarea that holds it when it is a memory or derived fact the scan did not hold and last scanned under the current rule set, and an admin through the journaled admin read ([roles and keys](roles-and-keys.md#the-admin-read)). Approval alone releases a body to the lake, and the outbox carries it ([admission](admission.md)).
