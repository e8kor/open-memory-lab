<!--
id: adr/ADR-0005-erasure-by-crypto-shredding
tier: engineering
status: accepted design
answers: how an append-only chain coexists with the right to erasure; what crypto-shredding destroys, what the erasure inventory scopes, and where the honest boundary of the claim lies
-->

# ADR-0005 — Erasure by crypto-shredding, with a named inventory and an honest boundary

**Status:** accepted design · 2026-10-29

## Context

An append-only, hash-chained journal and a right to erasure look irreconcilable. One move
reconciles them: decisions are immutable, content is erasable. The chain stores commitments;
bodies are encrypted; erasure destroys the key as a recorded event, and the chain verifies
afterwards.

The mechanism is the easy half. The hard half is the completeness claim, where designs of this
kind habitually overclaim — "never personal data", "no missed replicas", "no race with caches", a
backup posture from which a key can never revive. Each of those is falsified by one
counterexample: a retained usable key backup or a plaintext cache defeats the erase;
cryptographic erase requires verified sanitization of all usable copies of the target key; signed
identifying metadata is not non-personal just because adjacent content is hashed; subject-handle
discovery is incomplete; and the copy inventory spans every controlled deployment and projection,
not a one-store boundary. The honest repair for all of them is the same: an inventory.

So the inventory is built into the design from day one — every copy class a body, its
derivatives or its usable key copies can land in is enumerated below with its disposition — and the guarantee is stated
with its boundary in the same sentence. Everything here is decided, not built; the inventory is a
commitment about the design, enforced by nothing until the conformance work the
[roadmap](../product/roadmap.md) tracks exists.

## Decision

- **d1 — Bodies and attachments are encrypted under per-artifact DEKs, wrapped by a KEK.** Every
  body and every attachment is ciphertext at rest, everywhere outside the serving path
  ([ADR-0004](ADR-0004-bodies-and-attachments.md) d4). The DEK is per artifact — one key, one
  memory — wrapped by a tenant-scoped KEK held in KMS or a Kubernetes Secret per deployment
  profile. Wrapped DEKs live in `artifact_keys` (the
  [Postgres schema](../data/postgres-schema.md)); key custody is the
  [security page](../architecture/security.md)'s subject: only the serving path decrypts, the
  lake-writer's KMS grant is encrypt-only, the reconciler holds no keys.

- **d2 — Erasure is a journalled erase event plus DEK destruction.** The erase event enters the
  per-tenant hash chain like any other event; the wrapped DEK is then destroyed
  (`artifact_keys.destroyed_at`). Nothing is silent and nothing rewrites history: ciphertext and
  every digest reference to it survive, and the chain verifies end to end after the erase,
  because journal and outbox payloads were digest-only from the start
  ([ADR-0004](ADR-0004-bodies-and-attachments.md) d1). An erased artifact's state is `erased` — a
  recorded fact, served as absence with provenance, never as an error.

- **d3 — The guarantee is scoped to body and attachment bytes, and the scope is stated wherever
  the guarantee is.** What DEK destruction makes unreadable is the plaintext of the body and the
  attachments. It does not erase the fact that a memory existed, who wrote it, when, or where —
  that is d5's boundary. No page of this documentation states the guarantee without its scope.

- **d4 — The erasure inventory enumerates every copy class and its disposition, and the
  inventory is normative.** A copy class missing from this table is a design defect, not an
  operational surprise. Dispositions as designed:

  | Copy class | Holds | Disposition at erase |
  |---|---|---|
  | `journal.payload` / `signed_bytes` | digests and ciphertext references only ([ADR-0004](ADR-0004-bodies-and-attachments.md) d1) | never needs touching; chain verifies unchanged |
  | `outbox.payload` | same digest-only rule | never needs touching |
  | `bronze.events`, including all Delta versions and checkpoints | digest-only mirror of journal events | never needs touching |
  | `silver`/`gold` body columns | per-artifact-DEK ciphertext | unreadable at DEK destruction; superseded parquet versions and checkpoints keep the ciphertext until VACUUM and checkpoint cleanup run — that retention bound is part of the erasure SLO, not an afterthought |
  | object-store versions (`oml-bodies`, quarantine) | ciphertext | unreadable at DEK destruction; versioned copies stay ciphertext |
  | pgvector rows, tsvector entries | **plaintext-derived** (embeddings, token lists) | removed in the same transaction as the erase event; persist in Postgres base backups and WAL until the named backup retention window expires — that is the honest statement, and the window is part of the erasure SLO |
  | Postgres temp/spill files and standby replicas' disks | transient plaintext-derived content; every standby holds the primary's WAL and table copies | bounded by at-rest volume encryption and OS temp-file lifetime; standbys inherit every bound in this table |
  | OS/block-level volume snapshots of the Postgres volume | everything the volume held at snapshot time, wrapped DEKs included, outside the base-backup accounting | if the deployment takes them, their retention window joins the SLO composition exactly as base backups do, and the deployment backup table lists them |
  | `artifact_keys.wrapped_dek` in Postgres base backups and WAL | **a usable copy of the wrapped DEK** — key material, not content | live row nulled at erase; copies in base backups and WAL survive the backup retention window, and while the wrapping KEK lives a restored backup plus KMS unwrap could recover the DEK and decrypt every surviving ciphertext copy. The designed answer: scheduled KEK rotation re-wraps **live** DEKs only, and a retired KEK is destroyed once the backup horizon has passed, invalidating every wrapped-DEK copy in older backups — erasure completes against live stores immediately and against backups no later than **backup retention window + KEK rotation period** |
  | `read_events` | query digests, memory ids | retention-bounded by tenant policy (owned by the [retrieval](../architecture/retrieval.md) and [audit](../architecture/audit-and-lineage.md) pages); in this inventory because it outlives the body |
  | caches (serving-path decrypts, CLI output, client-side copies inside OML's control) | plaintext | bounded TTLs inside the deployment; anything outside the deployment's control is outside the claim, and the documentation says so |

  Two consequences of the table are worth stating plainly. First, erasure is not instantaneous
  across every class: the design's erasure claim is "unreadable now on every serving path;
  physically gone from derived and backup copies, and unlockable by no surviving key copy, within
  the named retention bounds", and the bounds (VACUUM/checkpoint cadence, backup retention
  window, KEK rotation period) are part of the erasure SLO — all
  unmeasured until built, tracked as FR-17 on the [roadmap](../product/roadmap.md). Second, the claim
  spans exactly the copies OML controls; a client that read a body before erasure holds what it
  holds.

- **d5 — Attribution metadata is not erased by crypto-shredding, and this boundary is stated,
  not hidden.** Writer identity, principal, timestamps and workarea path remain in the journal,
  the metadata tables and the lake after an erase — they are what makes the erase event itself
  auditable, and identity metadata in the chain is minimized at write time for exactly this
  reason ([storage](../architecture/storage.md)). Erasure or pseudonymization of attribution
  metadata is a tracked FR with an exit criterion on the [roadmap](../product/roadmap.md), not a
  property this record claims. Signed identifying metadata must be minimized or individually
  erasable, never assumed non-personal.

- **d6 — The sanctioned language.** No sentence anywhere in this documentation reads as
  "GDPR-compliant", "audit-ready" or any regulator's verdict rendered in advance. The sanctioned
  forms are: "produces the evidence an auditor asks for", and "a journalled erase event whose
  effect on each copy class is listed here". A regulation is cited only by article, from its
  primary text, with a fetch date — and this record cites none, deliberately: it describes a
  mechanism and its boundary, not a legal conclusion.

## Consequences

- Erasure and the append-only journal stop being a contradiction the design carries: the chain
  never held plaintext, so destroying a DEK breaks nothing it attests. The price was paid
  upstream, in [ADR-0004](ADR-0004-bodies-and-attachments.md) d1 — the journal alone can never
  answer what a memory said.
- The design owes an erasure SLO with three named bounds (lake VACUUM/checkpoint cadence,
  Postgres backup retention, KEK rotation period) before the erasure story is complete; all are
  roadmap rows with exit criteria, and until they are measured the honest sentence is "unreadable
  immediately, physically gone — and unlockable from any surviving key copy — within bounds we
  have not yet measured".
- A genuine loss of audit power is accepted: after an erase, the journal proves a decision was
  made over a commitment that can no longer be opened — not what the content was.
- Key management becomes load-bearing infrastructure: provable DEK destruction, a KEK whose
  compromise is a tenant-wide event, and a backup posture in which a restored database never
  serves from a resurrected wrapped key. The posture is now designed in two halves: the
  fenced-restore ceremony ([deployment](../operations/deployment.md)) re-applies erase events
  newer than the backup before readiness, and the KEK rotation/retirement schedule (d4) bounds
  how long the backup bytes themselves hold a usable key copy. Each is design work the
  [security page](../architecture/security.md) owns; none is built, and the end-to-end
  demonstration rides the [roadmap](../product/roadmap.md)'s FR-1 erasure row, whose exit
  criterion already demands unrecoverability from every copy class in this inventory — the
  inventory d4 just widened to include the key-copy class.
- Multi-subject content erases wholly: the key is per artifact, so an artifact mentioning two
  people erases for both when either asks. Over-erasure is accepted and recorded, because the
  alternative — subject-level keying of free text — promises a precision the admission path
  cannot deliver.

## Rejected alternatives

- **Plaintext bodies with physical deletion.** Physical delete in this architecture means
  rewriting an append-only lake: Delta retains superseded parquet files and checkpoints by
  design, delta-rs reads deletion vectors but does not write them (per the stack record's
  capability rule), and the journal's hash chain makes any retroactive rewrite indistinguishable
  from tampering. Deletion would also have to chase every backup and replica of plaintext — a
  completeness claim that cannot be verified. Encrypt first, destroy the key once.
- **Per-subject keys.** Free text has no reliable subject set — discovery is incomplete, a
  memory mentions several people, and keying requires knowing the subjects at write time, which
  the admission path cannot promise. Keys are per artifact, with subjects as a rebuildable index
  beside them, never as the keying unit.
- **Tombstone-only erasure** (record the deletion, stop surfacing the row, keep the plaintext).
  The legally and architecturally weakest option:
  suppression from query results is not erasure, and restricting access is a fallback where
  deletion is impossible — which, with per-artifact encryption, it is not. OML keeps tombstoning
  only in its honest role: demotion, which changes visibility and keeps bytes, is a different
  verb with a different promise, and the two are never conflated.
- **Deleting journal entries.** Breaks the chain, the fold and the entire evidentiary value of
  the system; erasure would become indistinguishable from tampering.
- **Anonymizing content in place.** Robust anonymization of free text is unsolved, and
  pseudonymization is not anonymization; the design would be claiming a property it cannot
  deliver — the exact failure class d6's language rules exist to prevent.
