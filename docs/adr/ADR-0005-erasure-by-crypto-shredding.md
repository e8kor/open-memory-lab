<!--
id: adr/ADR-0005-erasure-by-crypto-shredding
tier: engineering
status: accepted design
answers: how an append-only chain coexists with the right to erasure; what crypto-shredding destroys, what the erasure inventory scopes, and where the honest boundary of the claim lies
-->

# ADR-0005 — Erasure by crypto-shredding, with a named inventory and an honest boundary

**Status:** accepted design · 2026-10-29 · amended by [ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) · amended by [ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md) (d3, d4)

**Release:** not built ([releases](../changelog.md))

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
[open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap) tracks exists.

## Decision

- **d1 — Bodies and attachments are encrypted under per-artifact DEKs, wrapped by a KEK.** Every
  body and every attachment is ciphertext at rest, everywhere outside the serving path
  ([ADR-0004](ADR-0004-bodies-and-attachments.md) d4). The DEK is per artifact — one key, one
  memory — wrapped by a tenant-scoped KEK held in KMS or a Kubernetes Secret per deployment
  profile. Wrapped DEKs live in `artifact_keys` (the
  [Postgres schema](https://github.com/e8kor/open-memory-lab/tree/main/migrations)); key custody is the
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
  that is d5's boundary. No page of this documentation states the guarantee without its scope. *(Amended 2026-11-14 by [ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md) d7: clear text inside signed payloads is outside the guarantee and is kept as signed metadata: today a version's `title`, a verdict's `reason`, a workarea's or key's `name`, an anchor's `uri`, `path` and `fragment`, a crew's `charter_uri` and `external_manager`, and, once ADR-0089 is built, an `admin.fix` reason; the title is read by the admission scan at write and the reason by the veto scan. A digest with a sealed part for titles is #389.)*

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
  | `search_postings` (kind 1 lexeme ids, kind 2 learned sparse weights, the model's expansions included), `search_term_rows` ([ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d7) | **plaintext-derived** (hashed lexemes, model token ids and weights) | deleted in the erase's transaction by the postings trigger, with the version's `search_sparse_queue` entry (ids, times and a failure reason, no text); in base backups and WAL until the retention window ends, as tsvector entries |
  | the widen hint's signals, in the lead's memory ([ADR-0072](ADR-0072-recall-searches-the-trusted-workspaces-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md), as built by #244) | **plaintext-derived** (per-workarea counts of hashed lexemes, read from `search_term_rows`) | dropped for the whole tenant when the erase commits, in the lead process that commits it, and a build already reading is discarded when it ends; the erase must commit through `App::commit_requests` for this to hold; never written to disk, gone at a restart, and otherwise at most 300 s old |
  | Postgres temp/spill files and standby replicas' disks | transient plaintext-derived content; every standby holds the primary's WAL and table copies | bounded by at-rest volume encryption and OS temp-file lifetime; standbys inherit every bound in this table |
  | OS/block-level volume snapshots of the Postgres volume | everything the volume held at snapshot time, wrapped DEKs included, outside the base-backup accounting | if the deployment takes them, their retention window joins the SLO composition exactly as base backups do, and the deployment backup table lists them |
  | `artifact_keys.wrapped_dek` in Postgres base backups and WAL | **a usable copy of the wrapped DEK** — key material, not content | live row nulled at erase; copies in base backups and WAL survive the backup retention window, and while the wrapping KEK lives a restored backup plus KMS unwrap could recover the DEK and decrypt every surviving ciphertext copy. The designed answer: scheduled KEK rotation re-wraps **live** DEKs only, and a retired KEK is destroyed once the backup horizon has passed, invalidating every wrapped-DEK copy in older backups — erasure completes against live stores immediately and against backups no later than **backup retention window + KEK rotation period** |
  | `lake_staging` ([ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md) d3) | **plaintext fold inputs** of a commit not yet drained to the lake: bodies, rules, key hashes, addresses, findings, idempotency outcomes | the erase's transaction replaces the version's staged rows with tombstones (the part, `erased: true`, no bytes), so the bytes leave staging at once; in base backups and WAL until the backup window ends, as `memories` is |
  | `twin/inputs`, all Delta versions ([ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md) d2) | per-key ciphertext of the same inputs, held bodies included | never touched; unreadable once the key is destroyed |
  | `twin/keys`, earlier Delta versions ([ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md) d2) | **a usable wrapped copy** of a destroyed data key | current version destroyed at erase, and a rebuild destroys every key whose erase it replays; earlier versions until VACUUM at the lake erasure bound; a surviving copy dies with the KEK version rotation retires |
  | `read_events` | query digests, memory ids | retention-bounded by tenant policy (owned by the [retrieval](../architecture/retrieval.md) and [audit](../architecture/audit-and-lineage.md) pages); in this inventory because it outlives the body |
  | `recall_events.query_text` | **plaintext a reader wrote**, which may quote erased content; stored only where the tenant opted in ([ADR-0054](ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d3) | erase calls `oml_telemetry_erase`, which nulls the text of every event that served an erased version, under the tenant's row security; every row is purged 30 days after its recall in any case; in base backups and WAL until the backup window ends, as tsvector entries are (ADR-0054 d4) |
  | `recall_events.hit_ids` / `hit_scores` / `query_digest`, `recall_outcomes` | version ids, scores, query digests and labels: metadata, but a short query's digest inverts by word list | the same call zeroes the query digest of every event that served an erased version, removes the version's id and score and deletes its labels; the 30-day purge bounds every row (ADR-0054 d2, d4) |
  | caches (serving-path decrypts, CLI output, client-side copies inside OML's control) | plaintext | bounded TTLs inside the deployment; anything outside the deployment's control is outside the claim, and the documentation says so |

  Two consequences of the table are worth stating plainly. First, erasure is not instantaneous
  across every class: the design's erasure claim is "unreadable now on every serving path;
  physically gone from derived and backup copies, and unlockable by no surviving key copy, within
  the named retention bounds", and the bounds (VACUUM/checkpoint cadence, backup retention
  window, KEK rotation period) are part of the erasure SLO — all
  unmeasured until built, tracked as #108 on the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap). Second, the claim
  spans exactly the copies OML controls; a client that read a body before erasure holds what it
  holds.

- **d5 — Attribution metadata is not erased by crypto-shredding, and this boundary is stated,
  not hidden.** Writer identity, principal, timestamps and workarea path remain in the journal,
  the metadata tables and the lake after an erase — they are what makes the erase event itself
  auditable, and identity metadata in the chain is minimized at write time for exactly this
  reason ([storage](../architecture/storage.md)). Erasure or pseudonymization of attribution
  metadata is a tracked issue with an exit criterion on the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap), not a
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
  issues with exit criteria, and until they are measured the honest sentence is "unreadable
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
  demonstration rides the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap)'s #146 erasure row, whose exit
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
