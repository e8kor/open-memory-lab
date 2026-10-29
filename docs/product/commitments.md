# Commitments

**Status:** design draft for review · decided, not built

This page is the honest contract. Every guarantee Open Memory Lab makes appears here, sorted
into four tiers: guaranteed by construction, guaranteed within a stated bound, provided but
unmeasured, and never claimed. A claim that does not appear on this page is not a commitment,
whatever a sentence elsewhere may suggest. Each entry names its enforcement point and the
assumption it rests on; every bound links the [roadmap](roadmap.md) row that measures it.
Nothing is built — "by construction" means the design makes violation impossible *if the code
implements the design*, and the assumptions column says what "implements the design" has to
include.

## 1. Guaranteed by construction

| # | Guarantee | Enforcement point | Rests on |
|---|---|---|---|
| C1 | A write is durable iff its single Postgres transaction commits; journal, metadata and outbox bind atomically. [Storage](../architecture/storage.md), [ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md) d1. | One transactional commit point. | No write path exists outside `oml-server`'s use-cases — the hexagonal layout ([ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d3) makes that a dependency-graph fact, not a convention. |
| C2 | Per-tenant commit-visibility order equals `seq` order. [ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md) d3. | `pg_advisory_xact_lock` on the chain head, held to commit. | Every journal-appending path takes the lock; the single commit-point port centralizes this. |
| C3 | The journal is append-only. [ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md) d4. | `REVOKE UPDATE/DELETE` plus a guard trigger. | The DBA/superuser is excluded (tier 4), **and** the service role does not own the table — an owner can re-grant to itself, which is why the owner/migration-role split in C6 is load-bearing here too. |
| C4 | No memory is visible to anyone but its author before a non-author review (R2 + R3). [Admission](../architecture/admission.md), [ADR-0001](../adr/ADR-0001-the-constitution.md) d2/d3. | Promotion-time lineage-closure check, `serve_id` binding, the held state. | Closure is only as good as declared derivation (tier 4); principal identity is tenant-asserted data, so collusion is visible, not prevented (tier 4). |
| C5 | The workarea ladder moves forward only. [Workareas](../architecture/workareas.md). | No backward transition exists as a verb or as a domain transition in `oml-domain`. | Nothing — the absence of a verb is the strongest enforcement shape in this register. |
| C6 | No tenant reads or writes another tenant's rows. [Security](../architecture/security.md), [ADR-0007](../adr/ADR-0007-tenancy-first-class.md). | Tenant derived server-side from the key; row-level security on every tenant table; no `BYPASSRLS` role. | Three deployment commitments the [deployment page](../operations/deployment.md) carries as requirements, not tuning: `FORCE ROW LEVEL SECURITY` on every tenant table (a table's owner otherwise bypasses its policies silently), a table-owner/migration-role split so the querying service role never owns what it queries, and `SET LOCAL` tenant context inside each pooled transaction so a pooled connection cannot leak one tenant into the next. |
| C7 | The lake-writer and reconciler cannot decrypt anything. [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d3, [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md). | Neither binary composes a KMS port — "keyless" is a type error, not a discipline. | Deployment IAM mirrors the composition; the KMS grant matrix is pinned in [deployment](../operations/deployment.md). |
| C8 | Simulation taint never takes the default tip. [Simulations](../architecture/simulations.md), [retrieval](../architecture/retrieval.md). | Audience-scoped tip computation, `simulation.tip_refused`, per-simulation indexes. | Declared derivation only. |
| C9 | No held body reaches any lake zone. [Storage](../architecture/storage.md). | Bronze mirrors digest-only payloads — digests are all the outbox carries; silver is enqueued by promotion only. | Nothing — there is no datum to leak. |
| C10 | Chain verification survives erasure. [ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md). | Signed envelopes carry digests, never bodies. | Pure construction. |
| C11 | No client key exceeds its role's verb ceiling (R1). [Roles and keys](../architecture/roles-and-keys.md). | Server-side per-role check; promote, sign and delete appear in no client verb column; every transport is an adapter over `oml-app`. | The adapter rule of [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d3. |
| C12 | `self_signed` rows never leave their workarea and never enter silver or gold. [Workareas](../architecture/workareas.md), [delta tables](../data/delta-tables.md). | An indelible schema mark; the lake exclusion is checkable by the [reconciler](../architecture/reconciliation.md). | Reconciler coverage. |
| C13 | A review verdict counts once, for exactly what was served. [Admission](../architecture/admission.md), [ADR-0012](../adr/ADR-0012-admission-and-review.md). | Server-side single-use serve binding to (reviewer key, artifact version, digest set). | Server-side state; no client replay path exists. |
| C14 | A promoted term's CURIE is permanent (never re-mintable with a different meaning), the `is_a`/`replaced_by` graphs are acyclic, and simulations cannot mint vocabulary. [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md). | The minted-at-first-promotion rule and the cycle refusal are `oml-domain` invariants (like the tip CAS); the simulation veto is a deterministic scan-class refusal. | `oml-domain` encoding the invariants (ADR-0019 d1); humanness of a human-required verdict is an issuance-time **attestation**, never verified by the server. |

## 2. Guaranteed within stated bounds

- **Erasure, including backups.** Destruction is immediate on live tables; for copies held in
  backups, it completes no later than the stated backup-retention plus KEK-rotation window.
  [Audit and lineage](../architecture/audit-and-lineage.md),
  [ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md). The bound's actual size is
  unmeasured — [FR-17](roadmap.md) produces it.
- **Tamper-evidence, not tamper-proofing.** The chain detects rewriting by anyone who cannot
  sign. Against a key-holding attacker with database write access — who can regenerate a whole
  consistent chain — detection rests entirely on chain heads retained *outside* the deployment,
  and no mechanism yet exists that pushes a head outside; evidence export is on-demand only.
  [RS-9](roadmap.md) decides an anchoring mechanism. Until it closes, tamper-evidence against a
  full-compromise attacker is only as good as the exports tenants happened to retain (tier 4).
- **Exactly-once lake materialization.** Rests on delta-rs transaction-identifier behavior
  that [FR-3](roadmap.md)'s extended conformance test must verify — including that the
  applied-version state survives checkpoints, `VACUUM`, and transaction-identifier retention
  settings. If the recovery authority is garbage-collected, the writer silently resets to
  "never applied" and duplicates. [Storage](../architecture/storage.md),
  [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md).
- **Durability.** "Failover cannot lose acknowledged commits" requires quorum-synchronous
  replication with `remote_apply` — `remote_write` does not survive a standby OS crash and does
  not carry the claim. The single-node local profile is one machine's disk, stated as such.
  [Storage](../architecture/storage.md).
- **Read-log integrity.** Reads already covered by an anchored batch digest are
  tamper-evident; the unanchored tail — reads not yet batch-anchored — can be dropped without
  detection. The exposure window is the anchoring cadence, and no cadence bound is committed
  yet. In cluster mode the window also covers follower-written rows awaiting the lead's anchor,
  and a crashed follower loses its unflushed async batch ([RS-13](roadmap.md)).
  [Audit and lineage](../architecture/audit-and-lineage.md).
- **Revocation and grant latency.** A revoked key or withdrawn grant can act until the bounded
  cache epoch lapses; the window is stated in [security](../architecture/security.md), not zero.
- **Rate limiting.** In cluster mode the split follows the topology: write-path limits are
  exact (one lead — nothing to divide); read-path limits are limit/N across followers, so a
  tenant can briefly exceed its read quota and the error grows with follower churn.
  Lead-only deployments are exact on both paths. [FR-8](roadmap.md).
- **Vector-scan completeness.** Iterative vector scans are bounded by pgvector's
  `max_scan_tuples` and scan-memory caps — a configurable cutoff, not exhaustiveness. The
  recall consequences are [FR-4](roadmap.md)'s to measure.
  [Retrieval](../architecture/retrieval.md).
- **Ban scope.** A ban acts at admission only; content already visible needs its own demotion
  or erasure act. [Admission](../architecture/admission.md).

## 3. Provided, quality unmeasured

These capabilities exist in the design, and nothing yet says how well they perform or how the
open choice resolves. Each line is one [roadmap](roadmap.md) row; the row's exit criterion
produces the number or the decision.

- Erasure of attribution metadata, end to end across every copy class — FR-1.
- Review throughput: what a deployment's reviewing crews clear per unit time — FR-2.
- Exactly-once lake application, as verified behavior rather than library assumption — FR-3
  (the bound in §2 carries it until the test exists).
- Search quality: recall, latency, exact-versus-approximate — FR-4.
- Decrypted per-audience export — FR-5.
- Bulk key rotation for crew-sized key sets — FR-6.
- The record store's scale ceiling and the partition strategy past it — FR-7.
- Rate-limit enforcement accuracy — FR-8 (bound in §2).
- The scoped market claim — FR-9, re-verified at every publication.
- The agent-surface provider floor: own-held recall, MCP get and supersede, the journalled
  author retire-proposal — FR-10.
- Token-budget retrieval (`max_tokens` with deterministic truncation) — FR-11.
- Batch submit and generated SDKs — FR-12.
- Dataset onboarding, v1 slice ([ADR-0020](../adr/ADR-0020-dataset-onboarding.md)) — FR-13.
- Import throughput and the recall impact of indexed imports — FR-14.
- Admission scan latency, and the open attachment-pipeline points of
  [ADR-0004](../adr/ADR-0004-bodies-and-attachments.md) — FR-15.
- Lake-writer throughput and Delta commit latency — FR-16.
- Erasure-SLO bounds, measured — FR-17.
- Full-genesis reconciler runtime and quorum write latency — FR-18.
- The vocabulary v1 slice (prefixes, term lifecycle, releases, scanner, read surface) — FR-19.
- Vocabulary expansion recall, closure storage, mapping quality, and the per-vocabulary
  human-verdict load — FR-20.
- The `/v1` provider-ring compatibility covenant: dormant until the v1-stable declaration —
  FR-21 ([ADR-0022](../adr/ADR-0022-two-ring-api.md)).
- Profile conformance (one suite green on standalone, docker, kubernetes) — FR-22; the
  migration rehearsal — FR-23; lead-failover semantics and the single-lead write ceiling —
  FR-24; provider-ring latency against declared targets — FR-25
  ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md),
  [ADR-0024](../adr/ADR-0024-performance-foundation.md)).
- Open questions, each a decision not yet taken: embedding model and boundary (RS-1), MCP
  OAuth (RS-2), federation (RS-3), the Merkle specification (RS-4), sampling-policy semantics
  (RS-5), the connector surface (RS-6), MWK rotation (RS-7), tenant-wide erased-digest
  matching (RS-8), external chain-head anchoring (RS-9), the `oml:` upper-vocabulary register
  (RS-10), the external-ontology import profile (RS-11), the standalone keyring mechanism
  (RS-12), the cluster read-log split (RS-13), the DEK serving-cache shape (RS-14).

### Profile-dependent bounds

Several §2 bounds depend on the install profile ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md));
the per-profile detail lives with the [deployment](../operations/deployment.md) page's profile
sections and [security](../architecture/security.md)'s "Custody by profile" table, and the
rule here is the contract: **a profile never changes what the system refuses — it changes what
backs the refusal.** Durability on standalone and default docker is one host's disk (quorum
restores the claim only against a quorum-replicated Postgres); KEK custody in the `kms-file`
profiles is a file on the host, so "never leaves KMS" is replaced, not weakened; the erasure SLO
gains the keyring-file copy class; and tamper-evidence against the host administrator in
standalone rests entirely on exported chain heads (RS-9). The constitution-tier rows of §1 are
profile-independent — including C6's three RLS commitments, which bind in all three profiles.

## 4. Never claimed

The design never claims the following, and no page may claim them without amending this
register.

- **Truth of claims.** Review attests process — who looked, under what policy, at what
  digests — never that the content is true. [ADR-0001](../adr/ADR-0001-the-constitution.md).
- **Lineage completeness.** `derives_from` is declared by the author; closure checks check
  what was declared. [Audit and lineage](../architecture/audit-and-lineage.md).
- **Protection against the database or KMS administrator.** A superuser or KMS administrator
  is outside what construction can reach; the chain makes their tampering evident (within the
  §2 bound), not impossible. [Security](../architecture/security.md).
- **Review quality.** The gate guarantees a non-author looked, not that they looked well.
- **Collusion prevention.** An author and reviewer who collude are visible in the trail, not
  prevented by it; principal identity is tenant-asserted data.
- **Client plaintext custody.** Content the client held before submission is the client's;
  nothing OML erases reaches those copies.
- **Lead-compromise containment.** The compromised **lead** holds the signing key and KMS
  decrypt: it can unwrap any served tenant's DEKs, read every body, and serve fabricated
  content under a valid signature. Compromise means decrypt-everything and sign-everything.
  A compromised **follower** is narrower by construction ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)):
  serving-path decryption and misserving only — it cannot forge a chain event, promote or
  erase, so tamper-evidence survives follower compromise. In the default lead-only topology
  the lead admission stands alone. In the standalone profile the admission widens to the host:
  one machine holds signing key, KEK, database, lake and backups.
  [Security](../architecture/security.md).
- **Indexer-compromise containment.** `oml-indexer` necessarily handles plaintext; its
  compromise exposes the promoted plaintext of every tenant it indexes. And until
  [RS-1](roadmap.md) closes, there is no commitment that plaintext never leaves the
  deployment. [Retrieval](../architecture/retrieval.md).
- **Tamper-evidence against full compromise, beyond retained evidence.** Restating the §2
  bound as the admission it is: until [RS-9](roadmap.md) decides an anchoring mechanism,
  detection of a key-holding attacker rests on evidence someone outside the deployment chose
  to keep.
- **Availability under integrity stress.** OML trades availability for integrity at four
  named points — fail-closed review, quorum-synchronous commit, singleton lake-writer, and
  the single lead — a stalled reviewer, standby, lease, or lead takeover each stops writes
  rather than weakening them. During a lead takeover, followers keep serving reads; writes
  are refused for the lease-and-fork-check window ([FR-24](roadmap.md)).
  [Admission](../architecture/admission.md), [storage](../architecture/storage.md),
  [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md).
