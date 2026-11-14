<!--
id: adr/ADR-0060-the-lake-writer-drains-the-outbox-it-has-seals-what-it-materializes-and-answers-only-the-operator
tier: engineering
status: accepted design · 2026-11-06
answers: how ADR-0003's lake writer is built against the code as it stands; which parts of ADR-0003 and its pages were stale and what replaces them; the binary and crate split; the outbox input; exactly-once and the stable prefix; the lease; how bodies are sealed and erased before ADR-0005's capture-time keys exist; recall telemetry and benchmark results in the lake; the tables; the operator's queries; maintenance; the Postgres role; dead letters; storage; doctor lines and metrics; the one C library; who holds the KEK and how it rotates; the laws and their tests
-->

# ADR-0060 — The lake writer drains the outbox it has, seals what it materializes, and answers only the operator

**Status:** accepted design · 2026-11-06 (was accepted) · introduced in v0.6.0 · first recorded 2026-11-05 (d17 and d19 decided by the operator, 2026-11-05) · amended by [ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md) (d2, d3, d6, d14, d19) · amends [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) (d3, d4, d6), [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) (d1, d2), [ADR-0006](ADR-0006-pure-rust.md) (d1)

**Release:** introduced in v0.6.0 ([releases](../changelog.md))

**Amends:** ADR-0003 (d3, d4, d6), ADR-0005 (d1, d2), ADR-0006 (d1)

## Context

The operator decided on 2026-11-05 to build ADR-0003's lake for v0.6.0, so that all gathered
knowledge lands in Postgres and in Delta and the lake feeds dashboards of how the system is
used. ADR-0003 was written on 2026-10-29 with "no code exists", and the code has moved since.
Read against `origin/main` at v0.5.7, these parts of it and its pages are stale:

1. **The outbox carries one kind.** Every commit writes one `outbox` row of kind
   `bronze-event` carrying the event's id, type and seq (`oml-pg` `append_event`). No
   `materialize` or `retract` row is ever written, so "silver materialization is enqueued by
   promotion only" (ADR-0003 d3) has nothing to drain.
2. **No body is encrypted and no key exists.** Bodies are plaintext in `memories.claim`;
   `artifact_keys`, the KEK and KMS (ADR-0005 d1) are unbuilt, the submission event's
   `ciphertext_digest` is null, and erase has no use case (ADR-0052 d7). The lake cannot copy a
   ciphertext that does not exist.
3. **The writer has no signing key** (ADR-0006 d5), so ADR-0003 d6's journalled
   `materialize-skipped` event cannot come from it.
4. **The lease is Kubernetes'.** ADR-0016 d3 names a `coordination.k8s.io` lease; this host runs
   no cluster.
5. **delta-rs moved to 1.x.** ADR-0003 d4 and ADR-0006 d2 left its application transaction ids
   unverified. deltalake 1.1.2 (docs.rs, fetched 2026-11-05) records `txn` actions through
   `CommitProperties::with_application_transaction` and reads them back with
   `transaction_version`; the probe and the laws below show they survive OPTIMIZE, checkpoint
   and VACUUM. Its core hard-enables `rustls`, which reaches `aws-lc` through reqwest 0.12.
6. **Recall telemetry and benchmark results exist** (ADR-0054, `oml-bench/1`) and no lake page
   names them. Neither is journalled, so neither reaches the outbox.
7. **Memory state has no `superseded`.** A superseded version stays `promoted` with `is_tip`
   false (migration 0005); `silver.claims.state` must derive it. Anchors and formats
   (ADR-0056, ADR-0057) are not on `main`, so no column carries them yet.
8. **`gold.areas` and `gold.curated_<id>`** have no consumer in v0.6.0.

## Decision

- **d1 — A binary and an adapter, and nothing else reaches the lake.** `crates/adapters/oml-lake`
  holds delta-rs: tables, sealing, maintenance and the named queries; it reads no Postgres.
  `crates/bins/oml-lakewriter` (ADR-0006 d1's binary) reads Postgres and drives it. The lead,
  the HTTP transport, the CLI, `oml-app` and `oml-pg` depend on neither, and a law asserts it
  from `cargo metadata` (d18). The cost is a third binary to deploy; the benefit is that no
  serving or client path can read the lake, by construction (ADR-0013 d1).

- **d2 — The input is the outbox as written; the writer classifies by event type.** No new
  outbox kind. `bronze.events` takes every event. For `lifecycle.*` events the writer reads the
  named versions (`artifact_id`, `new_artifact_id`, `replaces_artifact_id`) from `memories` as
  they stand at drain time and upserts their `silver.claims` rows; a version whose
  `promoted_at` is null is never written, so no held body enters the lake (ADR-0003 d3 holds).
  A `self_signed` promotion stays out (ADR-0001); a `same_user` one enters with that column set,
  because on a one-person install those are most of what was gathered. `review.ratify` naming
  an `edge_id` upserts its edge when ratified. Amends ADR-0003 d3's mechanism, not its rule. *(Self-signing retired 2026-11-14 (ADR-0001 d5); no row carries the mark.)*

- **d3 — Exactly-once by per-table transaction version and a per-row filter.** Every commit
  carries `txn(appId = oml-lakewriter, version = batch_end)`. Each table applies only rows whose
  outbox seq exceeds its own last version, so a batch cut differently on replay still writes no
  row twice; ADR-0003 d4's deterministic reconstruction is no longer needed for correctness. The
  global frontier is the `txn` version of `_provider/lake/batches`, committed last; no Postgres
  frontier column is built. MERGE tables also guard on `applied_seq`, so an older operation never
  overwrites a newer one. Amends ADR-0003 d4.

- **d4 — The stable prefix is read without a lock.** The writer reads `outbox_seq_seq`'s value,
  then the virtual transactions holding `RowExclusiveLock` on `outbox`; every transaction that
  drew a seq at or below that value took the lock first and holds it to its end. Once those
  have ended the value is stable. The writer takes no lock on `outbox`, so a commit never waits
  for it; a transaction open past a second shows as lag, never as loss.

- **d5 — The lease is a Postgres session advisory lock.** Key `0x6f6d6c5f6c616b65` ("oml_lake"),
  held on a connection of its own. The Delta `txn` version and the store's put-if-absent stay
  the fence that holds when two writers overlap (ADR-0016 d3); a law runs two writers past the
  lease and finds no row twice.

- **d6 — The lake seals what it materializes, until ADR-0005's capture-time keys exist.** Each
  materialized version gets a random 256-bit data key; its body is sealed with
  ChaCha20-Poly1305 (ring), associated data binding tenant and version, and the data key is
  wrapped by a lake KEK read from an owner-only file and stored in `artifact_keys`
  (postgres-schema.md's shape, migration 0031). On `lifecycle.erase` the writer sets
  `wrapped_dek` NULL and `destroyed_at`, then MERGEs the row to `erased` with no body. Every
  older table version still holding the ciphertext opens under no key from that moment;
  VACUUM removes it within the erasure bound. The cost, stated: the writer holds the KEK and
  can unwrap, where ADR-0005 d1 gives it an encrypt-only grant; d19 accepts that and bounds
  it. When capture-time keys are built, the writer copies the capture ciphertext and drops the
  KEK. Amends ADR-0005 d1, d2.

- **d7 — Telemetry enters as daily counts only.** `telemetry.recall_daily` holds, per tenant,
  day, pipeline and access path: recalls, hits, empty recalls, how many events kept text, and
  each outcome label's count. No query text, no digest, no version id enters, so the opt-in,
  erase and the 30-day purge hold by construction; the writer rewrites the table from the 30
  days Postgres keeps, so the current version mirrors that retention. Copying opted-in text was
  rejected: it would put plaintext in a lake copy class ADR-0005 d4 does not list.

- **d8 — Benchmark results are provider data.** `_provider/bench/results` takes every number of
  every `oml-bench/1` file, one row per metric and category, keyed by the file's SHA-256, so a
  file loaded twice is one set of rows. `oml-lakewriter load-bench <paths>` loads them.

- **d9 — Lineage is ratified edges and materialized links.** `gold.lineage` holds ratified claim
  and brief edges and each materialized version's `derives_from` and `supersedes` links.
  `gold.areas` and `gold.curated_<id>` are deferred: the bronze `workarea.create` rows name each
  workarea for the queries.

- **d10 — Tables, per tenant prefix.** `<lake>/<tenant>/bronze/events` (partitioned by
  `event_date`), `silver/claims`, `gold/lineage`, `telemetry/recall_daily` (partitioned by
  `day`); `<lake>/_provider/lake/batches` and `bench/results` (by `run_date`). Columns are in
  [delta-tables.md](../data/delta-tables.md).

- **d11 — The operator's queries run in the writer's binary.** `oml-lakewriter query <name>` runs
  named DataFusion SQL over the lake files under the lake's own file permissions: writes per
  kind per day, review latency, recall volume per pipeline, recall outcomes, stale share, top
  workareas, latest benchmarks. No server route and no CLI verb reaches them (ADR-0013 d1); the
  same SQL runs in DuckDB against the same paths ([lake operations](../operations/lake.md)).

- **d12 — Maintenance in the writer.** Daily, per table: bin-pack OPTIMIZE, a checkpoint, expired
  log cleanup and VACUUM at the erasure bound (default 168 hours); a checkpoint every 100
  commits besides. `delta.setTransactionRetentionDuration` is never set, so no `txn` expires.
  Z-order is deferred until a table is large enough to measure it.

- **d13 — Its own Postgres role, granted at its start.** The writer logs in as a role that is
  neither owner nor serving, refused by name when it is a superuser or bypasses row security.
  Given `OML_LAKE_OWNER_URL`, it grants that role SELECT on the tables it reads and the outbox
  sequence, and SELECT, INSERT, UPDATE on `artifact_keys` and `lake_dead_letter`, then closes the
  owner connection (as ADR-0034 d4 does for the lead). Every tenant read sets `oml.tenant_id`.

- **d14 — Dead letters are a Postgres row and a doctor line.** An event that names a version or
  edge the writer cannot materialize goes to `lake_dead_letter` with a reason, and the stream
  moves on. A commit failure is retried next tick and never skipped. The journalled
  `materialize-skipped` event waits for a lead verb. Amends ADR-0003 d6.

- **d15 — Storage is a local path.** `OML_LAKE_URL` is an absolute path or `file://` URL (for example
  `/srv/oml/lake`). Another scheme is refused by name until ADR-0035's object
  store lands; nothing logs a URL or a credential.

- **d16 — Doctor lines and metrics.** `oml-lakewriter doctor` prints `lake.writer` (no lease
  holder: the writer is down), `lake.lag` (rows past the frontier), `lake.lag_seconds`,
  `lake.heartbeat`, `lake.kek` (file permissions, age past 90 days, a rotation left unfinished), `lake.commits` (failed ticks and the last error class), `lake.schema`
  (drift per table), `lake.dead_letter` and `lake.maintenance`. The metrics listener serves
  `oml_lake_frontier_seq`, `_horizon_seq`, `_lag_rows`, `_lag_seconds`, `_batches`, `_rows`
  and `_commit_failures` by table, `_dead_letters`, `_maintenance_runs`, `_maintenance_files`
  and `_batch_millis`; no label names a tenant.

- **d17 — One C library, confined to `oml-lakewriter`; an ADR-0006 exception decided by the
  operator, 2026-11-05.** deltalake-core 1.1.3 enables reqwest's rustls with aws-lc, a C
  library, and offers no ring choice. ADR-0006 d1 (pure Rust) stands for the server and the
  CLI; this binary is the one exception. The server and CLI do not link it:

  ```text
  $ cargo tree -p oml-server-lead -i aws-lc-sys
  error: package ID specification `aws-lc-sys` did not match any packages
  $ cargo tree -p oml-cli -i aws-lc-sys
  error: package ID specification `aws-lc-sys` did not match any packages
  $ cargo tree -p oml-lakewriter -i aws-lc-sys
  aws-lc-sys v0.47.0
  └── aws-lc-rs v1.19.0
      ├── rustls v0.23.46
      │   ├── hyper-rustls v0.27.10 …
  ```

  Cargo reports "did not match any packages" when the crate is not in that package's
  dependency graph: the server and the CLI contain none of it. d1's law
  `no_serving_or_client_binary_depends_on_a_lake_crate` keeps it so. The revisit trigger: a
  delta-rs release that lets the caller pick the rustls provider, or a ring-backed build of
  the lake adapter. Amends ADR-0006 d1 for this binary only. `deny.toml` carries the
  matching entries (the licences of four transitive crates, and the two quick-xml advisories
  reached only through object_store's cloud clients, which the writer does not use because
  d15 keeps storage local), each commented as operator-approved 2026-11-05.

- **d18 — The laws.** In `crates/bins/oml-lakewriter/tests/lake_laws.rs`, against a throwaway
  cluster: no serving or client binary depends on a lake crate; a drained lake holds every
  event once and a replay from a reset frontier writes nothing twice; a second writer is refused
  the lease and two writers past it write no row twice; an erased body opens in no lake version,
  its text is in no lake file and VACUUM removes its ciphertext; each tenant prefix holds only
  its own rows; a crash between tables, or with an uncommitted data file, replays to the same
  lake; recall counts reach the lake without query text for 30 days; bench results load once; a rotated KEK keeps every live body readable and an erased body
  unreadable; a KEK file that group or others can read stops the writer before it connects.

- **d19 — The writer holds the lake KEK; the file is owner-only and rotates. Decided by the
  operator, 2026-11-05; amends ADR-0005 d1.** ADR-0005 d1 gives the lake an encrypt-only
  grant. Until capture-time keys exist the writer must unwrap data keys to seal, so it holds
  the KEK. The bounds: (a) the KEK file is owner-only, and `run` and `drain-once` refuse to
  start on a file that group or others can read, naming the mode; (b) the file holds KEK
  versions, newest last, and a data key names the version that wrapped it in
  `artifact_keys.kek_id`; (c) `oml-lakewriter keygen --rotate --kek-file F` takes the lease
  (so no writer runs), adds a version, re-wraps every live data key under it in a
  compare-and-set that never resurrects a destroyed key, checks that no live key is wrapped
  by an older version, and only then rewrites the file with the new version alone; a
  rotation that stops part way leaves every version in the file and is finished by running
  it again; (d) `doctor` prints `lake.kek`: `fail` for a file that is too open or unreadable,
  `warn` past 90 days (the file's modification time, which a rotation resets) or for an
  unfinished rotation. Erase is unchanged: it destroys the data key, not the KEK. A KEK
  leaked while Postgres rows survive opens bodies still present in the lake; rotation limits
  that window and does not close it.

## Consequences

- The lake exists and is still never the authority: Postgres commits, the writer follows, and
  no client reads it.
- `artifact_keys` is now a key-material table in backups (postgres-schema.md durability).
- When ADR-0005's capture-time encryption is built, d6 is revisited: the writer copies the
  capture ciphertext and its key custody becomes encrypt-only.
- The release that ships it builds a third binary and runs migration 0031.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A lake-writer role inside the lead** | The lead holds the signing key; ADR-0006 d5 keeps the writer out of that process, and the lead would link delta-rs and DataFusion. |
| **New `materialize` and `retract` outbox kinds written by the lead** | A change to the commit path of every promotion for information the event type already carries. |
| **Plaintext bodies in silver until ADR-0005 is built** | Erase could then only be a rewrite, and old table versions would hold readable text until VACUUM. |
| **Copying opted-in query text into the lake** | A new plaintext copy class with its own erase rewrite, for analyses the counts already serve. |
| **`oml lake query` in the CLI** | It would put delta-rs and DataFusion in the CLI binary every user downloads. |
| **A Postgres frontier as recovery authority** | Rejected by ADR-0003 already; d3 keeps the Delta log authoritative. |
