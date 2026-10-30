<!--
id: operations/configuration
tier: engineering
status: design draft for review · decided, not built
answers: what the configuration surface is; how file and env compose; which settings exist, their scope and defaults; journalled settings vs boot config
-->

# Configuration

**Status:** design draft for review · decided, not built

This page is the single declaration site for the configuration surface that
[deployment](deployment.md) commits to: one file plus environment variables,
read identically by every binary in every profile. It declares the file
format, the precedence rule, the environment naming convention, which keys
each binary reads, and the reference table of every setting the design names.
The semantics of each setting stay with the page that owns them — this page
declares the key, its scope and its default, and links out; a second
description here would be the drift [ADR-0018](../adr/ADR-0018-the-documentation-set.md)
d5 exists to prevent.

## One surface: file plus environment

The configuration file is TOML, named **`oml.toml`** by default; every binary
accepts `--config <path>`. A deployment payload may ship the file under a
profile-specific name — the standalone profile's payload is
`oml-standalone.toml` ([deployment](deployment.md)) — but it is the same
document with the same keys; the name is a path choice, not a dialect.

Precedence is **defaults < file < environment**: an environment variable
overrides the file value for the same key. The reason is the shape of the
payloads: Helm values and compose files inject environment naturally, while
the file is the base document an operator writes and reviews. There is no
third surface — no setting rides a command-line flag. The two exceptions are
`--config` itself and the reconciler's scheduler-mode flags
(`--once` / `--every` / `--pass-deadline`), which choose a scheduler rather
than carry a value and are declared with the scheduling payloads in
[deployment](deployment.md).

### Environment naming

`OML_` + the key's dotted path in SCREAMING_SNAKE, with **double underscore
(`__`) as the nesting separator**:

```
search.fts_config        →  OML_SEARCH__FTS_CONFIG
postgres.url             →  OML_POSTGRES__URL
uploads.max_attachment_size → OML_UPLOADS__MAX_ATTACHMENT_SIZE
```

The separator is doubled because key segments themselves contain single
underscores (`fts_config`, `lease_ttl`); a single-underscore separator would
make `OML_SEARCH_FTS_CONFIG` ambiguous between `search.fts_config` and
`search_fts.config`. The mapping is mechanical in both directions, so a key
named in an error message or in `oml settings show` resolves to exactly one
environment variable.

## Boot configuration versus journalled settings

The key space is one, but keys apply through two routes, and the distinction
is load-bearing:

- **Boot configuration** is what a process must know before it can reach
  Postgres, or what names which process it is: the adapter bindings of
  [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) — Postgres
  URL, object-store URI, KMS adapter — plus node role, listen addresses, and
  lease parameters. Boot configuration is read from file + env at process
  start and changes by restart.
- **Journalled settings** are everything else: values validated and applied
  through `oml settings apply`
  ([CLI reference](../cli/reference.md)), journalled as
  `system.settings_applied`
  ([event catalog](../data/event-catalog.md)), and taking effect without
  restart. Plugin selection — which compiled-in scanner, embedder and
  exporter implementation is active — is journalled by
  [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d4, so
  "which plugin was active when" is always answerable from the journal.

The file may carry journalled-setting keys too: at first install the
bootstrap workload applies the file's journalled keys as the initial settings
document, so a deployment is described by one reviewed file. After bootstrap,
a journalled key edited in the file or environment does **not** take effect
on restart — it surfaces as divergence in `oml settings show --effective` and
in doctor's `settings_drift` field
([observability](observability.md)) until an operator applies it. Silent
pickup of an edited file is exactly the drift the journalled route exists to
prevent: the journal, not the filesystem, says what the deployment is
running.

Two more facts bound this page:

- **Every effective configuration is recorded by digest** in
  `system.settings_applied` — boot configuration at startup, journalled
  settings at apply — in all three profiles identically
  ([deployment](deployment.md),
  [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)).
- **A retired key is refused by name.** `oml settings validate` rejects a
  document naming a retired settings key and names its replacement; unknown
  keys are never silently ignored ([CLI reference](../cli/reference.md)).

A third class of configuration exists and is deliberately **not** on this
surface: **journalled tenant policy** — per-workarea review policy objects,
quota and rate-limit grants, the read-log retention window. Those are
tenant-scoped records changed through the API and CLI by tenant admins, not
deployment values; they appear in the reference table below only so the key
space reads complete, and their semantics stay with their owning pages.

## Per-binary applicability

Which binary reads which boot-configuration group. A binary that does not
compose a port has no key for it — the lake-writer has no decrypt
credential to configure, which is the custody design, not an omission
([security](../architecture/security.md)).

| Boot group | `oml-server` (lead) | `oml-server` (follower) | `oml-lakewriter` | `oml-reconciler` | `oml-indexer` | bootstrap |
|---|---|---|---|---|---|---|
| `postgres.*` (service role URL) | yes | yes (read role) | yes | yes (read-only role) | yes | owner role URL instead |
| `object_store.*` | yes (quarantine + bodies) | serving reads | yes (lake prefixes) | yes (read, digest verification) | no | no |
| `kms.*` | yes (wrap + decrypt) | decrypt only, serving path | wrap/encrypt only — never decrypt | no | per [security](../architecture/security.md) | keyring mint (`kms-file` profiles) |
| `server.role`, signing-key location | lead only | `role: follower`, no key material | n/a | n/a | n/a | n/a |
| `server.listen` / probe port | yes | yes | probe port | probe port | probe port | n/a |
| `cluster.lease_*` | yes | lead-eligible only | yes (lake-writer lease) | no | no | no |
| `reconciler.*` | no | no | no | yes | no | no |

The standalone profile does not accept `server.role` at all — a standalone
node is always the lead of a cluster of one
([deployment](deployment.md)).

## Settings reference

The key spellings below are declared here; everywhere else names them by
reference. The **Default** column states a value only where the design has
already bound one; every other shipped default is pinned by the conformance
suite — written as "stated at [FR-22](../product/roadmap.md)" — because this
design states no number it has not committed to
([ADR-0018](../adr/ADR-0018-the-documentation-set.md) d3).

### Boot configuration (file + env, read at process start)

| Key | Scope | Default | Semantics |
|---|---|---|---|
| `postgres.url` | deployment | none — required | The service-role connection; per-workload roles in [deployment](deployment.md) |
| `postgres.owner_url` | deployment | none — required at install and upgrade | The owner/migration role the bootstrap workload uses to run `sqlx migrate` (forward-only, `NNNN_name`) on first install **and on every upgrade**; no service workload holds it (C6, [security](../architecture/security.md), [postgres-schema](../data/postgres-schema.md)) |
| `object_store.uri` | deployment | none — required | `s3://…` or `file:///…`; the standalone local-fs constraint is enforced at startup ([deployment](deployment.md)) |
| `kms.adapter` | deployment | per profile payload | `aws` or `file` ([deployment](deployment.md), [security](../architecture/security.md)) |
| `kms.keyring_path` | deployment | per profile payload | `kms-file` profiles only; custody and backup consequences in [deployment](deployment.md); mechanism [RS-12](../product/roadmap.md) |
| `server.role` | deployment | `lead` | `lead` or `follower`; never a flag that grants the signer port; not accepted in standalone ([deployment](deployment.md), [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)) |
| `server.signing_key_path` | deployment | per profile payload | Mounted on the lead only — the custody boundary [deployment](deployment.md) draws |
| `server.listen` | deployment | stated at [FR-22](../product/roadmap.md) | API listen address; the other binaries expose `/healthz` + `/readyz` on a local probe port ([deployment](deployment.md)) |
| `cluster.lease_ttl`, `cluster.lease_heartbeat` | deployment | profile default — unmeasured, tracked as [FR-24](../product/roadmap.md) | Lead lease and lake-writer lease parameters ([deployment](deployment.md)) |
| `reconciler.every`, `reconciler.pass_deadline`, `reconciler.genesis_deadline` | deployment | profile default — unmeasured; measurement rides [FR-7](../product/roadmap.md)/[FR-18](../product/roadmap.md) | Scheduling and deadlines for incremental and full-genesis passes ([deployment](deployment.md)) |

### Journalled deployment settings (`oml settings apply` → `system.settings_applied`)

| Key | Scope | Default | Semantics |
|---|---|---|---|
| `plugins.scanner`, `plugins.embedder`, `plugins.exporter` | deployment | stated at [FR-22](../product/roadmap.md) | Which compiled-in implementation is active behind each port; journalled so the answer is always in the journal ([ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d4) |
| `search.fts_config` | deployment | `simple` | The Postgres text-search configuration `tsvector` rows are built with. `simple` is the v1 default because it stems nothing and so lies about nothing; the multilingual recall cost, and the reindex a change implies, are stated where search owns them ([retrieval](../architecture/retrieval.md)) |
| `idempotency.retention` | deployment | floor **24 hours** — committed | How long a `committed` idempotency record replays its stored response. The setting may only **raise** the floor; a replay after expiry is processed fresh, and that expiry-then-reprocess behavior is documented, not an anomaly ([HTTP API](../api/http-api.md)) |
| `api.cursor_ttl` | deployment | stated at [FR-22](../product/roadmap.md) | List cursors are positional and expire; a stale cursor answers `request.validation` ([HTTP API](../api/http-api.md)) |
| `api.page_limit_max` | deployment | **200** — shipped default (request default 50) | Ceiling on the list page `limit` ([HTTP API](../api/http-api.md#limits)) |
| `submission.max_body_size` | deployment | **1 MiB** — shipped default | Claim body (text) size bound ([HTTP API](../api/http-api.md#limits)) |
| `submission.max_attachments` | deployment | **64** — shipped default | Attachments per memory ([HTTP API](../api/http-api.md#limits)) |
| `uploads.part_size_min`, `uploads.part_size_max` | deployment | **5 MiB / 64 MiB** — shipped defaults | Bounds on the part size an upload session hands out ([HTTP API](../api/http-api.md#limits)) |
| `uploads.max_attachment_size` | deployment | **64 MiB** — shipped default | Upper bound on `size_bytes` at session creation ([HTTP API](../api/http-api.md#limits)) |
| `uploads.session_expiry` | deployment | stated at [FR-22](../product/roadmap.md) | Upload sessions expire; the window ([HTTP API](../api/http-api.md)) |
| `bodies.placement_threshold` | deployment | open — pinned at [FR-15](../product/roadmap.md) | The lake-inline-versus-object size threshold ([ADR-0004](../adr/ADR-0004-bodies-and-attachments.md), [ADR-0024](../adr/ADR-0024-performance-foundation.md) d6) |
| `dek_cache.ttl` | deployment | stated at [FR-22](../product/roadmap.md) | TTL of the DEK serving cache; the TTL joins [FR-17](../product/roadmap.md)'s composed erasure bound, and the cache's shape is open as [RS-14](../product/roadmap.md) ([retrieval](../architecture/retrieval.md)) |
| `auth.epoch_cache_bound` | deployment | reserved — unused; the bound in force is replication lag, not a setting | Reserved for a future effective-access cache and unused while none exists. The stated bound between a revocation, grant change or denial and the reads that honour it is not a setting: the lead applies it at the key's next request, and a follower, which resolves keys, grants and denials from its local replica, after replication lag, exposed as a follower metric ([retrieval](../architecture/retrieval.md#reads-in-cluster-mode), [security](../architecture/security.md), [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d14) |
| `readlog.anchor_cadence` | deployment | no committed bound | How often read-log batch digests anchor into the journal; the cadence **is** the unanchored-tail exposure window, and no bound is committed yet ([commitments](../product/commitments.md), [retrieval](../architecture/retrieval.md)); the cluster-mode split is [RS-13](../product/roadmap.md) |
| `quarantine.gc_schedule`, `quarantine.refused_expiry` | deployment | stated at [FR-22](../product/roadmap.md) | The keyless GC sweep over unreferenced quarantine objects, and the short named expiry of refused submissions' objects ([admission](../architecture/admission.md), [deployment](deployment.md)) |
| `delta.optimize_schedule`, `delta.checkpoint_interval`, `delta.vacuum_retention` | deployment | daily bin-pack / weekly Z-order; checkpoint every 100 commits; vacuum retention = the table's erasure bound | Lake maintenance run inside the lake-writer; vacuum retention is an erasure-SLO term ([delta-tables](../data/delta-tables.md), [deployment](deployment.md)) |
| `backup.postgres_retention`, `backup.kek_rotation_period`, `backup.object_lifecycle` | deployment | profile default — not measured, not regulator-sourced | **Declared** windows, not executed ones: the backup machinery runs outside OML, and these keys record what the operator declares so the erasure SLO and the evidence export can state it ([deployment](deployment.md), [audit and lineage](../architecture/audit-and-lineage.md)) |
| `doctor.thresholds` | deployment | unmeasured defaults | The verdict thresholds `oml doctor` applies ([observability](observability.md)) |

### Journalled tenant policy (not on this surface — listed for completeness)

| Object | Scope | Semantics |
|---|---|---|
| Review policy per workarea — `k`, `required_roles`, `human_required_classes`, `independence_axis`, `held_item_expiry`, `vocabulary_policy` | tenant / workarea | [Admission](../architecture/admission.md) |
| Quota and rate-limit grants | tenant | Journalled grant attributes; enforcement is write-exact on the lead and limit-over-N across followers, with the accuracy bound tracked as [FR-8](../product/roadmap.md) ([security](../architecture/security.md), [commitments](../product/commitments.md)) |
| Read-log retention window | tenant | [Retrieval](../architecture/retrieval.md) |

### Postgres settings OML verifies but does not set

These are properties of the Postgres the operator brings. OML has no key for
them; `oml doctor` checks them and reports, because a requirement discovered
in an incident review is a design defect ([observability](observability.md)):

| Property | Requirement | Semantics |
|---|---|---|
| `synchronous_commit` + quorum standbys | `on` / `remote_apply` with quorum synchronous standbys on the journal domain, in the scaled profile; the single-node reduction is stated, not silent | [Storage](../architecture/storage.md), [deployment](deployment.md) |
| `FORCE ROW LEVEL SECURITY`, C6 role split | binds in all three profiles | [Security](../architecture/security.md) |
| pgvector presence and version; iterative-scan caps (`hnsw.max_scan_tuples`, scan memory) | GUC-level cutoffs whose recall consequences are [FR-4](../product/roadmap.md)'s to measure | [Retrieval](../architecture/retrieval.md) |

## What this page does not decide

The value of any unbound default (those rows resolve at
[FR-22](../product/roadmap.md)), the semantics of any setting (owned by the
linked pages), and tenant policy content (tenant admin acts, journalled where
their owning pages say so). Pool sizing is deliberately absent: it becomes a
[deployment](deployment.md)-page value once
[FR-25](../product/roadmap.md)'s rig sizes it
([ADR-0024](../adr/ADR-0024-performance-foundation.md) d4).
