<!--
id: adr/ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database
tier: engineering
status: accepted design · 2026-11-14
answers: what the lake holds today and why PostgreSQL cannot be rebuilt from it; the lake's role as PostgreSQL's durable twin; the replay set; commit-time staging of fold inputs; the inventory of every writer and how each is reproduced; what is excluded and what a rebuild makes of it; sealing, custody, placement and R3; erasure and the non-erasable signed metadata; per-tenant frontiers, the provider tenant, waivers and resume; the rebuild verb, its order, fencing, chain verification and sequence; cuts, epochs and drills; signed head checkpoints kept off the lake; migrations and the fold law; derived rows; the equality proof; the RPO; storage; doctor lines and metrics; what ADR-0080 to ADR-0089 owe the twin
-->

# ADR-0090 — The lake is a replayable twin of PostgreSQL: it holds every signed event with its sealed fold inputs, and rebuilds PostgreSQL into an empty database

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with every default and reviewer recommendation drafted for confirmation decided as drafted, the RPO thresholds of 300 and 3,600 seconds and the hourly checkpoint included, and its amendments of ADR-0003, ADR-0005 and ADR-0060 · proposed the same day, requested by the operator on 2026-11-14 ("persisting to deltalake, and having opportunity to reconsolidate postgresql state from delta lake"; asked which role the lake should have, the operator chose a full replayable copy); revised the same day after two independent reviews, with four decisions the operator took (d3, d8, d10, d11)

**Amends:** ADR-0003 (d1, d3, d6), ADR-0060 (d2, d3, d6, d14, d19), ADR-0005 (d3, d4)

## Context

[ADR-0002](ADR-0002-postgresql-system-of-record.md) d1 makes one PostgreSQL transaction the commit
point, and [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) d1 narrows the lake to analysis.
[ADR-0060](ADR-0060-the-lake-writer-drains-the-outbox-it-has-seals-what-it-materializes-and-answers-only-the-operator.md)
built that lake. The operator now wants the lake to be the copy PostgreSQL can be rebuilt from,
with PostgreSQL still the commit point. This record keeps ADR-0002 whole and changes what the lake
holds, how it is fed and what may read it back.

### What is built (origin/main at 8172364d)

**Tables.** Per tenant prefix: `bronze/events`, `silver/claims`, `gold/lineage`,
`telemetry/recall_daily`; under `_provider/`: `lake/batches` and `bench/results`
(`crates/adapters/oml-lake/src/table.rs:182`, `:213`, `:246`, `:268`, `:295`, `:324`;
[delta-tables.md](../data/delta-tables.md)).

- `bronze.events` takes every journal event the outbox names, with `prev_hash`, `hash`, `sig`,
  `signed_bytes`, `payload`, `principal`, `api_key_id` and `outbox_seq`
  (`crates/bins/oml-lakewriter/src/lib.rs:816`; the read is `source.rs:240`). A failed bronze
  commit fails the batch, which is retried; it is never dead-lettered (`lib.rs:451`). The named
  queries `writes-per-kind-per-day` and `top-workareas` read it
  (`crates/adapters/oml-lake/src/query.rs:33`, `:75`–`79`, `:108`).
- `silver.claims` takes a version only when a `lifecycle.*` event names it, read from `memories`
  as it stands at drain time (`lib.rs:543`, `source.rs:300`); a version with no `promoted_at` is
  dead-lettered (`lib.rs:572`), and a `self_signed` event is skipped (`lib.rs:548`).
- `gold.lineage` takes ratified edges and each materialized version's links;
  `telemetry.recall_daily` is counts only, replaced each refresh from the 30 days PostgreSQL keeps
  (`lib.rs:704`).

**Sealing.** Each materialized version gets a random data key; its body is sealed with
ChaCha20-Poly1305 under associated data binding tenant and version, and the data key is wrapped by
the lake KEK from an owner-only file (`crates/adapters/oml-lake/src/crypto.rs:1`–`10`, `:76`,
`:281`). The wrapped keys live in PostgreSQL alone, in `artifact_keys` (migration 0031), keyed by
`artifact_id`; erase nulls them there (`source.rs:521`).

**What never reaches the lake.** Held bodies (ADR-0003 d3); self-signed promotions; and every
value a journal payload does not carry: memory bodies (`memories.claim`, stored plaintext,
`crates/oml-app/src/app.rs:5438`), guardrail rules (the payload carries `rule_digest`,
`app.rs:837`), API key hashes (absent from the `key.issued` payload, `app.rs:2552`, written by the
fold, `crates/adapters/oml-pg/src/lib.rs:3833`), allowlist addresses (`web_emails.email`,
migration 0015), provider subject pins, connect-code and token digests (ADR-0031 d10), sessions,
idempotency records, recall events, usage counters and every derived index.

**The outbox and the frontier.** Every commit writes one `outbox` row of kind `bronze-event` in the
journal's transaction (`oml-pg/src/lib.rs:5115`). The writer cuts one global batch from the stable
prefix without a lock (ADR-0060 d4, `source.rs:207`), applies each table past its own Delta `txn`
version, and commits `_provider/lake/batches` last; that table's `txn` version is the one global
frontier (ADR-0060 d3; `lib.rs:349`–`388`). One tenant's failing commit therefore holds every
tenant back. Outbox rows are never purged.

**What a restore relies on today.** A `pg_dump` or base backup plus the keyring file holding the
signing key ([restore](../operations/restore.md)). The lake is "not a restore scenario" there, and
how the writer resumes against a rewound journal is not verified: a restored database whose
outbox sequence is behind the lake's frontier would have its next rows skipped.

### Why PostgreSQL cannot be rebuilt from today's lake

1. No body of a held, rejected or self-signed version is in the lake.
2. The wrapped data keys are in PostgreSQL alone, so no sealed body opens without it.
3. The store's `MetadataFold` (`crates/oml-app/src/ports.rs:185`) is built by each use case from
   the request, and needs values the journal does not carry (above).
4. Several tables are written outside any journalled commit (d4), so even a complete journal
   would not reproduce them.
5. Nothing maps a stored event back to its fold.
6. Silver is state at drain time, so it cannot stop at a point in time.
7. No verb reads the lake into PostgreSQL, no law compares a rebuilt database with its source,
   and the built chain check runs steps 1 to 7 and 9 (`crates/oml-domain/src/chain.rs:209`) only
   at the lead's start. Step 8 has a function, `verify_genesis_reference`, that no path calls,
   because no build journals the provider's `tenant.created`.
8. One global frontier means one poison event stops the lake for every tenant.
9. Nothing records a head outside the server, so a rebuild cannot tell a complete lake from a
   truncated one.

### Drift found on the way (not decided here)

- [storage.md](../architecture/storage.md) still describes a PostgreSQL frontier and a journalled
  `materialize-skipped` event; ADR-0060 d3 and d14 replaced both.
- [security.md](../architecture/security.md) says memory DEKs are wrapped by the lead's keyring;
  bodies are stored plaintext (`app.rs:5438`), and the keyring holds the signing key
  (`crates/bins/oml-server-lead/src/main.rs:120`).
- `submission.*` payloads carry `title` in clear (`app.rs:887`); d7 amends ADR-0005 to say so,
  and a digest with a sealed part is #389.

Decided, not built.

## Decision

- **d1 — The lake is PostgreSQL's twin: complete, sealed and verifiable; PostgreSQL stays the
  commit point.** ADR-0002 d1 stands, and no lake write is on the commit path. Amends ADR-0003 d1:
  besides analysis, the lake holds everything needed to rebuild PostgreSQL's record and state into
  an empty database, and a rebuild proves itself against the deployment's public key and a head
  kept off the lake (d11). The lake still serves no client read (ADR-0013 d1).

- **d2 — The replay set: the journal, the staged fold inputs, the wrapped keys, and the counter
  images.** Per tenant prefix:
  - **`bronze.events` is the twin journal.** Its rows are never dead-lettered, skipped or
    rewritten except by compaction that preserves them; verdicts, grants, denials, names, quotas,
    policies, guardrail scope, workarea marks and transitions ride here as events.
  - **`twin/inputs`, append-only, one row per staged part (d3).** Columns: `tenant_id`,
    `journal_seq` (null for a part staged outside a commit), `stage_seq`, `outbox_seq`, `part`
    (a closed set, d4), `key_ref`, `sealed`, `plaintext_digest`, `ciphertext_digest`.
  - **`twin/keys`, MERGE by `key_ref`.** The lake's copy of the wrapped data keys: `wrapped_dek`,
    `kek_id`, `created_at`, `destroyed_at`, `applied_seq`.
  - **`twin/state` (decided by the operator, 2026-11-14).** Images of the two counters no event reproduces:
    `account_activity.recalls`, written by `meter_recalls` outside any commit, and `usage_daily`,
    appended by the lead's `roll_up_usage` (`crates/adapters/oml-pg/src/lib.rs:5172`) from the
    stocks as read at the time (d4); the replay reproduces every other meter through
    `apply_meters`. Each image is
    stamped with the outbox sequence value it was taken at, and is replaced only when it changed,
    or daily. A rebuild loads the newest image; a cut (d10) loads the newest image at or before the
    cut. The lake's `telemetry.recall_daily` refresh stops replacing an existing table with an
    empty one, so a rebuilt database with no recall events leaves the lake's counts alone.

  Silver, gold and telemetry stay the analysis zone and are never read by a rebuild.

- **d3 — Fold inputs are staged in the commit's own transaction (decided by the operator,
  2026-11-14).** `append_event` writes each input the fold carries and the payload does not into
  `lake_staging`, in the same transaction as the journal row, the fold and the outbox row. A
  value written outside a commit (d4) stages in that write's own transaction under a `stage_seq`.
  The writer seals each staged row into `twin/inputs`, commits it to the lake, and only then
  deletes it, under a DELETE grant on `lake_staging` that it gives its own role at start
  (ADR-0060 d13). This replaces ADR-0060 d2's drain-time read for every twin input, so no input
  depends on being immutable or still present at drain.
  - **Erase before drain (decided by the operator, 2026-11-14).** An erase replaces the version's staged rows,
    in its own transaction, with tombstones: the part, `erased: true` and no bytes. The bytes
    leave PostgreSQL's staging at once; the writer records the part in `twin/inputs` as absent
    because erased, and the tenant keeps draining. The law
    `an_erase_before_the_drain_leaves_the_tenant_draining_and_the_part_absent` holds it.
  - **One order.** Each staged row records the outbox sequence value current at its insert, and
    the writer's horizon waits on every transaction holding the insert lock of the outbox or of
    `lake_staging` (ADR-0060 d4, widened), so a staged row is never seen before the commit that
    staged it, nor after a batch that passed it. A row staged outside a commit
    (`idempotency_settle`) takes no outbox lock, which is why the `lake_staging` lock is waited on
    too.
  - **Cost.** Staging inserts run inside the chain lock, before the outbox insert, so they add
    their bytes to the commit's WAL (a body is written twice, once to `memories` and once to
    staging) and lengthen the time the tenant's chain lock is held; both are unmeasured (#390).
    `lake.staging_age` names the age of the oldest staged row (decided by the operator, 2026-11-14).

  Staging adds no exposure: it holds bytes PostgreSQL already holds in `memories`, `api_keys`,
  `web_emails` and `idempotency`, under the same row security, for the time between commit and
  drain.

- **d4 — Every writer in `oml-pg` is classified, and the class decides how a rebuild reproduces
  it.** Four classes: **journal** (the event's payload is the fold), **part** (staged, d3),
  **derived** (recomputed by the same code from the replayed rows), **excluded** (d5, with its
  consequence). Writers outside `apply_fold` and `append_event` are named by function
  (`crates/adapters/oml-pg/src/lib.rs`, and `telemetry.rs`):

  | Writer | Writes | Class | How a rebuild reproduces it |
  |---|---|---|---|
  | `insert_memory`, from any fold that inserts a `memories` row | `memories` body, `rule`, `search_text`, `search_text_by`; `lineage_principals`, `lineage_key_ids` | part `body`, `rule`, `search_text`; the two lineage columns derived | the parts unsealed under the version's key, `search_text` staged rather than recomputed so an adapter change cannot drift it (d13); the lineage columns computed by `fold_of` from the rebuilt database, as the live fold computes them from the live one |
  | `insert_findings` | `scan_findings`, with `line`, `span_*`, `via`, `field` beyond the payload's one finding | part `findings` | loaded as staged |
  | the `KeyIssued` fold | `api_keys.hash` | part `key_hash`; new `key.issued` payloads gain `key_hash_digest` (decided 2026-11-14) | loaded and checked against the digest; a key issued before the digest is named by `lake.keys_unverifiable` |
  | the `WebEmailAllowed` fold, `web.signed_up` | `web_emails.email` | part `email` | checked against `email_digest` |
  | `web_pin` | `web_emails.google_subject`, `github_subject` | journal `web.subject_pinned` with a subject digest, plus part `subject` (decided 2026-11-14) | replayed; without it, the first sign-in after a rebuild would pin whoever presents that address (ADR-0045) |
  | `append_event`, `idempotency_claim`, `idempotency_settle`, `idempotency_release` | `idempotency` | part `idempotency` at the commit or the settle that records an outcome; an unsettled claim and a release are excluded | a recorded outcome is loaded; an unsettled claim is dropped, so a console act whose first commit landed and whose claim never settled can be submitted again, which duplicates that commit (decided 2026-11-14) |
  | `apply_meters`, inside commits | `account_meters`, `account_activity` | derived | the replay runs the same meters with no ceiling refused |
  | `meter_recalls`, outside commits | `account_activity.recalls`, and an empty `account_meters` row for an account that has only read (`lib.rs:6720`–`6728`) | `recalls`: state image (d2, decided 2026-11-14); the empty row: derived from the image, which names every account it counts | the newest image at or before the cut; an imaged account with no `account_meters` row gets the empty one |
  | `roll_up_usage`, outside commits | `usage_daily`, appended and pruned past its retention | state image (d2, decided 2026-11-14) | the newest image at or before the cut |
  | `enqueue_review`, `set_memory_status`, `promote_memory` | `review_queue` | derived | rebuilt by the replayed folds |
  | `review_claim_next` | `review_queue.held_by_key` | excluded | every claim is released; a reviewer claims again |
  | the `ClaimEdgeProposed` fold | `lineage_principals`, `lineage_key_ids` | derived | `fold_of` reads the closures from the rebuilt database as the live fold reads them, so replay order widens the same closures |
  | `sparse_*` and the postings trigger | `search_postings`, `search_term_rows`, the sparse queue and state | derived | the trigger and the sparse backfill run again (ADR-0052) |
  | `fold_code_issued`, `spend_code`, `void_unopened_codes` | `connect_codes` | excluded (d5) | rows replayed voided, with no hash |
  | `connection_connect`, `fold_connection_opened`, `fold_connection_revoked`, `sweep_expired` | `oauth_connections` | excluded (d5) | an opened connection replays revoked with cause `rebuilt` |
  | `insert_tokens`, `rotate_refresh` | `oauth_tokens` | excluded | none |
  | `authorization_insert`, `take_authorization_by_handle`, `oauth_client_insert`, `upsert_cimd`, `touch_cimd`, `insert_dcr`, `remove_unused_dcr` | `oauth_authorizations`, `oauth_clients` | excluded | none; a client registers or is fetched again |
  | `web_session_open`, `web_session_end`, `web_sessions_sweep`, `end_entry_sessions` | `web_sessions`, `web_session_credentials` | excluded | none; everyone signs in again |
  | `telemetry.rs` writers | `recall_events`, `recall_outcomes` | excluded | none; the lake's daily counts stay |
  | `align_pre_0017_statuses` | `workareas.status`, at startup | derived | the lead runs it on the rebuilt database |
  | the lake writer | `artifact_keys`, `lake_dead_letter` | `twin/keys`; dead letters excluded | loaded from `twin/keys` |

  The law (d12) is column-level over every table: each column of each table is in exactly one
  class, and a new column fails the build until it is classified.

- **d5 — Excluded families and what `fold_of` makes of them.** The deployment signing key never
  enters the lake; a rebuild signs nothing, and the rebuilt lead loads its keyring from its own
  custody or rotates (`system.signing_key_rotated`, unplanned, chain-format.md). For the
  journalled credential families, `fold_of` yields a fixed shape: a `connection.opened` replays as
  a connection revoked with cause `rebuilt` (a new value in `revoked_cause`); a
  `connection.code_issued` replays as a code voided by the rebuild, its hash null; no token row is
  ever written. A connector's holder asks an admin for a new code and connects again. Session keys
  (`credential: session`) replay from `key.issued` and hold no hash; the next sign-in opens a
  session under them as today. Agent keys keep working, because their hashes are a part (d4). The
  law `a_rebuilt_tenant_with_a_connector_has_its_connection_revoked_as_rebuilt_and_no_token`
  replays a tenant that opened a connection.

- **d6 — Sealing, custody, placement and R3.** Every part is sealed under a data key named by
  `key_ref`: a version's body, rule, search text, findings and attachments under that version's
  key, created when the submission's staged row is sealed (amends ADR-0060 d6, which created it at
  promotion); every other part under a key of its own, so a key hash, an address or a subject is
  erasable alone. `artifact_keys` gains `key_ref` for the keys that seal no version. Associated
  data binds tenant, `journal_seq` (or `stage_seq`), part and `key_ref`, so a row moved anywhere
  else opens nowhere.
  - **Custody (amends ADR-0060 d19).** The lake files, the KEK and PostgreSQL's backups are three
    custody points. An escrow copy of the KEK is kept apart from the lake and the backups, and the
    drill (d14) rebuilds with that escrow copy, so the escrow is proved, not assumed. Rotation
    re-wraps `twin/keys` in the same lease, and finishes only when no live key in either copy is
    wrapped by an older version.
  - **Placement.** `oml-lakewriter doctor` adds `lake.placement`: `fail` when the KEK file and the
    lake root share a device, `warn` when the lake root shares a device with PostgreSQL's data
    directory, when the operator names it with `--pg-data-dir`.
  - **R3 ([ADR-0001](ADR-0001-the-constitution.md) d3).** `bronze.events` is an analysis table
    the named queries read; it holds the signed journal, which by ADR-0002 d5 carries no body but
    does carry the signed metadata d7 names. The `twin/` prefix is not a consumer zone: no named
    query reads it, `oml-lakewriter query` refuses a path under it, and no recipe names it. A held
    body leaves the twin only into a rebuilt PostgreSQL, where the server applies R3 again. Amends
    ADR-0003 d3's first sentence to "no held body reaches a consumer zone"; its rule that
    unreviewed content never enters a consumer-readable zone stands. The boundary, stated: whoever
    holds the KEK and the lake files can open held text, as the PostgreSQL owner can read
    `memories.claim` today.

- **d7 — Erasure reaches the twin, and the signed metadata it cannot reach is named.** On
  `lifecycle.erase` the lead replaces the version's staged rows with tombstones (d3), and the writer destroys the
  version's key in `artifact_keys` and MERGEs its `twin/keys` row to destroyed; the ciphertext in
  `twin/inputs` stays and opens under no live key. A removed allowlist entry
  (`web.email_removed`) and a closed deletion request (`web.deletion_closed`) destroy the key of
  that person's `email` and `subject` parts the same way. On replay, the rebuild destroys every key
  whose erase it replays, whatever `twin/keys` holds, and reads `twin/keys` only at its current
  version, so a rebuild cut before an erase still never opens what was erased later. Amends
  ADR-0005 d4 with three inventory rows (`lake_staging`, `twin/inputs`, `twin/keys`), and ADR-0005
  d3 to name what crypto-shredding cannot reach: clear text inside signed payloads, today a
  version's `title`, a verdict's `reason` (`app.rs:6054`), a workarea's or key's `name`, an
  anchor's `uri`, `path` and `fragment`, a crew's `charter_uri` and `external_manager`
  (`app.rs:1323`), and, once ADR-0089 is built, an `admin.fix` reason. The admission scan reads the
  title at write (`app.rs:7423`) and the reason under the veto scan (`app.rs:5963`); the text is
  kept as signed metadata. Until #389 lands, the named queries read no `title` of a held
  submission from `bronze.events`, so held text does not surface in analysis through its title (decided by the operator, 2026-11-14).
  Moving titles to a digest with a sealed part is #389.

- **d8 — Each tenant drains on a frontier of its own (decided by the operator, 2026-11-14).**
  Amends ADR-0060 d3. The stable prefix stays global (ADR-0060 d4); within it, each tenant's
  batch runs alone and commits its tables in order: `twin/inputs`, `twin/keys`, `bronze.events`,
  `twin/state`, then `<tenant>/lake/frontier`, whose Delta `txn` version is that tenant's
  frontier. A tenant whose batch fails stops at its frontier and the others go on. Silver, gold
  and telemetry for the tenant commit after its frontier, may dead-letter as before
  (ADR-0060 d14), and never hold the frontier back. The provider tenant drains first in every
  tick; a tenant event that names a provider seq (`tenant.genesis`) waits until the provider's
  frontier passes it, so a stalled provider holds back only new tenants. The rule is inert until the provider's
  `tenant.created` is journalled: today `tenant.genesis` names no provider seq, so no tenant waits. `_provider/lake/batches`
  becomes a log of ticks (start, horizon, tenants advanced, tenants stalled), no longer a frontier.
  Staged rows are deleted per tenant, only up to that tenant's frontier. On resume, the writer
  reads each tenant's frontier from its own log and nothing else.
  - **Waivers.** `oml-lakewriter waive --tenant T --outbox-seq N --reason <text>` records, in
    `<tenant>/lake/waivers`, the event's id and hash, the parts not written, the reason and the
    time; the tenant then advances past that event with those parts absent. A waiver never skips a
    `bronze.events` row: a missing signed event breaks the chain, so a tenant whose bronze cannot
    take an event stays stopped until the cause is fixed. A rebuild refuses to pass a waived event
    without `--accept-waiver T:N`, and with it reports every row the waived parts would have
    filled.
  - Bronze, `twin/inputs`, `twin/keys` and `twin/state` never dead-letter (amends ADR-0003 d6 and
    ADR-0060 d14 for these tables).
  - The law `one_stalled_tenant_holds_no_other_tenant_back` stalls one tenant on a poison event
    and drains the others past it.

- **d9 — One rebuild verb, on the writer's binary, fenced and verified first.**
  `oml-lakewriter rebuild --into <owner URL of an empty database> --signing-key <public key file>
  --kek-file <file> --checkpoint <file> [--tenant <name or id>]… [--cut <outbox seq> | --cut-at
  <time>] [--drill] [--accept-waiver T:N]…`.
  1. **Fence.** It takes the writer's advisory lease on the target and a lock object
     (`_provider/lake/rebuild.lock`, put-if-absent) on the lake root, and writes a `lake_rebuild`
     marker row (`in_progress`); `run` refuses a target whose marker is not `complete`, and the
     lead refuses readiness on one that is `in_progress` (`rebuild.incomplete`). It refuses a lake
     whose table schemas are newer than its own build, and runs its own migrator, which is the
     lead's of the same release; the lead that serves the result is that release or newer.
  2. **Verify.** It reads every tenant's `bronze.events` up to that tenant's frontier (never past
     it, whatever bronze's Delta version holds) and up to the cut, and verifies each chain with
     chain-format.md steps 1 to 7 and 9 against the authenticated key set, before writing any row.
     Step 8 joins when the provider's `tenant.created` is journalled. Each tenant's verified head
     must reach the checkpoint's head for that tenant (d11).
  3. **Replay in global outbox order.** It commits events in ascending `outbox_seq` across tenants,
     each in one transaction: the journal row, the chain head, `fold_of(event, parts)` through
     `oml-pg`'s own fold code with metering applied and no ceiling refused, and an outbox row with
     the event's original seq. Parts staged outside a commit apply in `stage_seq` order between
     the events they fall between. Each part is checked against its payload digest where one
     exists.
  4. **Keys and state.** It loads `artifact_keys` from `twin/keys` as it stands, destroys every
     key whose erase it replayed, and loads the `twin/state` images. The law
     `a_replayed_erase_destroys_its_key_whatever_the_twin_keys_hold` holds the erase.
  5. **Sequence.** It sets `outbox_seq_seq` past the larger of the largest replayed seq and the
     largest tenant frontier, so the writer never skips a new row. `oml-lakewriter doctor` adds
     `lake.sequence`: `fail` when any tenant's frontier is past the database's outbox sequence. The
     law `a_rebuilt_database_takes_new_outbox_rows_past_every_frontier` holds it.
  6. **Finish.** It writes `rebuild.json` beside the target's marker, sets the marker
     `complete`, and releases the lock.

  **Cross-tenant conflicts (decided by the operator, 2026-11-14).** Tenants drain on separate frontiers, so a
  rebuild can replay one tenant behind another, and a deployment-wide unique value can collide:
  tenant A stalled before it removed an allowlist address, tenant B allowed the same address after.
  The rebuild then refuses by name, `rebuild.cross_tenant_conflict`, naming both tenants and both
  seqs, and writes nothing more; it offers a `--cut` at the stalled tenant's frontier, or a rebuild
  without that tenant (`--tenant` for the rest). The law
  `a_rebuild_refuses_when_a_stalled_tenant_still_holds_an_address_another_tenant_reused` holds it.

  **Idempotency.** The rebuild resumes from the target's chain heads: an event at or below a head
  must equal the stored row byte for byte. On `rebuild.diverged` the target did not come from
  this lake: the rebuild names the tenant and the first differing seq, changes nothing, and the
  operator rebuilds into a fresh empty database; the diverged one is kept for audit, never edited.

  The writer still holds no signing key (ADR-0006 d5). `oml-lakewriter` gains `oml-app` and
  `oml-pg` as dependencies, the direction ADR-0060 d18's law allows; the law still forbids the
  reverse. No server route or MCP tool reaches the verb (R1, ADR-0001 d1).

- **d10 — Cuts fork the lake; drills never feed it; a backup rolls forward (decided by the
  operator, 2026-11-14).**
  - A **disaster-recovery cut** (`--cut`, or `--cut-at`, which resolves to an outbox seq through
    bronze) also takes `--new-lake <root>`: the rebuild copies each tenant's twin tables up to the
    cut into the new root, sets each frontier there to the cut, and records an epoch in
    `_provider/lake/epochs` (epoch number, parent root, cut, time, reason). The writer then runs
    against the new root. The old root is kept read-only, never deleted. A full rebuild (no cut)
    keeps the root and the epoch.
  - A **drill** (`--drill`) builds a scratch database marked `drill`; `run` refuses a database so
    marked, so the writer never resumes against it.
  - **Rolling a backup forward.** `--onto <owner URL of a restored backup>` accepts a target whose
    every chain head and the lake's bronze for that tenant are prefixes of one another. Where the
    lake is ahead, the rebuild replays the suffix; where the backup is ahead, it replays nothing for
    that tenant and the writer re-drains the suffix from the backup, whose staged rows past the
    lake's frontier were never deleted, since a staged row is deleted only after the lake holds
    it. This is how the lake and `oml-backup` (#182) compose: the backup is the fast restore and
    the lake brings it forward, erase events included.

- **d11 — Signed head checkpoints are kept off the lake (decided by the operator, 2026-11-14).**
  The lead writes, on a schedule (hourly by default, decided by the operator, 2026-11-14), a checkpoint file: each
  tenant's head seq and hash, the time, and the deployment key's signature over domain-separated
  canonical bytes (`oml-head-checkpoint/1`). It is not a journal event, so it grows no chain. The
  file goes where the backups or the KEK escrow go, never under the lake root. A rebuild takes the
  newest checkpoint, prints its time, and refuses to finish below its head for any tenant
  (`rebuild.behind_checkpoint`) and names the gap; a tenant the checkpoint names and the lake lacks
  fails the rebuild the same way (decided by the operator, 2026-11-14); `--accept-loss` lets it finish and records the gap in `rebuild.json`. `oml doctor` adds
  `server.head_checkpoint`: `warn` past twice the interval, `fail` when none was ever written. The law
  `a_truncated_lake_is_caught_by_the_checkpoint` drops a lake's last commits and finds the
  rebuild refused.

- **d12 — Migrations and the fold law.** Every event folds into the current schema: `fold_of`
  reads every payload shape ever written, as the store does today (an event without `format` reads
  as `text/plain`). Two laws hold it: each use case's fold equals `fold_of` of its event and its
  staged parts; and the column-level classification of d4 covers every column of every table. The
  build adds migrations, numbers reserved when it starts: `lake_staging` (with the writer's
  SELECT and DELETE grant given at its start); `key_ref` on
  `artifact_keys`; `journal_seq` on `idempotency`; `rebuilt` in `oauth_connections.revoked_cause`;
  a null hash for a rebuilt `connect_codes` row; the `lake_rebuild` marker; and the
  `web.subject_pinned` fold.

- **d13 — Derived rows are recomputed, with their version inputs pinned or excluded.** The rows
  d4 classes derived are written by the same triggers, folds and backfills a live commit runs.
  `search_text` is staged rather than derived, so its adapter's version cannot change it. Kind-2
  sparse postings depend on the configured model; a rebuild recomputes them under the model the
  rebuilt server names, and the comparison (d14) excludes them unless both sides name the same
  model.

- **d14 — The proof is an equality, in CI and in the drill.** `oml-lakewriter compare --left <URL>
  --right <URL>` writes one digest per table over its canonical, ordered rows and names every table
  and column that differs. It skips the excluded families (d4, d5) and a closed list of columns
  that a rebuild cannot reproduce: `review_queue.held_by_key` and `held_since`; kind-2 postings
  under a different model; and every column set by `now()` at commit rather than from the event's
  `at`: the columns whose default is `now()` where the insert does not set them (`created_at` of
  `tenants`, `workareas`, `api_keys` and `outbox`, migration 0001), and
  `idempotency.committed_at` and `artifact_keys.created_at`. `memories.created_at` is different:
  it is a separate application clock read, `self.clock.now()` in the `MemoryRecord`
  (`app.rs:5428`), not the event's `at`. The build moves it to the event's `at` on the live
  insert and in `fold_of` alike, so both sides agree (decided by the operator, 2026-11-14); until then it is
  listed. The build moves each `now()` column it can to the event's `at`
  and lists the rest with a reason. The law
  `a_rebuilt_database_equals_the_database_it_was_drained_from` runs the demo workload on a
  throwaway cluster, drains, rebuilds into a second cluster and compares. The drill compares
  against a restored backup rolled forward to the same frontiers (d10), never against the live
  database, which moves; where no backup is at hand, the drill proves only that the rebuild
  completes, and equality is CI's.

- **d15 — The RPO of a lake-only rebuild is each tenant's lag.** Events PostgreSQL committed and the
  writer had not drained are lost when PostgreSQL is lost with no backup; `synchronous_commit`
  (ADR-0002 d2) does not shorten the lake's lag. `lake.rpo` reads the oldest undrained row over all
  tenants: `warn` past `--rpo-warn-seconds` (default 300), `fail` past `--rpo-fail-seconds`
  (default 3600), decided by the operator, 2026-11-14 as the starting values, revisited when #390 measures the lag. `lake.stalled` says how many
  tenants are stopped, never which (no tenant label).

- **d16 — Storage: the object_store port, append-only, no retention cut.** The twin uses the
  storage ADR-0060 d15 allows and moves to object storage with the same port; a target without
  conditional put stays refused. `bronze.events` and `twin/inputs` are never pruned: OPTIMIZE
  rewrites files and VACUUM removes only unreferenced ones. Ciphertext of erased parts stays,
  unopenable; dropping it would be a later, journalled decision.

- **d17 — Operability.** Metrics `oml_lake_twin_rows` and `oml_lake_twin_bytes` by table,
  `oml_lake_staged_rows`, `oml_lake_stalled_tenants`, `oml_lake_waivers`,
  `oml_lake_rebuild_events`, `oml_lake_rebuild_seconds`; no tenant label. Doctor lines:
  `lake.rpo`, `lake.stalled`, `lake.sequence`, `lake.twin` (per tenant, bronze rows up to the
  frontier equal the journal's), `lake.twin_mismatch` (a part that fails its digest),
  `lake.placement`, `lake.waivers`, `lake.staging_age`, `lake.drill` (`warn` with no drill younger than 35 days),
  `lake.keys_unverifiable` (keys issued before `key_hash_digest`), and `server.head_checkpoint`.
  The runbook gains "Rebuild PostgreSQL from the lake", "Roll a backup forward" and "Run a drill" in
  [lake operations](../operations/lake.md) and [restore](../operations/restore.md). Size per
  memory, write amplification, staging cost, lag and rebuild time are unmeasured: #390.

- **d18 — Every record that adds state names its twin parts.** ADR-0080 to ADR-0087 and ADR-0089,
  accepted the same day, add state this record had not classified. Each states its class per d4
  in a "Twin parts" line under its Release (PRs #375 and #377), with this record's reading as the
  default: skill and document claims (ADR-0080, ADR-0081) and console edits (ADR-0082) are
  `memories` rows, so their bodies are parts; the cockpit and the To do page (ADR-0083) and the
  impact report (ADR-0087) read only, and ADR-0083's withdrawal is a journal event; a bundle's
  import ledger (ADR-0084), release manifests and each key's binding (ADR-0085), test runs and
  gate results (ADR-0086) and an `admin.fix` reason (ADR-0089) are journal or part, never
  excluded; ADR-0084's per-import staging keys are excluded, transient and destroyed when the
  import ends, and ADR-0089's release overlay state is derived from the replayed fix events. ADR-0088, deferred, states
  its trusted-source pins the same way when it is accepted. The build names each column's class
  in the law of d12.

- **d19 — The constitution holds unchanged.** [ADR-0001](ADR-0001-the-constitution.md) R1 to R4 are
  not restated. The rebuild is an operator act no agent surface reaches (R1); it replays approvals
  the journal holds and decides none (R2); held text never reaches a consumer zone (R3, d6);
  nothing is deleted, a staged row leaves only once the lake holds it, and an erase stays an erase
  through every rebuild (R4, d7).

## Consequences

- PostgreSQL can be lost without losing the record: a rebuild returns every tenant to its frontier,
  verified against the public key and an off-lake checkpoint, at the cost of every session, token,
  connection and review claim.
- Accepted cost: an idempotency claim that never settled is dropped by a rebuild (d4), so a
  console act whose first commit landed before PostgreSQL was lost can be submitted again after the
  rebuild and commit a second time.
- The commit path gains staging inserts; the writer gains per-tenant batches and the twin tables;
  `oml-app` gains `fold_of`. Each cost is unmeasured (#390).
- The migrations of d12 ship with the build, and the lead gains the checkpoint writer.
- The KEK becomes the key to held text as well as approved text; its escrow is drilled.
- ADR-0005's d3 and d4 text, ADR-0003 d3's first sentence and the three status lines change in this
  change; the other amended decisions read with this record.
- The drift named in the context is not repaired here.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **The lake as the authority, PostgreSQL rebuilt as its cache** | ADR-0002's dual-write argument still decides; the operator kept PostgreSQL as the commit point. |
| **Copy table images instead of replaying events** | Images cannot stop at a seq, carry no signature and copy a corrupt index faithfully. Images are kept only for the counters no event reproduces (d2). |
| **Read fold inputs at drain time** | The first draft's choice. It depends on every input being immutable and still present, and loses a row a later event removes before drain. Staging in the commit (d3) holds what the commit held. |
| **Put fold inputs in the journal payload** | Bodies and addresses in signed bytes cannot be erased (ADR-0002 d5, ADR-0005 d2). |
| **One global frontier, as built** | One poison event stops every tenant (d8). |
| **Skip a poison event without a record** | A silent gap. A waiver is recorded in the lake and a rebuild stops at it unless told (d8). |
| **Resume the writer against a database cut to an earlier point** | Its frontier is past the cut, so it would skip every new row; a cut forks the root (d10). |
| **Rebuild from silver** | State at drain time, approved versions only, a consumer zone. |
| **Keep the wrapped keys only in PostgreSQL** | Then the lake opens nothing once PostgreSQL is gone. |
| **Re-issue every API key after a rebuild** | Kept hashes authenticate nothing alone; re-keying every agent after an outage multiplies it. The operator kept the hashes on 2026-11-14 (d4). |
| **Trust the lake's own last version as complete** | A truncated lake verifies as a consistent prefix; only a head kept elsewhere shows the gap (d11). |
| **Time-travel the key table for a cut** | It would resurrect keys erased after the cut (d7). |
