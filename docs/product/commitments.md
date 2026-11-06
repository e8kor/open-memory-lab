<!--
id: product/commitments
tier: client
status: current
answers: every guarantee Open Memory Lab makes, sorted into four honesty tiers, each with its enforcement point and the assumption it rests on
-->

# What OML guarantees

Every guarantee OML makes appears here, in four tiers: guaranteed by construction, guaranteed
within a stated bound, provided but unmeasured, and never claimed. A claim that is not on this
page is not a guarantee, whatever a sentence elsewhere may suggest. Each entry names its
enforcement point and the assumption it rests on, and each bound links the
[open issue](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap) that measures it.
"By construction" means the code makes violation impossible, such as a verb that does not exist;
the assumptions column says what that depends on.

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
| C8 | Simulation taint never takes the default tip. [Simulations](https://github.com/e8kor/open-memory-lab/blob/main/design/simulations.md), [retrieval](../architecture/retrieval.md). | Audience-scoped tip computation, `simulation.tip_refused`, per-simulation indexes. | Declared derivation only. |
| C9 | No held body reaches any lake zone. [Storage](../architecture/storage.md). | Bronze mirrors digest-only payloads — digests are all the outbox carries; silver is enqueued by promotion only. | Nothing — there is no datum to leak. |
| C10 | Chain verification survives erasure. [ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md). | Signed envelopes carry digests, never bodies. | Pure construction. |
| C11 | No client key exceeds its role's verb ceiling (R1). [Roles and keys](../architecture/roles-and-keys.md). | Server-side per-role check; promote, sign and delete appear in no client verb column; every transport is an adapter over `oml-app`. | The adapter rule of [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d3. |
| C12 | `self_signed` rows never leave their workarea and never enter silver or gold. [Workareas](../architecture/workareas.md), [delta tables](../data/delta-tables.md). | An indelible schema mark; the lake exclusion is checkable by the [reconciler](https://github.com/e8kor/open-memory-lab/blob/main/design/reconciliation.md). | Reconciler coverage. |
| C13 | A review verdict counts once, for exactly what was served. [Admission](../architecture/admission.md), [ADR-0012](../adr/ADR-0012-admission-and-review.md). | Server-side single-use serve binding to (reviewer key, artifact version, digest set). | Server-side state; no client replay path exists. |
| C14 | A promoted term's CURIE is permanent (never re-mintable with a different meaning), the `is_a`/`replaced_by` graphs are acyclic, and simulations cannot mint vocabulary. [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md). | The minted-at-first-promotion rule and the cycle refusal are `oml-domain` invariants (like the tip CAS); the simulation veto is a deterministic scan-class refusal. | `oml-domain` encoding the invariants (ADR-0019 d1); humanness of a human-required verdict is an issuance-time **attestation**, never verified by the server. |

## 2. Guaranteed within stated bounds

- **Erasure, including backups.** Destruction is immediate on live tables; for copies held in
  backups, it completes no later than the stated backup-retention plus KEK-rotation window.
  [Audit and lineage](../architecture/audit-and-lineage.md),
  [ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md). The bound's actual size is
  unmeasured — #108 produces it.
- **Tamper-evidence, not tamper-proofing.** The chain detects rewriting by anyone who cannot
  sign. Against a key-holding attacker with database write access — who can regenerate a whole
  consistent chain — detection rests entirely on chain heads retained *outside* the deployment,
  and no mechanism yet exists that pushes a head outside; evidence export is on-demand only.
  #152 decides an anchoring mechanism. Until it closes, tamper-evidence against a
  full-compromise attacker is only as good as the exports tenants happened to retain (tier 4).
- **Exactly-once lake materialization.** Rests on delta-rs transaction-identifier behavior
  that #101's extended conformance test must verify — including that the
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
  and a crashed follower loses its unflushed async batch (#154).
  [Audit and lineage](../architecture/audit-and-lineage.md).
- **Revocation and grant latency.** A revoked key or withdrawn grant stops acting at the key's
  next request served by the lead: access is recomputed from the record on every request, and no
  cache holds an older answer. In cluster mode a follower checks access against its own copy of
  the record, so on the reads a follower serves, a revoked key or withdrawn grant keeps acting
  until replication catches up: the window is replication lag, exposed as a follower metric and
  never assumed zero. A lead-only deployment, the default, has no such window
  ([security](../architecture/security.md)).
- **Rate limiting.** In cluster mode the split follows the topology: write-path limits are
  exact (one lead — nothing to divide); read-path limits are limit/N across followers, so a
  tenant can briefly exceed its read quota and the error grows with follower churn.
  Lead-only deployments are exact on both paths. #133.
- **Vector-scan completeness.** Iterative vector scans are bounded by pgvector's
  `max_scan_tuples` and scan-memory caps — a configurable cutoff, not exhaustiveness. The
  recall consequences are #102's to measure.
  [Retrieval](../architecture/retrieval.md).
- **Ban scope.** A ban acts at admission only; content already visible needs its own demotion
  or erasure act. [Admission](../architecture/admission.md).

## 3. Provided, quality unmeasured

These capabilities exist in the design, and nothing yet says how well they perform or how the
open choice resolves. Each line is one [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap) row; the row's exit criterion
produces the number or the decision.

- Erasure of attribution metadata, end to end across every copy class — #146.
- Review throughput: what a deployment's reviewing crews clear per unit time — #98.
- Exactly-once lake application, as verified behavior rather than library assumption — #101
  (the bound in §2 carries it until the test exists).
- Search quality: recall, latency, exact-versus-approximate — #102.
- Decrypted per-audience export — #125.
- Bulk key rotation for crew-sized key sets — #120.
- The record store's scale ceiling and the partition strategy past it — #99.
- Rate-limit enforcement accuracy — #133 (bound in §2).
- Token-budget retrieval (`max_tokens` with deterministic truncation) — #106.
- Batch submit and generated SDKs — #122.
- Dataset onboarding, v1 slice ([ADR-0020](../adr/ADR-0020-dataset-onboarding.md)) — #148.
- Import throughput and the recall impact of indexed imports — #148.
- Admission scan latency, and the open attachment-pipeline points of
  [ADR-0004](../adr/ADR-0004-bodies-and-attachments.md) — #147.
- Lake-writer throughput and Delta commit latency — #101.
- Erasure-SLO bounds, measured — #108.
- Full-genesis reconciler runtime and quorum write latency — #99.
- The vocabulary v1 slice (prefixes, term lifecycle, releases, scanner, read surface) — #149.
- Vocabulary expansion recall, closure storage, mapping quality, and the per-vocabulary
  human-verdict load — #149.
- The `/v1` provider-ring compatibility covenant: dormant until the v1-stable declaration —
  #150 ([ADR-0022](../adr/ADR-0022-two-ring-api.md)).
- Profile conformance (one suite green on standalone, docker, kubernetes) — #117; the
  migration rehearsal — #117; lead-failover semantics and the single-lead write ceiling —
  #117; provider-ring latency against declared targets — #150
  ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md),
  [ADR-0024](../adr/ADR-0024-performance-foundation.md)).
- Claude's own apps over OAuth by connect code
  ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)): the connect flow green end
  to end and one live claude.ai connection — #115; the sign-in challenge on every refused
  request — #126; the key value a retried issuance keeps for a day — #127.
- Open questions, each a decision not yet taken: embedding model and boundary (#153),
  federation (#151), the Merkle specification (#148), sampling-policy semantics
  (#148), the connector surface (#148), MWK rotation (#148), tenant-wide erased-digest
  matching (#148), external chain-head anchoring (#152), the `oml:` upper-vocabulary register
  (#149), the external-ontology import profile (#148), the standalone keyring mechanism
  (#154), the cluster read-log split (#154), the DEK serving-cache shape (#154).

### Profile-dependent bounds

Several §2 bounds depend on the install profile ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md));
the per-profile detail lives with the [deployment](../operations/deployment.md) page's profile
sections and [security](../architecture/security.md)'s "Custody by profile" table, and the
rule here is the contract: **a profile never changes what the system refuses — it changes what
backs the refusal.** Durability on standalone and default docker is one host's disk (quorum
restores the claim only against a quorum-replicated Postgres); KEK custody in the `kms-file`
profiles is a file on the host, so "never leaves KMS" is replaced, not weakened; the erasure SLO
gains the keyring-file copy class; and tamper-evidence against the host administrator in
standalone rests entirely on exported chain heads (#152). The constitution-tier rows of §1 are
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
  #153 closes, there is no commitment that plaintext never leaves the
  deployment. [Retrieval](../architecture/retrieval.md).
- **Tamper-evidence against full compromise, beyond retained evidence.** Restating the §2
  bound as the admission it is: until #152 decides an anchoring mechanism,
  detection of a key-holding attacker rests on evidence someone outside the deployment chose
  to keep.
- **Availability under integrity stress.** OML trades availability for integrity at four
  named points — fail-closed review, quorum-synchronous commit, singleton lake-writer, and
  the single lead — a stalled reviewer, standby, lease, or lead takeover each stops writes
  rather than weakening them. During a lead takeover, followers keep serving reads; writes
  are refused for the lease-and-fork-check window (#117).
  [Admission](../architecture/admission.md), [storage](../architecture/storage.md),
  [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md).
