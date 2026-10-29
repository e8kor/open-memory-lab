<!--
id: adr/ADR-0006-pure-rust
tier: engineering
status: accepted design
answers: why the implementation is pure Rust; what named trigger would admit Scala/Spark outside the trusted computing base
-->

# ADR-0006 — Pure Rust, with Scala/Spark behind a named trigger outside the TCB

**Status:** accepted design · 2026-10-29

## Context

The implementation language has to be priced against OML's workload and trust
shape, which are:

- a multi-tenant HTTP service where per-tenant hash-chain assignment is serialized
  under a transaction-scoped lock on the hot write path
  ([storage](../architecture/storage.md)) — though the honest accounting is that
  the only service code inside that lock is a hash and a signature, microseconds
  of CPU against a quorum-synchronous commit round trip measured in milliseconds
  ([ADR-0024](ADR-0024-performance-foundation.md)); the language case does not
  rest on hot-path throughput;
- a lake-writer that must drive Delta Lake commits, MERGE, OPTIMIZE, VACUUM and
  checkpointing with exactly-once semantics ([storage](../architecture/storage.md));
- a trusted computing base whose compromise is signing-key compromise
  ([security](../architecture/security.md)), which puts a premium on a small,
  auditable runtime with no VM, no interpreter and no dynamic import path in the
  process that holds the key.

Each point argues the same direction: the system's risk lives in concurrency and
storage-protocol correctness, and its most trusted process should carry the
smallest possible runtime. The hot-path rules the stack must obey are recorded in
[ADR-0024](ADR-0024-performance-foundation.md), not here.

## Decision

- **d1 — Every OML binary is Rust.** The server (its lead and follower
  compositions, d6), `oml-lakewriter`,
  `oml-reconciler`, `oml-indexer` and the `oml` CLI are one language, one
  toolchain, one set of static binaries. The committed stack: **axum** for the
  HTTP service, **sqlx** for Postgres, **object_store** for S3/MinIO/filesystem,
  and the **deltalake crate (delta-rs)** for all lake writes and maintenance.
  Decided, not built — no code exists.

    The sqlx choice is made with its limits on the table, not in ignorance of them:
    sqlx has **no pipelining** (tokio-postgres is a pipelined client,
    [docs.rs/tokio-postgres](https://docs.rs/tokio-postgres)); it **does** prepare
    and cache statements automatically per connection
    ([sqlx docs](https://docs.rs/crate/sqlx/%5E0.6.1)), which satisfies
    [ADR-0024](ADR-0024-performance-foundation.md)'s prepared-statement rule out of
    the box; and the measured gaps against tokio-postgres are anecdotal and
    fetch-heavy ([sqlx issue #2007](https://github.com/launchbadge/sqlx/issues/2007)
    reports a large `fetch_all` running ~50% faster on tokio-postgres) — a shape the
    design avoids: the write path is one statement per round trip and the read path
    is small-k hydration ([ADR-0024](ADR-0024-performance-foundation.md) d1, d4).
    The revisit trigger is named: *if the load rig attributes more than a stated
    fraction of the submit budget to driver overhead, `oml-pg` swaps to
    tokio-postgres + deadpool behind the same port — an adapter change, invisible
    above the port boundary* ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md)).

- **d2 — The deltalake crate is the whole lake interface, and its capability list
  is a sourced claim, not an assumption.** The design relies on these operations
  being available in delta-rs's operations layer: OPTIMIZE (bin-pack and
  Z-order), VACUUM, checkpoint creation, MERGE, and application transaction ids
  (appId + version recorded per commit). *This capability list is to be verified
  against delta-rs documentation with a fetch date at writing time; until then it
  is a design assumption, and the exactly-once claim that rests on it is written
  only as "exactly-once, verified by conformance test" — the test is
  [FR-3](../product/roadmap.md).* One capability is already known to constrain
  the design the other way: **deletion vectors are read-only in delta-rs**, so no
  OML design may assume DV-based deletes; retracts are MERGE operations in outbox
  order ([storage](../architecture/storage.md)).

- **d3 — All lake maintenance runs inside the lake-writer process.** OPTIMIZE,
  VACUUM and checkpointing are not side jobs with their own credentials; they run
  in the one process that already serializes lake commits, so the singleton fence
  ([deployment](../operations/deployment.md)) covers maintenance too. Delta
  commits on S3-compatible stores require conditional put (put-if-absent); the
  deployment pins a MinIO version supporting `If-None-Match`, and the txn
  appId + commit-version conflict is the second fence at failover.

- **d4 — Scala/Spark enters only behind a named revisit trigger, and never inside
  the TCB.** The trigger is: *a reconciliation or compaction job that delta-rs
  cannot express at our scale.* If it fires, Spark arrives as an untrusted batch
  job — its own credentials, read access scoped to what the job needs, never the
  signing key, never a KMS decrypt grant — and its output re-enters the system
  through the same journalled paths as any other privileged service's
  ([security](../architecture/security.md)). Until the trigger fires, no JVM runs
  anywhere in the deployment.

- **d5 — Key custody follows the language boundary nowhere and the process
  boundary everywhere.** Only `oml-server-lead` loads the deployment signing key
  ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md));
  lake-writer, reconciler and indexer run under their own credentials. This is
  declared in [security](../architecture/security.md) and restated here only
  because it is the reason d4's TCB line is enforceable: a job outside the key's
  process can be any language without entering the signing trust discussion.

- **d6 — One Cargo workspace, shaped by the hexagon
  ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md)).** The crates:

    | Layer | Crates | Holds |
    |---|---|---|
    | Domain | `oml-domain` | the pure core — admission state machine, workarea algebra, role/verb ceilings, status ladder, taint predicate; **every constitution invariant, and nothing with I/O** |
    | Application | `oml-app` | the port traits and the use-cases that drive the domain through them |
    | Driven adapters | `oml-pg` (sqlx), `oml-lake` (delta-rs), `oml-objectstore` (object_store), `oml-kms` | one crate per infrastructure dependency, implementing its port, holding no policy |
    | Driving adapters | `oml-transport-http`, `oml-transport-mcp` | the HTTP surface (both rings, [ADR-0022](ADR-0022-two-ring-api.md)) and the MCP facade as adapters over `oml-app` |
    | Plugins | `oml-scan-*`, `oml-embed-*`, `oml-export-*` | compiled-in implementations behind the scanner/embedder/exporter ports, under ADR-0019's plugin law |
    | Binaries | `oml-server-lead`, `oml-server-follower`, `oml-lakewriter`, `oml-reconciler`, `oml-indexer` | thin compositions: each wires `oml-domain` + `oml-app` to the adapter set its job needs. The server ships as two compositions for cluster mode ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md)): `oml-server-lead` composes the signer port and KMS; `oml-server-follower` composes KMS serving decrypt only and **no signer port** — "a follower cannot sign" is a dependency-graph fact plus a secret that is never mounted, the same pattern as C7's keyless lake-writer |
    | CLI | `oml-cli` | a thin `/v1`-and-`/v2` HTTP client ([ADR-0015](ADR-0015-cli-administration.md)) — every ceremony rides `/v2`; nothing administrative rides `/v1` ([ADR-0022](ADR-0022-two-ring-api.md)); composes no port trait and no domain crate — it reaches the hexagon only through the HTTP transport |

    This layout is the enforcement home of "constitution in the core": only `oml-domain`
    encodes invariants, adapters hold no policy, and a binary that lacks a port in its
    composition cannot perform that port's operation — d5's custody boundary, restated as a
    dependency graph.

## Consequences

- One toolchain builds the static binaries and the CLI; container images carry
  no interpreter and no package manager, which shrinks both image size and the
  attack surface of the process that holds the signing key. The cost is paid in
  iteration speed: Rust's compile-and-borrow-check loop is slower than an
  interpreted language's, and OML accepts that because its risk lives in
  concurrency and storage protocols, not in rapid prompt or schema iteration.
- The design is coupled to delta-rs's feature set. Where delta-rs lacks an
  operation (deletion vectors for writes, today), the design routes around it
  rather than shelling out to Spark — d4's trigger is deliberately narrow so the
  coupling is confronted, not laundered through a side channel.
- [FR-3](../product/roadmap.md) (delta-rs exactly-once conformance) and the
  fetch-dated capability citation in d2 are blocking items for the storage
  documentation's claims: until they land, every exactly-once sentence in the
  doc set carries its marker.
- The embedding model choice ([RS-1](../product/roadmap.md)) may yet pull a
  non-Rust component into the deployment as an external endpoint; that decision
  is open there and nothing in this record forecloses it — it constrains only
  what runs inside the TCB.

## Rejected alternatives

- **Rust + Scala/Spark from day one.** Spark would buy mature table maintenance
  and large-scale jobs before anyone has measured a need for them, and it costs
  what d4 exists to avoid: a JVM in the deployment enters the TCB discussion —
  its classpath, its serialization surface, its credential handling — for
  marginal gain at v1 scale, where delta-rs's own OPTIMIZE and VACUUM are
  designed to suffice (d2's verification pending). The trigger is named so this
  rejection is revisited on evidence, not on preference.
- **A Python core.** Python's iteration speed wins where the service's own code
  is never the bottleneck — a workload gated on a network LLM call measured in
  seconds can afford an interpreter. OML's profile is the opposite: the
  service's own code sits on the per-tenant chain lock and the lake commit
  path, so Rust's performance margin lands exactly where the cost lives. The
  iteration-speed argument also misses OML's actual schedule risk, which is
  storage and concurrency correctness, not rapid prompt or schema iteration —
  and an interpreter in the process that holds the signing key reopens the TCB
  argument d1 settles.
