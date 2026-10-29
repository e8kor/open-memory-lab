# ADR-0023 — Three install profiles, one conformance bar; cluster-mode lead/follower

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. It makes two connected moves: the
Kubernetes-only deployment story of [ADR-0016](ADR-0016-kubernetes-workloads.md) becomes one of
three first-class install profiles, and `oml-server` gains a cluster mode — one lead, N
followers — that supersedes the every-replica-signs design. Every guarantee a profile weakens is
stated in [commitments](../product/commitments.md), and every unmeasured claim carries a roadmap
handle ([FR-22, FR-23, FR-24, RS-12, RS-13](../product/roadmap.md)).

## Context

The requirement: OML must install as a very performant memory provider on a single host
(standalone), under docker compose, and on Kubernetes — and `oml-server` must run in a cluster
mode with lead and follower roles, lead-only by default. The provider ring
([ADR-0022](ADR-0022-two-ring-api.md)) is what the profiles serve: an agent that adopts OML as
its only memory should not need an orchestrator to get it.

The category's evidence says this requirement is also the positioning. Zep's Community Edition
(clone + `docker compose up`) is deprecated and unsupported per Zep's own FAQ; Letta's Docker
image is, per current Letta docs, "no longer an actively maintained or supported product
surface"; Redis Agent Memory Server self-hosting is private preview, license-gated, Kubernetes +
Helm only. Nobody in the category ships a maintained standalone/docker/kubernetes triad; the two
that had Docker stories retreated to cloud, and Redis gates self-hosting entirely. (Fetched
2026-10-29; the dated one-liners live in [positioning](../product/positioning.md).)

Two technical findings make the standalone profile honest rather than hopeful:

- **F1 — `object_store`'s `LocalFileSystem` supports conditional put, atomically.** The crate
  (0.14.2) defines `PutMode::Create` as an atomic write returning `AlreadyExists` when the
  object exists, implements `copy_if_not_exists`/`rename_if_not_exists`, and documents its
  staged-file-plus-rename mechanism (filenames matching `/.*#\d+/` are reserved "to provide
  atomic writes"). The caveat travels with the claim: this atomicity is a POSIX-local-filesystem
  property; the docs promise nothing for network filesystems, so the standalone profile must
  refuse NFS/SMB data directories (d3).
- **F2 — delta-rs needs no locking provider on `file://`.** Its storage support table lists
  Local as fully supported with no lock requirement; only S3 requires one (or conditional PUT).
  The second fence of ADR-0016 d3 — put-if-absent on the next Delta log entry — is therefore
  native on local disk, same shape as S3/MinIO. The `SetTransaction` exactly-once mechanism is
  Delta log content, not backend behavior; the only backend-dependent part is commit atomicity,
  which F1 covers. The [FR-3](../product/roadmap.md) harness simply runs per backend.

ADR-0016 rejected docker-compose — but as a *rehearsal of the Kubernetes deployment*, and that
argument stands. What changed is the requirement: docker and standalone are now targets in their
own right, and the drift risk that rejection priced is answered by a conformance suite that must
pass on all three profiles ([FR-22](../product/roadmap.md)), not by refusing the payloads.

## Decision

- **d1 — Three first-class profiles, one system.** `standalone` (single host, no orchestrator),
  `docker` (one compose file), `kubernetes` (the existing chart, now one of three). Identical in
  every profile: the four processes (the server counted once — its lead/follower compositions
  are [ADR-0006](ADR-0006-pure-rust.md) d6's), the Postgres schema and journal, the commit protocol, the
  constitution, the bootstrap ceremony, the CLI, and the conformance suite (FR-22). Profiles
  differ only in (a) who supervises the processes, (b) what backs the object-store and KMS
  ports, and (c) which guarantees weaken — each weakening stated in
  [commitments](../product/commitments.md), never discovered. The deployment principle changes
  from "the service is a set of workloads on Kubernetes" to **"the service is the same four
  processes wherever they run; the profile chooses their supervisor and their stores"**
  ([deployment](../operations/deployment.md)).

- **d2 — Standalone runs the four real binaries under one supervisor process — not merged into
  one binary, not four hand-managed units.** `oml standalone up` is a small supervisor that
  forks `oml-server`, `oml-lakewriter`, `oml-reconciler` and `oml-indexer` as child processes,
  restarts them with backoff, and propagates shutdown. Why not a single merged process:
  [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d3's point is that keyless is a
  type error because the lake-writer *composes no KMS port* — merging the binaries would keep
  the type-level claim but erase the OS-process custody boundary the
  [security](../architecture/security.md) "never" columns describe, and would add a fifth
  composition that drifts from the four that run everywhere else. Why not systemd units as the
  primary story: Linux-only, needs root, turns the quickstart into a sysadmin task, and needs a
  timer unit kept in sync with the reconciler's schedule. The supervisor keeps a three-command
  quickstart on any Linux/macOS box; `oml standalone systemd` *generates* unit files (per-role
  `User=` lines) for operators who want init-system supervision — offered, not default.
  Singleton honesty: the supervisor runs exactly one lake-writer, but — exactly as in
  Kubernetes — the supervisor is the lease-equivalent *optimization*; the load-bearing fence
  remains Delta conditional put, which F1/F2 confirm works on `file://`. Two accidental
  supervisors get refused commits, not duplicates.

- **d3 — Standalone backing: host Postgres, local-filesystem object store, local Delta
  tables.** Postgres is bring-your-own (a host service), pinned major version, pgvector
  required; `oml doctor` verifies the version, the extension, the C6 role split and
  `FORCE ROW LEVEL SECURITY`. The object-store port is `object_store`'s `LocalFileSystem`
  rooted at one data directory (`<data-dir>/quarantine`, `<data-dir>/bodies`,
  `<data-dir>/lake/<tenant>/…` — the same per-tenant path shapes as S3 prefixes). Delta tables
  live on local disk; commits use native put-if-absent (F1/F2). Hard constraint, enforced, not
  advised: **the data directory must be a local POSIX filesystem** — startup runs the
  conditional-put scratch-key self-test (ADR-0016 d5's) and refuses NFS/SMB mounts, because F1's
  atomicity promise stops at the local filesystem.

- **d4 — KMS becomes a software keyring adapter (`kms-file`) in standalone and docker.** A
  file-held KEK behind the existing KMS port: wrap/unwrap in-process, the KEK in a 0600 file,
  optionally passphrase-protected at rest (age/scrypt — pure Rust, satisfying
  [ADR-0006](ADR-0006-pure-rust.md); OS keychains rejected: headless servers). What survives:
  the crypto-shredding mechanics of [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) —
  destroy the wrapped DEK; KEK rotation re-wraps live DEKs; retired-KEK destruction is file
  deletion, journalled. What collapses, said plainly: "the KEK never leaves KMS" becomes "the
  KEK is a file on the host"; encrypt-only versus decrypt *grants* no longer exist as an
  external IAM fact — C7's keylessness survives **only** as composition (the lake-writer has no
  KMS port) plus whatever OS boundary the profile gives (file permissions in standalone, mount
  scoping in docker). The keyring file is a new copy class in the
  [erasure inventory](../architecture/audit-and-lineage.md): any backup of it extends the
  erasure SLO exactly as wrapped-DEK copies in Postgres backups do. The exact keyring mechanism
  is [RS-12](../product/roadmap.md).

- **d5 — Standalone supports the full feature set; nothing is excluded, only guarantees
  weaken.** Lake, reconciler, indexer, simulations, erasure, evidence export — all run.
  Reconciliation on local disk stays meaningful because its job is journal↔lake *consistency*:
  it catches writer bugs and divergence, which do not care where the bytes live. A "lite"
  standalone without the lake was rejected (below): it forks the conformance surface and makes
  standalone a different product rather than the same product with weaker custody.

- **d6 — Reconciler scheduling: the binary grows `--once` and `--every` modes; the CronJob
  becomes one of three schedulers.** Kubernetes keeps the CronJob running `--once` (unchanged:
  `concurrencyPolicy: Forbid`, `activeDeadlineSeconds`). Standalone and docker run the
  reconciler long-lived in `--every <interval> --pass-deadline <d>` mode: sequential passes in
  one process give Forbid semantics by construction, and an internal pass timeout gives the
  deadline semantics — a killed pass redoes work, never corrupts it, which the cursor design
  already guarantees. Rejected: host cron and cron-sidecar containers — a second scheduling
  payload to keep in sync, and compose has no native cron.

- **d7 — Docker profile: one compose file, MinIO by default, software keyring, a one-shot
  bootstrap service.** The shape:
    - `postgres` — `pgvector/pgvector:pg17` (the pgvector project's own image, pinned to a
      major tag — the pattern real-world compose files use; per the image's own notes (verify at
  writing time), Postgres 18 images changed the
      data-directory layout, so the pin matters for volume mounts). Volume-backed, healthcheck
      `pg_isready`. Two roles created at init: the migration/owner role and the service role —
      C6's split is a profile invariant, not a Kubernetes nicety.
    - `minio` — the **same pinned tag the Helm chart pins**: the `If-None-Match` pin is one
      fact declared once, consumed by chart and compose. A documented `file:` variant (shared
      volume + `LocalFileSystem`) exists for minimal installs and inherits standalone's honesty
      text. MinIO is the default because it rehearses the real store path, keeps the
      lake-writer's conditional-put self-test identical to production, and keeps per-prefix
      credentials expressible.
    - `oml-bootstrap` — one-shot (`restart: "no"`): runs migrations under the owner role, mints
      the keyring (d4) and the single-use enrollment secret to a named volume; every other
      service gates on `service_completed_successfully`. The compose twin of the Helm hook Job —
      same ceremony, same show-once claim.
    - `oml-server` — the only service that mounts the keyring and the signing key; healthcheck
      on `/readyz`, fork check included, unchanged.
    - `oml-lakewriter` — no keyring mount, no signing-key mount, its own Postgres credentials.
      One replica; the conditional-put fence backs it exactly as in Kubernetes.
    - `oml-reconciler` — `--every` mode (d6), read-only credentials, no key-material mounts.
    - `oml-indexer` — its own credentials, no key mounts.

    Middle-profile honesty, stated where the compose file is documented: custody separation
    exists *between containers* — the lake-writer container genuinely cannot read the keyring,
    because it is not mounted — but one docker host is one trust domain: root on the host reads
    every volume, every secret, and the keyring. Resource notes ship as compose
    `deploy.resources` comments, not claims — the no-invented-numbers rule applies.

- **d8 — One configuration story across three profiles; "profile" is the deployment payload,
  not a runtime flag.** The binaries stay profile-blind: each reads the same configuration
  surface (file + env) naming its adapters per
  [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) — Postgres URL, object-store URI
  (`s3://…` | `file:///…`), KMS adapter (`aws` | `file`), scheduler mode, node role (d13 —
  i.e. which of the two server compositions of
  [ADR-0006](ADR-0006-pure-rust.md) d6 the payload deploys; never a flag that grants the
  signer port). A
  profile is a named bundle of those choices shipped as the payload native to each world: Helm
  values (`local`/`scaled`), the compose file, `oml-standalone.toml`. There is **no `--profile`
  flag** to drift from the payload. ADR-0019's journalled settings apply unchanged: every
  effective configuration is recorded by digest (`system.settings_applied`), so "which adapters,
  which profile, when" is always answerable from the journal — in all three profiles
  identically. `oml doctor` grows per-profile checks (quorum settings in scaled Kubernetes; the
  MinIO pin in docker; local-fs atomicity, pgvector and the role split in standalone —
  [observability](../operations/observability.md)).

- **d9 — This record is the install-profiles decision; ADR-0016 is scoped, not rewritten.**
  ADR-0016's rejected alternative "docker-compose as the local story" argued against compose as
  a rehearsal of the Kubernetes deployment, and that argument stands. The requirement changed:
  docker and standalone are first-class targets, and the drift risk the rejection priced is
  answered by FR-22's conformance bar, not by refusing the payloads. ADR-0016 gains a scope
  line ("this record governs the Kubernetes profile") and a pointer from its compose rejection
  here; its d1–d5 are otherwise untouched, except that d13 below supersedes the
  every-replica-signs portion of its d4.

- **d10 — The upgrade path standalone → docker → kubernetes: same journal, three mechanical
  moves, one ceremony.** (1) Postgres: `pg_dump`/restore — and a restore is already an operator
  ceremony in [deployment](../operations/deployment.md): fork check, re-apply newer erase
  events, full-genesis reconciliation before readiness; the migration inherits that ceremony
  wholesale. (2) Lake and objects: copy the Delta table directories and the quarantine/bodies
  prefixes into the bucket — Delta add-actions are table-relative paths by default, so the log
  survives relocation, and the full-genesis reconciliation pass is the verification that it
  did. (3) Keys: the signing key material moves into a Secret/KMS unchanged — deployment
  identity continues, the chain does not restart — and the KEK migrates by running the
  *existing* KEK-rotation machinery: re-wrap all live DEKs under the new KMS KEK, then
  journalled destruction of the file KEK. What honestly does not transfer: **history's
  guarantees.** Writes accepted under standalone were one-disk-durable and host-admin-custodied
  *when accepted*; migration upgrades the future, not the past, and the evidence export says
  under which profile (settings digest) each era ran — journalled settings make that a query,
  not archaeology. The rehearsal is [FR-23](../product/roadmap.md).

- **d11 — Critical path: standalone is M1's profile; compose lands at M2; the Kubernetes local
  profile moves to M3.** Decided here, flagged as a critical-path edit: M1's exit
  ("one-commit-point and chain-order semantics demonstrated") needs no orchestrator —
  standalone is the cheapest end-to-end rig and becomes the dev profile the whole build runs
  on. Compose joins M2, the provider-floor milestone, when early users first want a trivial
  install. The k3d/Helm local profile moves to M3, the first milestone that exercises the
  Kubernetes-specific machinery (lease, CronJob) against the lake; M0 spike 2 already tests
  MinIO conditional put without a chart, and M0 spike 1 extends to run the FR-3 harness on
  `file://` as well. M6's scaled profile is unchanged. This edits M1's exit criterion; the
  [critical path](../product/critical-path.md) carries the change as of this record's date.

- **d12 — Positioning.** One sentence in [positioning](../product/positioning.md), backed by
  the dated category evidence above: *OML installs like the category used to promise — one
  host, one compose file, or one chart — and governs unlike it; same binaries, same journal,
  same conformance suite on all three.*

### Cluster mode (d13–d17)

- **d13 — Two node roles for `oml-server`; the N-identical-replica design is superseded.**
  `role: lead` owns the write path: it loads the deployment signing key, holds KMS decrypt and
  generate-data-key, runs admission, signs journal events. `role: follower` serves the read
  path (recall, get, search, evidence-export reads), holds KMS decrypt for the serving path,
  **never loads the signing key**, and forwards every write verb to the current lead over
  HTTP. This replaces "every replica loads the same signing key"
  ([security](../architecture/security.md), ADR-0016 d4): read capacity scales by followers;
  write capacity is one lead, and that is a stated cost, not an accident — per-tenant write
  throughput was already bounded by the chain lock
  ([deployment](../operations/deployment.md) says so), so the lead ceiling mostly renames an
  existing bound. The honest residual: cross-tenant *aggregate* write throughput, which N
  signing replicas could spread, now funnels through one node —
  [FR-24](../product/roadmap.md), next to FR-7's unmeasured ceiling.

- **d14 — Lead election is a Postgres lease, and the lease is never load-bearing — the chain
  lock is.** Exactly the lake-writer precedent: no Raft, no external coordinator; Postgres is
  the authority. A lead-eligible node acquires a lead-lease row in Postgres (holder id,
  address, epoch, expiry; renewed on heartbeat) and only then takes traffic. Discovery is the
  same row: followers read the lead's address from Postgres and forward writes to it — no DNS
  tricks, no gossip. The fence hierarchy mirrors ADR-0016 d3: the lease keeps the common case
  quiet, but correctness never depends on it, because the per-tenant `pg_advisory_xact_lock` on
  the chain head (C2, [commitments](../product/commitments.md)) already serializes every
  journal append — two split-brain leads cannot fork a chain; they merely contend on the lock.
  **C2's chain lock now has two dependents that are allowed to be the last line: the
  lake-writer fence and the lead fence.** Failover = lease expiry, takeover by a lead-eligible
  node, the startup fork check (unchanged), then a **journalled `system.lead_acquired` event
  signed by the new lead as its first act** — the journal always answers "which process was
  lead when" ([event catalog](../data/event-catalog.md)). The role itself is configuration and
  rides `system.settings_applied` (d8); the takeover is a runtime event, hence its own event
  type rather than a settings digest.

- **d15 — In-flight writes across failover: atomic by construction, retried by idempotency.**
  A write is durable iff its single Postgres transaction committed (C1), so a dying lead leaves
  no partial writes — only unanswered clients. A follower forwarding a write that gets no
  answer surfaces the same retryable error a client would see, and the existing idempotency
  machinery (the facade's idempotency keys, [ADR-0014](ADR-0014-http-api-and-mcp-facade.md);
  per-item batch idempotency, [FR-12](../product/roadmap.md)) makes the retry safe. No write
  queue lives on the lead; nothing drains on takeover. Lease parameters — TTL, heartbeat,
  takeover latency, which together set the availability window during which writes are
  refused — are unmeasured and ship as profile defaults; [FR-24](../product/roadmap.md)
  measures them.

- **d16 — Defaults: lead-only everywhere; followers are scale-out, never a prerequisite.**
    - **standalone** — lead-only, always. `role` is not even exposed in `oml-standalone.toml`;
      a follower on the same host adds custody theater, not capacity. A standalone node is
      simply always the lead of a cluster of one.
    - **docker** — lead-only by default. The compose file ships an `oml-server-follower`
      service under a compose profile (`--profile followers`, `deploy.replicas: N`): no
      signing-key mount, the serving-path unwrap credential only, `role: follower` in its
      environment.
    - **kubernetes** — two Deployments: `oml-server-lead`, replicas 1, lease-guarded (rolling
      updates briefly yield two lead-eligible pods; the lease picks one, exactly like the
      lake-writer), with the signing-key Secret mounted here **only**; `oml-server-follower`,
      replicas N with HPA on the read path, default **0** — so a default install is lead-only
      in all three profiles. The Service story: one client-facing Service selecting both
      Deployments (followers forward writes internally), so clients keep a single endpoint and
      no client ever routes by role.

- **d17 — Read-log writes in cluster mode: followers append, the lead anchors.** Reads are
  logged with per-batch digests anchored into the journal
  ([audit and lineage](../architecture/audit-and-lineage.md)). Followers append read-log rows
  directly — a non-journal table, under their own RLS-scoped credentials; anchoring a batch
  digest is a journal act and therefore the lead's, on the lead's cadence. The consequence,
  stated honestly: the unanchored-tail exposure window (already a §2 bound with no committed
  cadence) now also covers follower-written rows awaiting the lead's anchor — the same bound,
  one more sentence. The exact split is [RS-13](../product/roadmap.md).

## Consequences

- **The per-profile guarantee downgrades are stated, not discovered**: the contract sentence
  and the durability/KEK/tamper-evidence/erasure summary live in
  [commitments](../product/commitments.md)' "Profile-dependent bounds" subsection together
  with the profile-marked §2/§4 bullets (availability singletons, rate limiting, the
  lead/follower/host compromise admissions); the per-profile mechanics — what backs each
  claim in each profile — live on the [deployment](../operations/deployment.md) page's
  profile sections and in [security](../architecture/security.md)'s "Custody by profile"
  table. Construction-tier commitments are profile-independent,
  with one made explicit: C6's role split binds in **all three** profiles; compose and
  standalone do not get to skip it.
- **Roadmap handles opened**: [FR-22](../product/roadmap.md) (profile conformance — the same
  suite green on all three, FR-3 run per backend including the NFS-refusal check),
  [FR-23](../product/roadmap.md) (migration rehearsal), [FR-24](../product/roadmap.md) (lead
  failover semantics, measured), [RS-12](../product/roadmap.md) (keyring mechanism),
  [RS-13](../product/roadmap.md) (read-log split).
- **`system.lead_acquired` joins the [event catalog](../data/event-catalog.md)**; the keyring
  file joins the [erasure inventory](../architecture/audit-and-lineage.md) as a copy class for
  the non-KMS profiles; the custody map in [security](../architecture/security.md) gains a
  follower column, and its "every replica" paragraph is rewritten to "the signing key lives
  only in the lead, at most one, lease-guarded, takeover journalled".
- **The honest costs, in one place**: standalone's tamper-evidence against the host admin
  rests entirely on externally retained chain heads (RS-9); the docker host is one trust
  domain whatever the mounts say; cross-tenant write throughput funnels through one lead; and
  a lead takeover refuses writes while reads keep flowing — the window is unmeasured until
  FR-24.
- **d11 is a critical-path edit** (M1 standalone, M2 compose, M3 Helm local), applied in
  [critical path](../product/critical-path.md) together with this record.

## Rejected alternatives

- **One merged binary for standalone.** Keeps ADR-0019's type-level keylessness but erases the
  OS-process custody boundary the security page's "never" columns describe, and adds a fifth
  composition that drifts from the four that run everywhere else.
- **systemd units as the primary standalone story.** Linux-only, needs root, turns a
  three-command quickstart into a sysadmin task, and needs a timer unit kept in sync.
  `oml standalone systemd` generates units for operators who want them — offered, not default.
- **A cron sidecar (or host cron) for the reconciler in compose.** A second scheduling payload
  to keep in sync with the chart's CronJob settings, and compose has no native cron; `--every`
  mode gives Forbid-and-deadline semantics inside the binary itself.
- **A "lite" standalone without the lake.** Forks the conformance surface and makes standalone
  a different product rather than the same product with weaker custody; reconciliation's
  journal↔lake consistency checks are exactly as meaningful on local disk.
- **Raft or an external coordinator for lead election.** Postgres is already the authority for
  every correctness-bearing order in the system; a second consensus system is a second truth to
  keep consistent, bought to optimize a path the chain lock already serializes.
- **Keeping N signing replicas as the scale path.** Buys cross-tenant write spread, costs N
  copies of the signing key — and the custody narrowing (one signer, at most one, takeover
  journalled) is the point of cluster mode, not a side effect.
- **Postgres read replicas instead of followers.** Moves the read path off the fold tables'
  consistency story onto replication lag with no custody win — a follower is an OML process
  under OML's RLS and read-log discipline; a read replica is neither.
