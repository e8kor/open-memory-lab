<!--
id: adr/ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database
tier: engineering
status: proposed
answers: what the lake holds today and why PostgreSQL cannot be rebuilt from it; the lake's new role as PostgreSQL's durable twin; the replay set (signed events, sealed fold inputs, wrapped data keys); what is excluded and re-issued after a rebuild; who can open a sealed twin row, R3 in the lake, KEK custody and erasure; the rebuild verb, its order, its idempotency and its chain verification; point-in-time and per-tenant rebuilds; derived indexes and migrations; the equality proof; the RPO and how the lake and a backup complement each other; storage and retention; doctor lines, metrics, the drill and the cost
-->

# ADR-0090 — The lake is a replayable twin of PostgreSQL: it holds every signed event with its sealed fold inputs, and rebuilds PostgreSQL into an empty database

**Status:** proposed · 2026-11-14 · requested by the operator on 2026-11-14 ("persisting to deltalake, and having opportunity to reconsolidate postgresql state from delta lake"; asked which role the lake should have, the operator chose a full replayable copy)

**Amends:** ADR-0003 (d1, d3, d6), ADR-0060 (d2, d6, d14, d19), ADR-0005 (d4)

## Context

[ADR-0002](ADR-0002-postgresql-system-of-record.md) d1 makes one PostgreSQL transaction the commit
point, and [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) d1 narrows the lake to analysis: a
projection nobody restores from. [ADR-0060](ADR-0060-the-lake-writer-drains-the-outbox-it-has-seals-what-it-materializes-and-answers-only-the-operator.md)
built that lake. The operator now wants the lake to be the copy PostgreSQL can be rebuilt from,
with PostgreSQL still the commit point. This record keeps ADR-0002 whole and changes what the lake
holds and what may read it back.

### What is built (origin/main at 8172364d)

**Tables.** Per tenant prefix: `bronze/events`, `silver/claims`, `gold/lineage`,
`telemetry/recall_daily`; under `_provider/`: `lake/batches` (the frontier) and `bench/results`
(`crates/adapters/oml-lake/src/table.rs:182`, `:213`, `:246`, `:268`, `:295`, `:324`;
[delta-tables.md](../data/delta-tables.md)).

- `bronze.events` takes every journal event the outbox names, with `prev_hash`, `hash`, `sig`,
  `signed_bytes`, `payload`, `principal`, `api_key_id` and `outbox_seq`
  (`crates/bins/oml-lakewriter/src/lib.rs:816`; the read is `source.rs:240`). It is never
  dead-lettered: a failed bronze commit fails the batch, which is retried (`lib.rs:451`).
- `silver.claims` takes a version only when a `lifecycle.*` event names it, read from `memories`
  as it stands at drain time (`lib.rs:543`, `source.rs:300`); a version with no `promoted_at` is
  dead-lettered as `version_not_promoted` (`lib.rs:572`), and a `self_signed` event is skipped
  (`lib.rs:548`).
- `gold.lineage` takes ratified edges and each materialized version's `derives_from` and
  `supersedes` links. `telemetry.recall_daily` is counts only, rewritten from the 30 days
  PostgreSQL keeps (`lib.rs:704`).

**Sealing.** Each materialized version gets a random data key; its body is sealed with
ChaCha20-Poly1305 under associated data binding tenant and version, and the data key is wrapped
by the lake KEK from an owner-only file (`crates/adapters/oml-lake/src/crypto.rs:1`–`10`, `:76`,
`:281`). The wrapped keys live in PostgreSQL alone, in `artifact_keys` (migration 0031); erase
nulls `wrapped_dek` there (`source.rs:521`). The KEK id is the first 8 bytes of its SHA-256
(`crypto.rs:32`).

**What never reaches the lake.** Held bodies, by design (ADR-0003 d3; `lib.rs:572`); self-signed
promotions; every row the journal payload does not carry: memory bodies (`memories.claim`, stored
plaintext in PostgreSQL, `crates/oml-app/src/app.rs:5438`), guardrail rules (the payload carries
`rule_digest`, `app.rs:837`), API key hashes (the `key.issued` payload carries none, `app.rs:2552`,
while the fold writes `api_keys.hash`, `crates/adapters/oml-pg/src/lib.rs:3833`), web allowlist
addresses (`web_emails.email`, migration 0015; the payload carries `email_digest`), connect-code
and token digests (ADR-0031 d10), web sessions, idempotency records, recall events and their text,
and every derived index.

**The outbox and the frontier.** Every commit writes one `outbox` row of kind `bronze-event` in the
journal's transaction (`oml-pg/src/lib.rs:5115`). The writer cuts batches from the stable prefix
without a lock (ADR-0060 d4, `source.rs:207`), applies each table only past its own Delta `txn`
version, and commits `_provider/lake/batches` last; that table's `txn` version is the frontier, and
no PostgreSQL column records it (ADR-0060 d3; `lib.rs:349`–`388`). Outbox rows are never purged.

**What a restore relies on today.** A `pg_dump` or base backup plus the keyring file holding the
signing key ([restore](../operations/restore.md)). The lake is "not a restore scenario" there, and
how the writer resumes against a rewound journal is not verified.

### Why PostgreSQL cannot be rebuilt from today's lake

1. **No body of a held, rejected or self-signed version is in the lake**, and a promoted body is
   in silver only after its promotion was drained; a held version's text exists in PostgreSQL alone.
2. **The wrapped data keys are in PostgreSQL alone.** Without `artifact_keys`, every sealed silver
   body opens under no key, the KEK file notwithstanding.
3. **Folds read inputs the journal does not carry.** The store's `MetadataFold`
   (`crates/oml-app/src/ports.rs:185`) is built by each use case from the request and the event:
   a key's hash, a version's body and guardrail rule, an allowlist entry's address, a connection's
   token digests. Replaying `bronze.events` alone cannot reproduce `api_keys`, `memories` or
   `web_emails`.
4. **No code turns an event back into its fold.** Folds are constructed inside the use cases of
   `oml-app`; nothing maps a stored event to the fold it committed with.
5. **Silver is state at drain time, not at the event.** It answers "what does this version look
   like now", which a point-in-time rebuild cannot use.
6. **Nothing verifies, orders or writes a rebuild.** No verb reads the lake into PostgreSQL, no
   law compares a rebuilt database with its source, and the chain verification (chain-format.md
   steps 1–9; `oml-pg/src/lib.rs:1849`) runs only at the lead's start.
7. **The KEK file and the lake share no stated custody split.** Lake files plus the KEK file plus
   a PostgreSQL copy open every promoted body; the twin would add held bodies to that set.

### Drift found on the way (not decided here)

- [storage.md](../architecture/storage.md) still describes a PostgreSQL frontier and a journalled
  `materialize-skipped` event; ADR-0060 d3 and d14 replaced both.
- [security.md](../architecture/security.md) says memory DEKs are wrapped by the lead's keyring;
  bodies are stored plaintext in `memories.claim` (`app.rs:5438`), and the keyring holds the
  signing key (`crates/bins/oml-server-lead/src/main.rs:120`).
- The `submission.*` payload carries `title` in clear (`app.rs:887`), so `bronze.events` and the
  journal hold a version's title as plaintext, where ADR-0002 d5 and ADR-0005 d4 call both
  digest-only. Erasure cannot reach a title.

Decided, not built.

## Decision

- **d1 — The lake is PostgreSQL's twin: complete, sealed and verifiable; PostgreSQL stays the
  commit point.** ADR-0002 d1 stands: a write is durable when PostgreSQL commits it, and no lake
  write is on the commit path. What changes is the lake's purpose (amends ADR-0003 d1): besides
  analysis, it holds everything needed to rebuild PostgreSQL's record and state into an empty
  database, and the rebuild proves itself against the deployment's public key. The lake still
  serves no client read (ADR-0013 d1). The benefit is a second copy that is independent of
  PostgreSQL's files, verifiable without trusting the copy, and replayable to any outbox seq. The
  cost is a lake that holds every body, held ones included, sealed; d4 bounds who can open it.

- **d2 — The replay set is the journal, the fold inputs it lacks, and the wrapped keys; nothing
  else.** Three tables carry it, per tenant prefix:
  - **`bronze.events` is the twin journal.** It already holds every column chain verification
    needs (`signed_bytes`, `sig`, `hash`, `prev_hash`, envelope columns). Its completeness becomes
    a contract: a bronze row is never dead-lettered and never skipped (as built, `lib.rs:451`), and
    its rows are never rewritten except by compaction that preserves them. Verdicts
    (`review.approve`, `review.reject`, `review.ratify`), grants, denials, names, quotas, policies,
    guardrail scope (`key.issued`), workarea marks and transitions all ride here, since each is an
    event whose payload carries its fold.
  - **`twin/inputs`, append-only, one row per (event, part).** The fold inputs a payload carries
    by digest or not at all, each sealed: a version's body for every `submission.*` event, held
    or not; its guardrail rule; an attachment's bytes once ADR-0004 builds them; an issued key's
    hash (`key.issued`); an allowlist address (`web.email_allowed`, `web.signed_up`); the
    idempotency record a commit wrote. Columns: `tenant_id`, `journal_seq`, `outbox_seq`, `part`
    (a closed set), `key_ref` (the data key that seals it), `sealed`, `plaintext_digest`,
    `ciphertext_digest`, and `absent` with a reason when the source row no longer exists at drain.
  - **`twin/keys`, MERGE by `key_ref`.** The lake's copy of `artifact_keys`: `wrapped_dek`,
    `kek_id`, `created_at`, `destroyed_at`, `applied_seq`. PostgreSQL's table stays the live copy;
    this one is what makes a sealed row openable after PostgreSQL is gone.

  The writer reads each input when it drains the event that names it, as it reads versions today
  (ADR-0060 d2), and checks it against the payload before sealing: a body against
  `plaintext_digest`, a rule against `rule_digest`, an address against `email_digest`. A mismatch
  stops the stream by name (`lake.twin_mismatch`), never skips. Bodies, rules, key hashes and
  addresses are immutable per id, so the drain-time read is the commit-time value. The one input
  that can vanish before drain is an allowlist row a later `web.email_removed` deleted; it is
  recorded `absent: removed_later`, and d6 says what a rebuild does with it. The idempotency
  record has no link to its event today, so the build adds `journal_seq` to `idempotency`
  (a migration, reserved at build time). Silver, gold and telemetry are unchanged: they stay the
  analysis zone and are never read by a rebuild.

- **d3 — Excluded and re-issued after a rebuild: secrets, sessions and counters.** None of these
  enters the lake:
  - the deployment signing key (the lead's keyring, ADR-0023 d4); a rebuild verifies with the
    public key and signs nothing, and the rebuilt lead loads its keyring from its own custody or
    rotates (`system.signing_key_rotated`, unplanned, chain-format.md);
  - raw API keys, which exist nowhere after issuance (hashes only, d2 keeps the hashes);
  - connect-code digests, authorization codes, OAuth tokens and client registrations
    (`connect_codes`, `oauth_tokens`, `oauth_authorizations`, `oauth_clients`): a rebuilt
    connection has no tokens, its refresh fails, and the holder connects again with a new code;
  - web sessions and their capacity credentials: everyone signs in again;
  - the in-memory rate counters (ADR-0079), recall events and outcomes (30-day telemetry,
    ADR-0054 d2; their daily counts stay in `telemetry.recall_daily`), and `lake_dead_letter`.

  Derived rows are recomputed, never copied (d7). The cost: a rebuild logs out every person and
  disconnects every connector; agent keys keep working, because their hashes are in the twin.

- **d4 — Sealing, custody and R3 in the lake.** Every `twin/inputs` row is sealed under a data key
  of its own scope: a version's body, rule and attachments under that version's data key (the one
  ADR-0060 d6 already creates, now created when the submission is drained rather than the
  promotion; amends ADR-0060 d6); every other part under a data key of its own, so a key hash or an
  address is erasable alone. Associated data binds tenant, `key_ref` and part, so a row moved to
  another place opens nowhere.
  - **Custody (amends ADR-0060 d19).** The lake files, the KEK file and PostgreSQL's backups are
    three custody points, and no two share a medium: lake files plus the KEK now open held bodies
    too. `oml-lakewriter doctor` fails `lake.kek` when the KEK file lies under the lake root, and
    the runbook stores the KEK backup apart from both. Rotation is ADR-0060 d19's, extended to
    re-wrap `twin/keys` in the same lease; a rotation finishes only when no live key in either copy
    is wrapped by an older version.
  - **R3 ([ADR-0001](ADR-0001-the-constitution.md) d3).** The `twin/` prefix and `bronze.events`
    are not a consumer zone. No named query reads `twin/`, `oml-lakewriter query` refuses a path
    under it, and the DuckDB recipes name none. A held body leaves the twin only into a rebuilt
    PostgreSQL, where the server applies R3 again. ADR-0003 d3's rule "unreviewed content never
    enters a consumer-readable zone" stands; its first sentence becomes "no held body reaches a
    consumer zone" (amends ADR-0003 d3). The boundary, stated: whoever holds the KEK file and the
    lake files can open held text, as the PostgreSQL owner can read `memories.claim` today.
  - **Erasure ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d2).** On `lifecycle.erase` the
    writer destroys the version's data key in `artifact_keys` and MERGEs its `twin/keys` row to
    destroyed. The ciphertext in `twin/inputs` stays, opening under no live key. Earlier Delta
    versions of `twin/keys` hold the wrapped key until VACUUM removes them at the erasure bound
    (ADR-0060 d12), and a copy that outlives that opens only under a KEK version that rotation
    retires. A rebuild reads `twin/keys` at its current version only, never by time travel, so the
    erase horizon of every rebuild is now: a point-in-time rebuild never brings back text erased
    after its cut. ADR-0005 d4 gains two inventory rows (amends ADR-0005 d4):

    | Copy class | Holds | Disposition at erase |
    |---|---|---|
    | `twin/inputs`, all Delta versions | per-key ciphertext of bodies, rules, attachments, key hashes, addresses, idempotency records | never touched; unreadable once the key is destroyed |
    | `twin/keys`, earlier Delta versions | a usable wrapped copy of a destroyed data key | current version destroyed at erase; earlier versions until VACUUM at the lake erasure bound; any surviving copy dies with the KEK version rotation retires |

- **d5 — One rebuild verb, on the lake writer's binary, into an empty database.**
  `oml-lakewriter rebuild --into <owner URL of an empty database> --signing-key <public key or
  fingerprint file> --kek-file <file> [--tenant <name or id>]… [--upto-outbox-seq N | --upto-at T]`.
  It runs the current migrations (the lead's migrator), then, per tenant, the provider tenant first
  (chain-format.md step 8 needs it):
  1. reads `bronze.events` up to the cut in `journal_seq` order and verifies the chain before it
     writes anything: steps 1 to 9 against the authenticated key set, which is the named key
     extended by the provider chain's cross-signed rotations; step 9 runs against the last event
     of the cut. A tenant that fails stops the rebuild, named;
  2. for each event, opens its `twin/inputs` parts, checks every digest d2 checks, and derives its
     fold with `oml_app::fold_of(event, inputs)`, a pure function the build adds beside the use
     cases;
  3. commits the journal row, the chain head, the fold and an outbox row carrying the event's
     original `outbox_seq` in one transaction, through `oml-pg`'s own fold code, metering applied
     and no ceiling refused, since the event was already committed once;
  4. when every tenant is done, loads `artifact_keys` from `twin/keys` as it stands, sets the
     outbox sequence past the largest seq, and writes nothing else.

  **Idempotency.** The rebuild resumes from the target's own chain heads: an event at or below a
  head must equal the stored row byte for byte, or the rebuild refuses (`rebuild.diverged`). Run
  twice, it writes nothing the second time. Because the outbox keeps its seqs, the writer resumes
  against the rebuilt database at the same frontier and re-drains nothing.

  **Point in time.** The cut is a global outbox seq, the one order every tenant shares at the
  stable prefix (ADR-0060 d4); `--upto-at` resolves to the largest outbox seq whose event is at or
  before `T`. A cut that falls inside an `absent: removed_later` window refuses and names the
  window's ends.

  **Per tenant.** `--tenant` rebuilds the named tenants and the provider chain into an empty
  database. Merging a rebuilt tenant into a live database is not decided here.

  **Rolling forward a backup.** `--onto <owner URL of a restored backup>` accepts a non-empty target
  whose every chain head is a byte-equal prefix of `bronze.events`, and replays the suffix. This is
  how the lake and `oml-backup` (#182) compose: the backup is the fast restore, and the lake brings
  it forward to its own frontier, erase events included, which is the fenced restore ADR-0005's
  consequences call for.

  The writer still holds no signing key (ADR-0006 d5); `oml-lakewriter` gains `oml-app` and
  `oml-pg` as dependencies, the direction ADR-0060 d18's law allows, and the law still forbids the
  reverse. The verb is the operator's alone: it needs the owner URL, the KEK file and the host, and
  no server route or MCP tool reaches it (R1, ADR-0001 d1).

- **d6 — Migrations: every event folds into the current schema.** `fold_of` reads every payload
  shape ever written, as the store does today (an event without `format` reads as `text/plain`,
  event-catalog.md). A schema change that adds a fold input adds its `twin/inputs` part in the
  same change, and a law fails when a `MetadataFold` variant has a field neither the payload nor a
  part supplies.

- **d7 — Derived rows are recomputed by the code that builds them live.** Search postings and
  term rows, trigram and full-text indexes, `similar_keys`, the sparse queue, `review_queue`,
  `scan_findings` as the payload records them, lineage flattening, account meters and
  `usage_daily` are written by the same triggers and folds a live commit runs, or by the existing
  backfills (the sparse backfill, ADR-0052). The rebuild copies none of them, so a rebuild also
  repairs a corrupt index.

- **d8 — The proof is an equality, in CI and as a drill.** `oml-lakewriter compare --left <URL>
  --right <URL>` writes one digest per table over its canonical, ordered rows and names every table
  that differs. The comparison skips the d3 exclusions and a closed list of columns written by
  `now()` at commit rather than from the event's `at`; the build either moves each such column to
  the event's `at` or lists it with its reason. The law
  `a_rebuilt_database_equals_the_database_it_was_drained_from` runs the demo workload against a
  throwaway cluster, drains, rebuilds into a second cluster and compares. The drill runs the same
  rebuild and compare against a scratch database on the operator's host, monthly, and records its
  time.

- **d9 — The RPO of a lake-only rebuild is the writer's lag, and it is a doctor line.** Events
  PostgreSQL committed and the writer had not drained are lost when PostgreSQL is lost with no
  backup. `synchronous_commit` (ADR-0002 d2) bounds what PostgreSQL loses on failover; it does not
  shorten the lake's lag. `oml-lakewriter doctor` adds `lake.rpo`: `warn` past
  `--rpo-warn-seconds` (default 300), `fail` past `--rpo-fail-seconds` (default 3600), both read
  from `lake.lag_seconds`; the defaults are a starting choice, unmeasured until the lag is measured
  (#101 measures exactly-once, not lag; d11 proposes the issue). The silver table may still
  dead-letter (ADR-0060 d14); bronze, `twin/inputs` and `twin/keys` never do: a row they cannot
  take fails the batch and the frontier stops, so the twin is never knowingly incomplete (amends
  ADR-0003 d6 and ADR-0060 d14 for these tables). The cost: one poison event stops the twin, and
  `lake.lag` says so within one tick.

- **d10 — Storage: the object_store port, append-only, no retention cut.** The twin uses the
  storage ADR-0060 d15 allows (a local path) and moves to object storage with the same port when
  the storage decision lands; a target without conditional put stays refused. The lake belongs on
  a different failure domain from PostgreSQL's data: a twin on PostgreSQL's disk dies with it, and
  the runbook says so. `bronze.events` and `twin/inputs` are never pruned: OPTIMIZE rewrites files
  and VACUUM removes only unreferenced ones, so no row leaves; `twin/keys` is compacted like any
  MERGE table. Ciphertext of erased inputs stays forever, unopenable; dropping it would be a later,
  journalled decision.

- **d11 — Operability.** Metrics: `oml_lake_twin_rows` and `oml_lake_twin_bytes` by table,
  `oml_lake_rebuild_events`, `oml_lake_rebuild_seconds`, no tenant label (ADR-0060 d16). Doctor:
  `lake.rpo` (d9), `lake.twin` (per tenant, the bronze row count up to the frontier equals the
  journal's), `lake.twin_mismatch` (d2), `lake.kek` placement (d4), `lake.drill` (`warn` when no
  recorded drill is younger than 35 days). The runbook gains "Rebuild PostgreSQL from the lake" and
  "Roll a backup forward" in [lake operations](../operations/lake.md) and
  [restore](../operations/restore.md). The cost per memory, the write amplification and the
  rebuild's time are unmeasured; the issue to measure them is proposed in the operator's report.

- **d12 — The constitution holds unchanged.** [ADR-0001](ADR-0001-the-constitution.md) R1 to R4
  are not restated here. The rebuild verb is an operator act no agent surface reaches (R1); it
  replays approvals the journal already holds and decides none (R2); held text never reaches a
  consumer zone (R3, d4); nothing is deleted, and an erase stays an erase through every rebuild (R4,
  d4).

## Consequences

- PostgreSQL can be lost without losing the record: a lake-only rebuild returns every tenant to
  the frontier, verified against the public key, at the cost of every session, token and
  connection.
- The writer reads more per event (one input per submission, key issuance and allowlist change)
  and writes two more tables; the size and time are unmeasured.
- `oml-app` gains `fold_of`, and a law holds it equal to each use case's fold.
- The idempotency table gains `journal_seq`, which needs a migration.
- The KEK becomes the key to held text as well as approved text; its custody rule tightens (d4).
- ADR-0003's status line reads "amended by ADR-0060, ADR-0090 (proposed)"; ADR-0060's and
  ADR-0005's gain ADR-0090.
- The drift named in the context (storage.md's frontier, security.md's keyring, titles in clear)
  is not repaired here; each is a separate change.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **The lake as the authority, PostgreSQL rebuilt as its cache** | ADR-0002's dual-write argument still decides: no transaction spans both stores. The operator kept PostgreSQL as the commit point. |
| **Copy table images instead of replaying events** | A snapshot of each table at drain time cannot stop at a seq, cannot be verified against a signature, and replays a corrupt index as faithfully as a sound one. Replay through folds gives point-in-time, per-tenant rebuilds and a signature behind every row. |
| **Sealed fold inputs written by the lead into the outbox at commit** | Exact at commit time, but the lead holds no lake KEK, so the inputs would ride the outbox in plaintext and every erase would have to reach the outbox too. Drain-time reads are exact for every immutable input; the one mutable case is named (d2). |
| **Put fold inputs in the journal payload** | Bodies and addresses in signed bytes cannot be erased without breaking the chain (ADR-0002 d5, ADR-0005 d2). |
| **Rebuild from silver** | Silver is state at drain time, holds approved versions only, and is a consumer zone; it cannot carry held text under R3. |
| **Store the wrapped data keys only in PostgreSQL, as built** | Then the lake opens nothing once PostgreSQL is gone, which is the case the twin exists for. |
| **Re-issue every API key after a rebuild instead of keeping hashes** | A hash authenticates nothing on its own, and forcing every agent to be re-keyed after an outage multiplies the outage. Kept as an operator question. |
| **A rebuild into the live database, tenant by tenant** | Merging into a database that is serving needs its own fencing against the lead's writes; deferred. |
| **Time-travel the key table for point-in-time rebuilds** | It would resurrect keys erased after the cut. The erase horizon is always now (d4). |
