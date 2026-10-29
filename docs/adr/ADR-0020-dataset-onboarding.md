# ADR-0020 — Dataset onboarding: the import manifest

**Status:** accepted design · 2026-10-29 · explicitly post-v1 ([critical path](../product/critical-path.md) M5)

Everything this record decides is design — no code exists, and nothing here is on the v1 build.
It is recorded now because a few of its reservations are cheap in v1 and expensive later; those
reservations are listed in the consequences.

## Context

OML must be able to map big-data datasets — a Delta table, a set of Parquet or CSV files, a
warehouse query at a point in time — into the provider, as governed memory. The requirement
collides with the register at two points:

- **Per-row review cannot scale.** No reviewer can be served ten million rows; serving them
  would turn the gate of [ADR-0012](ADR-0012-admission-and-review.md) into rubber-stamping or a
  refusal to import at all.
- **Per-row chain events cannot scale.** Ten million journal events per import is write
  amplification that the per-tenant chain of
  [ADR-0002](ADR-0002-postgresql-system-of-record.md) was never priced for.

The resolution is a change of **grain**: the reviewed claim becomes the dataset snapshot, not
the row. [ADR-0001](ADR-0001-the-constitution.md) fixed the rules R1–R4 but never fixed the
grain of the unit they apply to — so this record changes the grain and amends nothing. At
manifest grain, every rule holds: R1 — the manifest is submitted by a writer and ratified by a
dreamer; R2 — the manifest's author and lineage closure exclude its reviewers; R3 — nothing an
import carries is visible to anyone before the manifest is ratified; R4 — mapping failures,
partial loads and refusals are journalled, never silent.

## Decision

- **d1 — The import manifest is the claim.** The admission unit is the dataset snapshot. A new
  artifact kind, `dataset_manifest`, passes the existing pipeline unchanged: capture,
  quarantine, deterministic scan, non-author review, ratification, journal, release
  ([admission](../architecture/admission.md)). The manifest body is normative and carries: the
  source locator with a snapshot pin (Delta version, Parquet digest set, CSV digest, or
  warehouse query plus as-of); a deterministic, versioned schema-mapping spec — columns to
  `claim_key` (unique within the snapshot), body, attribution/producer, event-time — whose
  digest rides the signed envelope; the transform's plugin identity and version
  ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md)); the row count, mapping-failure
  count and failure-report digest; **two Merkle roots** — per-row plaintext digests and per-row
  ciphertext digests, in a fixed leaf order; and declared provenance and licensing, in the same
  honesty class as `derives_from`. The reviewer attests three things: that the mapping spec is
  correct for this source (deterministic, so the digests are re-derivable), that the
  source-authority and licensing declaration is plausible and complete *as a declaration*, and
  their findings over the journalled sample (d3). Never row truth. The evidence export phrases
  every such verdict as "admitted under this process".

- **d2 — Rows are manifest-subordinate claims: one chain event per import.** The chain carries
  `import.manifest_submitted` (both roots under the signature), the review family on the
  manifest, and `import.completed`. Per-row facts live in a root-anchored side table,
  `import_rows` (`tenant_id`, `manifest_id`, `row_index`, `artifact_id`, `claim_key`,
  `plaintext_digest`, `ciphertext_digest`, `event_time`, `body_ref`) — outside the chain but
  reconcilable against it: the [reconciler](../architecture/reconciliation.md) recomputes the
  Merkle root over `import_rows` and compares it to the signed root, the same pattern
  `read_events` and `system.readlog_anchor` already use. `artifact_id` is UUIDv5 over
  (`manifest_id` + row plaintext digest), so retry and re-import are idempotent. Per-row
  journal events exist only for post-admission individual acts — demote, erase, individual
  re-review, organic supersession; the bulk case is dataset-level erasure (d7). Rows fold into
  `memories` as ordinary rows: `provenance='connector'`, a new nullable `manifest_id` column,
  state held until the manifest promotes; the fold's source is the manifest event plus
  `import_rows`, and the schema page's folds section says so.

- **d3 — Per-row review is a journalled, seeded sampling policy.** The review policy object
  ([ADR-0012](ADR-0012-admission-and-review.md) d3) gains `sample_rate` (or a fixed `n`),
  `seed`, `max_failure_rate`, and optional `human_required_classes` triggers for licensing and
  provenance classes. The seed is **server-assigned at serve time**, after the roots are fixed,
  so a submitter cannot pre-game which rows the sample will hit. Sampling is deterministic from
  the seed over the Merkle leaf order, so an auditor can reproduce exactly which rows were
  sampled. The serve binding extends: `review.serve` binds (reviewer key, manifest version,
  seed, sampled row digest set); the server decrypts exactly the sampled rows; the verdict
  records the served digests. The statistical honesty is stated at the policy itself: a defect
  affecting a fraction *p* of rows escapes a sample of *n* with probability (1−p)^n — sampling
  detects systematic defects and structurally cannot detect rare poisoned rows.

- **d4 — One dataset is one dedicated workarea, and imported rows carry an indelible
  `review_scope` mark.** Import creates the workarea if absent, under any non-frozen parent:
  `claim_key` namespacing prevents collisions, dataset-level demote/erase/validate become
  workarea-scoped acts, and recall scoping falls out of the existing subtree filter. The ladder
  ([workareas](../architecture/workareas.md)) is untouched. Imported data *can* reach
  `validated` by ordinary propose-and-ratify, but every imported row carries `review_scope` —
  `sampled(manifest_id)` versus `individual` — in Postgres, silver and curated gold, analogous
  to `self_signed` but weaker: it narrows what the trail attests, not where the row may go.
  "Validated" then honestly means "a frozen governed set whose admission trail is: manifest
  ratified under policy P, rate r, seed s" — and the evidence export says exactly that. A row
  sheds the mark only by passing the normal pipeline individually — a journalled per-row
  serve, verdict and promote referencing the manifest. Escalation is available row by row,
  never required.

- **d5 — A new `import` event family.** `import.manifest_submitted` (roots, counts,
  mapping-spec digest, snapshot pin, scan result); the `review.*` family reused with
  `subject_kind: "dataset_manifest"`; `import.completed` (actually-loaded count and recomputed
  roots — the attested "what landed"); `import.superseded`; `import.erased` (the dataset-level
  shred of d7). Bronze mirrors these as single rows; imported row data reaches the lake through
  silver materialization, not the event mirror — the digest-only bronze invariant of
  [storage](../architecture/storage.md) is untouched.

- **d6 — No second bulk lake loader.** The keyless singleton writer of
  [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) is load-bearing — erasure inventory,
  reconciliation, conditional-put fencing all assume one writer — so imports ride it. Promotion
  of a manifest enqueues **one** outbox row, kind `import-materialize`, carrying `manifest_id`
  and never row payloads. The lake-writer reads `import_rows` under per-tenant RLS and
  bulk-appends to `silver.claims` in **chunked** Delta commits under a per-manifest appId
  (`oml-lakewriter:import:<manifest_id>`, version = chunk index) for internal idempotence; the
  ordinary per-table appId/version commits only on the final chunk, so the single-stream
  protocol sees the import as one atomic outbox operation and recovery resumes at chunk *k*.
  Bodies move from quarantine to lake as ciphertext, verbatim (encryption happens at capture,
  d7): the keyless writer stays keyless. **Head-of-line blocking is the honest cost**: a
  ten-million-row materialization stalls every retract behind it. It is bounded, not hidden, by
  a tenant-configurable inline budget (rows/bytes) above which the import is scheduled into the
  maintenance window; the stale-frontier report shows the lag. A second drain lane is the
  escape hatch only if measurement finds the bound unacceptable — it needs per-artifact `seq`
  fencing against retract-before-insert, which is why it is deliberately not first.

- **d7 — A per-manifest wrapping-key tier, and encryption at capture.** Each row gets a random
  DEK, wrapped locally (in-process AES, no per-row KMS round-trips) under a per-manifest
  wrapping key (MWK); the MWK is wrapped by the tenant KEK in **one** KMS call. `artifact_keys`
  rows carry `kek_id` = the MWK. Per-row erasure: null the row's `wrapped_dek` — the DEK is
  random and exists nowhere else. Dataset-level erasure: destroy the MWK (`import.erased`) — a
  bulk-erase verb gained for free. Encryption and digesting happen **at capture, before
  review**: the runner (inside `oml-server`, the single key-holding locus) streams the pinned
  snapshot, maps, encrypts rows into quarantine chunk objects
  (`oml-quarantine/<tenant>/import/<manifest>/chunk-*`, rows packed with offsets; `body_ref` =
  `{class: import_chunk, chunk_digest, offset, len}`,
  [ADR-0004](ADR-0004-bodies-and-attachments.md)), and computes both Merkle roots — so both
  digests exist when `import.manifest_submitted` is signed, preserving the ciphertext-comparand
  rule of [storage](../architecture/storage.md). The reconciler verifies chunk bytes and
  per-row ciphertext digests keylessly. `artifact_keys` gains on the order of ten million rows
  per import — a Postgres volume fact [FR-7](../product/roadmap.md) must carry, stated, not
  solved.

- **d8 — Lineage is structural at row grain, ordinary at dataset grain.**
  `provenance='connector'` already exists in the `memories` CHECK and in silver — the schema
  anticipated this record; the event catalog did not, and d5 fills that gap. Row-to-manifest
  lineage is the structural `manifest_id` column (the `simulation_id` pattern), **not** ten
  million `claim_edges` rows. Dataset-level lineage is ordinary: a manifest's `derives_from`
  names prior manifests and source-describing claims; manifest versions share a `claim_key`
  (the dataset's identity) and supersede normally. The R2 extension: the manifest's closure
  stands in for its rows' — a reviewer in the manifest's lineage cannot review the manifest,
  and imported rows contribute the manifest, not ten million entries, to any downstream
  closure of derivations.

- **d9 — Re-import is manifest supersession, with a resurrection fence.** Two modes:
  full-replace (the new manifest promotes, the prior manifest's rows demote — one journalled
  act, one lake MERGE on `manifest_id`) and delta (a mapping key column drives a deterministic
  diff; changed rows supersede their claim-key tips, removed rows demote; the roots cover only
  the delta). Row-level tip CAS happens in bulk inside the promotion transaction; no per-row
  chain events. The **resurrection fence**: an erased row still exists in the external source,
  and a naive re-import would silently re-admit it. Every manifest supersession therefore
  carries the **exclusion set** — the plaintext digests of rows individually erased from prior
  manifest versions; capture deterministically refuses matching rows; the exclusion set is
  journalled with the manifest. Digests survive erasure, which is legal under
  [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md)'s stated boundary; the fence enters that
  record's inventory discussion as a named rule.

- **d10 — Mapping failures are counted, attached, and vetoed over a threshold.** Excluded rows
  are counted and the failure report is stored as a manifest attachment under the ordinary
  attachment rules, its digest in the signed event. `max_failure_rate` is enforced at capture
  as a deterministic scan-class veto: above the threshold, the whole import is refused with a
  typed error and a journalled rejection. Partial semantics are all-or-nothing at capture
  against the threshold; after ratification, `import.completed` attests actually-loaded against
  declared, and a shortfall is a reconciler `missing` finding.

- **d11 — Imported rows are not indexed by default.** The default is fetch- and
  filter-serving, not the shared FTS and vector indexes; a manifest's `index_policy`
  (`none | fts | fts+vector`) opts in, journalled and priced. A ten-million-row index flood is
  both a build cost (embedding at scale — the RS-1 boundary) and a recall-quality cost
  (reference data drowning organic memories in [retrieval](../architecture/retrieval.md)).
  Both are unmeasured — [FR-14](../product/roadmap.md).

- **d12 — Simulation imports are allowed, with the provenance compromise stated.** A manifest
  and its rows may take a `simulation_id`; rows then take `provenance='simulation'`, so every
  existing fence applies unchanged — per-simulation indexes, audience-scoped reads, overlay
  tips, end-of-simulation demotion as one manifest-scoped MERGE. The compromise is honest: the
  provenance column says simulation, and the external-source fact lives on the manifest. The
  alternative (`connector` plus `simulation_id`) would force rewriting the read-fence predicate
  on every read path — containment-critical code — and is not worth a label.

- **d13 — Export is the mirror image.** `gold.curated_<workarea>` already is "a validated
  workarea as a stable Delta dataset", in ciphertext; the readable version is exactly
  [FR-5](../product/roadmap.md). This record adds one requirement: exports emit a
  manifest-shaped descriptor — snapshot pin, row count, Merkle roots — so that an OML export
  can be re-imported elsewhere verifiably. Import and export are symmetric.

## Consequences

- **v1 reservations, cheap now and expensive later** — these are the only parts of this record
  on the v1 build, and they are already reserved in the
  [schema](../data/postgres-schema.md) and [event catalog](../data/event-catalog.md) pages:
  nullable `memories.manifest_id` and `review_scope` columns, the `import-materialize` outbox
  kind in the CHECK, and the `import.*` event namespace. This also closes an existing
  inconsistency: `provenance='connector'` sat in the `memories` CHECK with no event family in
  the catalog.
- **The honesty list.** Eight admissions, none optional:
    1. No one ever attests row truth — sampling detects systematic defects, not adversarial
       needles.
    2. Source authority is a declaration; falsified provenance is review-rejectable, not
       machine-detectable.
    3. Erasure never reaches the external source. With the fence it is durable against *this
       dataset's* re-imports only — the same bytes arriving under a different manifest are a
       new admission, unless tenant-wide erased-digest matching is adopted
       ([RS-8](../product/roadmap.md)).
    4. The chain is O(1) per import, but Postgres is not — on the order of ten million rows
       across three tables per import; [FR-7](../product/roadmap.md) becomes load-bearing.
    5. Head-of-line blocking is bounded, not eliminated.
    6. Fail-closed at dataset scale: an unreviewed manifest holds millions of encrypted rows
       in quarantine until expiry — the occupancy is priced, not hidden.
    7. Attribution attests the importer; source-side authorship is mapped data.
    8. Bulk chunked exactly-once inherits [FR-3](../product/roadmap.md)'s unverified delta-rs
       assumption, now with a second appId scheme to conformance-test.
- **The smallest useful slice** ([critical path](../product/critical-path.md) M5): pinned-file
  sources only (Parquet/CSV by digest), full-replace only, the exclusion fence reduced to
  refusing re-import of erased digests, one dataset workarea per import under `committed`, no
  FTS or vector indexing, no simulation targeting, a hard 1M-row cap under the inline lake
  budget. The slice exercises every structural decision at a scale where the unmeasured
  foundations (FR-2, FR-3, FR-7) are least likely to collapse; everything after is relaxing
  caps, not redesigning.
- **Roadmap rows opened**: [FR-13](../product/roadmap.md) (the v1 slice end to end),
  [FR-14](../product/roadmap.md) (import throughput; indexed-import recall impact), and the
  open questions RS-4 (Merkle specification), RS-5 (sampling-policy semantics), RS-6 (the
  connector surface), RS-7 (MWK rotation against KEK rotation and backup decay), RS-8
  (tenant-wide erased-digest matching).

## Rejected alternatives

- **Per-row review.** No reviewer can be served ten million rows; the gate would collapse into
  rubber-stamping or refuse imports outright. The seeded, journalled sampling policy keeps the
  attestation honest about exactly what it covers, with the escape probability stated.
- **Per-row chain events.** Ten million events per import is write amplification the journal
  was never priced for. The root-anchored side table keeps every per-row fact auditable —
  the reconciler recomputes the root — at one chain event per import, on the pattern the read
  log already set.
- **A second bulk lake loader.** The keyless singleton writer is load-bearing for the erasure
  inventory, reconciliation and conditional-put fencing; a second writer forfeits all three to
  save one queue.
- **HKDF-derived per-row keys from a master.** Erasing one row would force destroying the
  master and re-encrypting everything else; per-row erasure dies. Random DEKs under the MWK
  keep both erasure grains.
- **Row-grain `claim_edges` lineage.** An edge table at row grain is write amplification
  through the back door, plus a closure walk that touches ten million edges. The structural
  `manifest_id` column carries the same fact.
