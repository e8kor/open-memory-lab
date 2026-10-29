# ADR-0019 — Hexagonal architecture and plugin modularity

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. The Cargo workspace that implements
this shape is decided in [ADR-0006](ADR-0006-pure-rust.md); the module view it produces is drawn
in [architecture/overview.md](../architecture/overview.md).

## Context

OML's constitution ([ADR-0001](ADR-0001-the-constitution.md)) is a set of invariants — verb
ceilings, non-author review, visibility gating, no silent deletion — and the register so far
decides *where* each is enforced but not *what shape of code* makes the enforcement
unavoidable. Without a stated internal structure, the invariants end up re-checked in every
handler and every binary, and a new adapter (a different object store, a different scanner)
becomes an opportunity to route around them.

The system also has points where deployments legitimately differ — which scan rules run, which
embedding model is used, what an evidence bundle looks like — and points where they must never
differ: the admission state machine, the workarea algebra, the status ladder. A modular design
has to make the first kind cheap to swap and the second kind impossible to swap.

Both pressures point at the same well-known shape: a hexagonal (ports and adapters)
architecture, with a dependency-free domain core, explicit ports, and every piece of
infrastructure — including the plugin points — behind an adapter boundary.

## Decision

- **d1 — The domain core is a pure crate, and every constitution invariant lives in it, only.**
  `oml-domain` has no I/O: no database driver, no HTTP, no clock read, no KMS call. It holds the
  admission state machine ([admission](../architecture/admission.md)), the workarea algebra and
  status ladder ([workareas](../architecture/workareas.md)), the role and verb ceilings
  ([roles and keys](../architecture/roles-and-keys.md)), and the simulation taint predicate
  ([simulations](../architecture/simulations.md)). R1–R4 and the one bounded development
  exception are encoded here as types and transition functions, nowhere else — so no adapter,
  transport or plugin can enforce a different constitution, because none of them contains one.
  The core is testable without a running database, which is where the invariant tests live.

- **d2 — Ports are explicit Rust traits, owned by the core/application layer.** The application
  layer (`oml-app`) declares one trait per thing the domain needs from the world: the
  journal-and-metadata store port (the one-transaction commit point of
  [ADR-0002](ADR-0002-postgresql-system-of-record.md)), the lake materialization port
  ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md)), the object store port, the KMS port,
  the scanner port, the embedder port, the exporter port, and a clock/id port so that even time
  and identifiers enter the core as inputs. Adapters implement the traits — Postgres via sqlx,
  Delta via delta-rs, S3/MinIO via object_store, KMS — with **one adapter crate per
  infrastructure dependency**, so a dependency's blast radius is one crate and its version bump
  is one review.

- **d3 — The four binaries are thin compositions, and the client surfaces are adapters too.**
  Each binary wires the same domain core to exactly the adapter set its job needs, and nothing
  else: `oml-server` composes all write-path ports plus the driving transports; `oml-lakewriter`
  composes the outbox read side and the lake port — keyless, moving ciphertext verbatim, exactly
  as [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) requires; `oml-reconciler` composes
  read-only ports ([ADR-0017](ADR-0017-reconciliation.md)); `oml-indexer` composes the embedder
  port and the serving-side read. On the driving side, the HTTP `/v1` transport, the MCP facade
  ([ADR-0014](ADR-0014-http-api-and-mcp-facade.md)) and the `oml` CLI
  ([ADR-0015](ADR-0015-cli-administration.md)) are themselves adapters: they translate requests
  into application-layer calls and carry no policy. A binary that does not compose a port cannot
  perform its operation — the lake-writer has no KMS port to misuse, which is how "keyless"
  stops being a discipline and becomes a type error.

- **d4 — Plugin points exist where deployments legitimately differ, and plugins obey a stated
  law.** The plugin points are the **scanner port** (which deterministic rule-sets run at
  admission), the **embedder port** (which embedding model the indexer calls — the open model
  choice in the [roadmap](../product/roadmap.md)), and the **exporter port** (evidence-bundle
  formats). In v1 a plugin is a compiled-in Rust implementation behind the port trait, selected
  by deployment configuration — and configuration changes are journalled
  (`system.settings_applied`, [event catalog](../data/event-catalog.md)), so which plugin was
  active when is always an answerable question. Dynamic loading (dlopen, WASM) is explicitly
  out of scope for v1: loaded code would enter the TCB of whichever process hosts it, and a
  plugin supply chain is an attack surface the register has not priced. The revisit trigger is
  named: *a deployment that needs a scanner or embedder which cannot ship in the compiled
  release*. The **plugin law**: a plugin can narrow, never widen. It cannot mint verbs or
  extend any role's ceiling; at admission it can only move the fail-closed outcome toward
  holding — a scanner can veto or hold, never promote; it never sits on the signing or erasure
  path; and its identity and version are recorded in the journal events its work informs, as
  the scan envelope's `scanner_version` already does ([event catalog](../data/event-catalog.md)).

## Consequences

- The crate layout follows mechanically and is committed in
  [ADR-0006](ADR-0006-pure-rust.md) d6: `oml-domain` (pure), `oml-app` (ports and use-cases),
  one adapter crate per dependency, plugin crates behind the scanner/embedder/exporter ports,
  and five thin composition crates for the binaries and the CLI.
- Adding an adapter or a plugin has a fixed recipe: implement the port trait in a new crate,
  register it in the composition of the binaries that need it, and let the journalled settings
  apply select it. No step touches `oml-domain`, and a change that wants to is a constitution
  change, which is an ADR, not a pull request.
- Some things stay impossible by construction rather than by review: a scanner that promotes
  (the port's return type has no promoting arm), a lake-writer that decrypts (no KMS port in
  its composition), a transport that bypasses admission (transports call use-cases, and the
  use-cases run the state machine), a plugin whose activity is invisible to an auditor (its
  selection is journalled and its version rides the events it informs).
- The cost is indirection: every infrastructure call crosses a trait boundary, and contributors
  must learn the port vocabulary before the first adapter. OML accepts that because its risk
  profile ([ADR-0006](ADR-0006-pure-rust.md)) is correctness under concurrency, and a core that
  can be exhaustively tested without infrastructure is the cheapest correctness instrument the
  design owns.
- Events informed by embedder or exporter work do not yet carry a plugin identity field the way
  scan events do; whether and where those fields land is the
  [event catalog](../data/event-catalog.md)'s to decide, flagged there rather than silently
  assumed here.

## Rejected alternatives

- **A layered monolith without ports.** The conventional service/repository layering lets
  infrastructure types leak upward until the domain logic is only testable against a running
  Postgres, and every invariant check acquires a second, slightly different copy near whichever
  handler needed it. The constitution would then be a convention, and OML's whole register
  exists because conventions are not mechanisms.
- **A microservice per port.** Splitting the hexagon's edges into network services buys
  independent deployment nobody asked for and costs what ADR-0002 forbids spending: the commit
  point must be one Postgres transaction, and a service boundary through it is a dual write.
  Operational surface (N deployments, N credential sets, N versions to reconcile) grows with no
  movement of any trust boundary — the process boundaries that matter are already drawn by key
  custody ([ADR-0006](ADR-0006-pure-rust.md) d5).
- **Dynamic plugin loading at v1.** dlopen'd or WASM-hosted plugins make the swap cheapest of
  all, and put third-party code inside the processes the threat model works hardest to keep
  small — the scanner runs in `oml-server`, the process that holds the signing key. Until the
  named trigger in d4 fires, compiled-in selection keeps the TCB argument of
  [ADR-0006](ADR-0006-pure-rust.md) intact and the supply chain inside `cargo build`.
