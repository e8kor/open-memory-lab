<!--
id: adr/ADR-0002-postgresql-system-of-record
tier: engineering
status: accepted design
answers: why PostgreSQL is the sole system of record and commit point; why each tenant gets a signed hash chain; what the synchronous journal durability domain covers
-->

# ADR-0002 — PostgreSQL is the system of record: one commit point, per-tenant signed chains, a synchronous journal domain

**Status:** accepted design · 2026-10-29 · [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d13 supersedes d6's every-replica-loads-the-key statement — the signing key is the lead's alone; the attests-the-deployment boundary stands

Everything this record decides is design — no code exists. Mechanics (schemas, the outbox
protocol, the chain layout) are owned by [architecture/storage.md](../architecture/storage.md);
this record decides and gives the reasons.

## Context

OML's journal is the record of every act — capture, verdict, ratification, promotion, demotion,
erasure, grant, ban — and the metadata beside it (memories, workareas, grants, idempotency
records) is what locates, indexes, audits and traces every memory. A write the service has
acknowledged must be a write the journal holds, and a write the journal holds must be a write the
metadata and the outbox agree on. That is a statement about a transaction, and only one store can
own it.

The journal is append-only, signed and hash-chained, the single source of truth, with every
projection a rebuildable view. The authority split follows from that: one PostgreSQL transaction
commits the signed event, the authoritative state and the durable outbox work, with Delta Lake as
an asynchronous projection and never a second authority. This record commits to that split and
answers the questions a multi-tenant, replicated service forces: tenancy (one chain per tenant,
not one per deployment), a stated replication guarantee for the journal's durability domain,
digest-only payloads so erasure and the chain can coexist, and an explicit answer to what the
signature attests when the server is N replicas.

A journal also needs a single writer per chain, and the right unit of exclusion is the
authority's own: where the authority is a database, the database serialises its writers, and no
external guard adds anything. OML's authority is PostgreSQL, so exclusion is the database's — a
transaction-scoped advisory lock per tenant chain head — and identity belongs to the deployment
(one signing key per deployment, loaded by every replica), never to a machine, directory or
volume.

## Decision

- **d1 — One PostgreSQL transaction is the commit point.** A write commits the signed journal
  event, its metadata rows, and a durable outbox row in one transaction, and the write is durable
  iff that transaction commits. Nothing downstream — the lake, the indexes, any cache — is part of
  the durability statement; everything downstream is derived from the outbox or rebuilt from the
  journal.

- **d2 — The journal's durability domain is synchronously quorum-replicated.** The guarantee in d1
  holds only when the Postgres cluster runs `synchronous_commit = on` or `remote_apply`
  (`remote_write` does not survive a standby operating-system crash and does not carry this claim)
  with quorum synchronous standbys; the deployment profiles pin this. The local single-node
  profile cannot provide it, and its documentation says so rather than implying the production
  guarantee: on one node, "durable" means one disk. The cost is quorum round-trip latency on every
  write; that latency is unmeasured and is tracked as its own measurement row (FR-18) on the
  [roadmap](../product/roadmap.md).

- **d3 — The hash chain is per tenant.** One chain-head row per `tenant_id`. Chain assignment —
  per-tenant `seq`, `prev_hash`, ed25519 signature — is serialized by
  `pg_advisory_xact_lock` on the tenant's chain head, held to commit, so commit visibility order
  equals `seq` order per tenant. The outbox insert happens inside that same serialized
  transaction, which is what lets the lake writer (ADR-0003) and the reconciler (ADR-0017) treat
  outbox order as journal order **per tenant**. Across tenants nothing is serialized: the global
  outbox `seq` is assigned at insert and becomes visible only at commit, so the global stream has
  no visibility order of its own — the lake writer's stable-prefix cut (ADR-0003 d4) is what makes
  draining it safe. The cost: one tenant's writes serialize on its chain head; writes
  of different tenants do not contend.

- **d4 — The journal table is append-only by mechanism, not convention.** `REVOKE UPDATE/DELETE`
  on the table plus a guard trigger. Corrections are new events; erasure is a journalled act that
  destroys a key, never a row (the erasure boundary is ADR-0005 of this register).

- **d5 — Journal and outbox payloads never carry body plaintext.** An event references content
  only by digest — the plaintext digest (content identity) and the ciphertext digest of the stored
  per-artifact-DEK encrypted object, both computed at admission, where the body is encrypted — and
  the ed25519 signature covers that digest envelope. So chain verification survives
  crypto-shredding: destroying a DEK invalidates no signature and breaks no link. The journalled
  ciphertext digest is also what gives the key-less reconciler (ADR-0017) a journal-side comparand
  for store bytes. Identity metadata in the chain (writer
  identity, principal) is minimized but present — attribution is what reconciliation and evidence
  exports need — and its erasure or pseudonymization is an open FR on the
  [roadmap](../product/roadmap.md), stated there rather than implied here.

- **d6 — The signature attests the deployment, and every replica holds the key.** Every
  `oml-server` replica loads the same deployment signing key (Kubernetes Secret or KMS). What is
  single is chain-head assignment inside Postgres (d3), not a signer process. The honest reading
  is stated as the threat model: compromise of any replica is compromise of the key. A unique
  per-process signer would not change that boundary, because every replica must be trusted to
  write anyway; it would only add key distribution and rotation surface.

- **d7 — Startup runs a fork check.** Before accepting writes, `oml-server` verifies that the
  stored chain head verifies against the last signed head. This detects commits lost to a failover
  under reduced synchronous guarantees (and any history rewrite). Detection is the backstop, not
  the guarantee — d2 is the guarantee; a failed fork check refuses service rather than signing on
  top of a fork.

## Consequences

- Every write pays the price of its guarantee: a quorum round trip (d2) and a per-tenant
  serialization point (d3). Both are stated costs with unmeasured magnitudes; the Postgres scale
  ceiling and partition strategy FR on the [roadmap](../product/roadmap.md) carries the
  measurement with an exit criterion.
- Chain verification is keyless and erasure-proof: an auditor (or the reconciler, ADR-0017) can
  verify every link from digest/ciphertext envelopes without a DEK and after any shredding (d5).
- The local single-node profile is honest about itself: same code, reduced durability claim, said
  in its own documentation (d2).
- Everything downstream of the commit point is derived. The lake consumes the outbox
  (ADR-0003); a divergence is found and repaired by folding the journal (ADR-0017); no recovery
  path reads anything but the journal to decide what is true.
- The per-tenant chain makes the per-tenant evidence export self-contained: one tenant's chain
  verifies without any other tenant's bytes.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Delta Lake as the system of record** | The commit point must atomically bind the journal event, the metadata, and the outbox row; no transaction spans Postgres and a Delta commit, so a Delta authority is a dual write: either the service blocks its write path on object-store commit latency, or the two stores diverge and a reconciler must decide which one is right — which makes the reconciler the authority. Delta as sole canonical body store or operational authority is rejected for the same reason. Delta's job is ADR-0003's. |
| **One global chain per deployment** | Two independent defects. Hot row: every tenant's write serializes on one chain-head lock, so one noisy tenant throttles all of them. Export privacy: a per-tenant evidence export must carry neighbours' event hashes to be verifiable, leaking the existence and timing of other tenants' events. A per-tenant chain is verifiable within the tenant's own export and contends only with itself. |
| **Event sourcing through a broker (Kafka) instead of an in-transaction outbox** | A broker between the commit point and the lake is a third store with its own durability, ordering and retention to operate and reconcile. The outbox row commits in the same transaction as the event (d1, d3), which is the whole exactly-once story; a broker adds a hop that must itself be made exactly-once. Rejected here and again at the lake boundary (ADR-0003). |
| **Per-replica signing keys** | It looks like it attests more — which process signed — but chain assignment is serialized in Postgres regardless, and every replica can reach the chain head, so the trust boundary does not move. It buys key-management burden (N keys to rotate, revoke, and correlate in verification) and no reduction in what a compromised replica can do. d6 states the boundary honestly instead. |
| **A singleton signer service** | Reintroduces a single process on the hot write path with its own liveness and failover story. The database already serializes chain assignment; a signer singleton adds an availability dependency and removes nothing from the threat model while every server still handles plaintext. |
| **Asynchronous replication with fork detection only** | A failover can lose acknowledged signed commits; the fork check (d7) detects the fork but cannot restore the lost suffix, and a journal whose honesty is "we notice when history is gone" is not a system of record. Detection stays as the backstop under d2's quorum guarantee. |
| **Body plaintext in journal payloads** | Erasure (crypto-shredding) would then require rewriting signed bytes, which breaks the chain — or erasure would be impossible, which breaks the product. Digest-only payloads (d5) are what let R4 ("no silent deletion") and erasure coexist. |
