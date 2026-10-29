# ADR-0024 — The performance foundation

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists and no measurement of OML exists.
Per the [roadmap](../product/roadmap.md) rules, no OML performance number appears here or
anywhere else; every quantity this record cares about resolves to [FR-25](../product/roadmap.md),
whose exit criterion produces the numbers. Third-party figures appear below as cited context
only, and are never restated as properties of OML.

## Context

The provider ring (`/v1`, [ADR-0022](ADR-0022-two-ring-api.md)) is the product's hot path:
submit, get, recall, supersede, attachment IO. The requirement is a very performant foundation
for exactly that ring, on the stack [ADR-0006](ADR-0006-pure-rust.md) committed (axum, sqlx,
object_store, delta-rs) and the cluster topology
[ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) defines (lead/follower, default
lead-only; election and failover are that record's, not this one's).

The research behind this record produced one decisive structural finding. The hot write path's
floor is the **quorum-synchronous commit round trip held inside the per-tenant chain lock**
([storage](../architecture/storage.md) D1). Everything cryptographic is noise next to it:
ed25519 signing is reported around 15 µs per signature on commodity hardware
([ed25519-dalek README benchmarks](https://github.com/dalek-cryptography/ed25519-dalek)), and
AES-GCM with AES-NI runs at multiple GB/s per core
([BoringSSL measurement](https://boringssl.googlesource.com/boringssl/+/a26d4c3f439543e45df5d0f381f0a815504255a7%5E%21))
— third-party figures, cited as scale context. The shape of the synchronous-replication cost is
also third-party: EDB's injected-latency test found sync replication near noise at 3 ms of
standby latency and material at 10 ms
([EDB: The Varying Cost of Synchronous Replication](https://enterprisedb.com/blog/the-varying-cost-synchronous-replication)
— the author calls the rig a proof of concept; shape, not numbers). The chain arithmetic
follows: lock-held time ≈ one round trip (begin + lock + chain-head read) + microseconds of
hash and sign + one round trip (the insert) + the quorum-apply commit, so per-tenant
single-submit throughput is on the order of the reciprocal of commit latency. That is
arithmetic over cited context, not an OML number — [FR-18](../product/roadmap.md) and FR-25
produce the real figures.

Two consequences frame every decision below. First, nothing OML optimizes in its own code
buys anything while avoidable work sits inside the lock or avoidable round trips sit inside
the transaction — so the record's job is exclusion rules, not tuning. Second, the floor
itself is not a defect: the quorum-synchronous commit is
[commitments](../product/commitments.md) §2's availability-for-integrity trade, senior to any
latency budget.

## Decision

- **d1 — Lock discipline: the chain lock holds read-head + hash + sign + one insert
  statement + commit, and nothing else.** No KMS call, no object-store IO, no scan, no
  embedding, no serialization heavier than the signed envelope inside the lock; the signature
  is the only CPU work permitted there, and the lock-held floor is the quorum commit, by
  design. The transaction is at most two round trips before commit: round trip one is
  `BEGIN` + `pg_advisory_xact_lock` + chain-head read as one statement; round trip two is the
  journal insert + metadata fold + outbox insert as **one data-modifying-CTE statement** (or
  one prepared multi-insert). One statement also neutralizes sqlx's missing pipelining
  ([ADR-0006](ADR-0006-pure-rust.md) d1): with one statement there is nothing to pipeline.
  Everything deterministic and everything networked runs **before `BEGIN`**: ban check,
  idempotency lookup, the deterministic scan, DEK generation + KMS wrap, AES-GCM body
  encryption, both SHA-256 digests, and the quarantine object PUT for bucket-placed bodies
  ([admission](../architecture/admission.md)). Corollary: a transaction that fails after its
  quarantine PUT orphans a ciphertext object, so a **keyless GC sweep over unreferenced
  quarantine objects** is a named mechanism — declared with quarantine placement in
  [admission](../architecture/admission.md), never an inline concern on the write path.

- **d2 — No LLM call, and no network call other than Postgres, on either ring's hot path,
  ever.** Embedding stays asynchronous in `oml-indexer`
  ([retrieval](../architecture/retrieval.md)); KMS appears in exactly two places —
  pre-transaction admission prep and the serving-path unwrap — and in neither does it sit
  inside a lock or a transaction.

- **d3 — The DEK serving cache is a declared design object, because the alternative is a KMS
  fan-out on every recall.** Without a cache, a recall with k hits pays k per-artifact DEK
  unwraps, each a network call to KMS whose latency has no published figure (AWS documents no
  latency for Decrypt/GenerateDataKey — unverified, measured by FR-25's harness; AWS's own
  mitigation guidance is bounded data-key caching,
  [AWS Security Blog](https://aws.amazon.com/blogs/security/caching-kms-data-keys-in-multi-thread-environments-per-tenant-encryption-for-event-driven-systems-at-scale/)).
  The cache therefore exists, and its design tension is stated plainly: it holds **plaintext
  keys of erase-eligible artifacts**. It is a named copy class in the erasure inventory
  (declared in [retrieval](../architecture/retrieval.md), inventoried in
  [audit and lineage](../architecture/audit-and-lineage.md)); erase purges it synchronously;
  its TTL joins [FR-17](../product/roadmap.md)'s composed erasure bound. The open
  sub-decision — cache unwrapped per-artifact DEKs, or cache an unwrapped per-tenant KEK and
  unwrap DEKs locally — carries different KMS-call profiles, blast radii and erasure
  reasoning, and is recorded as [RS-14](../product/roadmap.md); neither option is free.

- **d4 — Statement discipline: prepared statements on every hot query, hydration in one or
  two statements, fusion in one pass.** sqlx's automatic per-connection statement cache
  satisfies the prepared-statement rule out of the box
  ([sqlx docs](https://docs.rs/crate/sqlx/%5E0.6.1)). Recall hydration — steering rows, hits,
  bodies — is one or two prepared statements fetched set-wise, **never N+1 per hit**; fusion
  of FTS and vector candidates is computed in one pass (in SQL or in-process over two
  candidate lists), never a third query round; server-side token-budget truncation
  ([FR-11](../product/roadmap.md)) doubles as a performance feature by bounding decrypt and
  wire bytes. Pooling: v1 uses the in-process sqlx pool per process, no external pooler; pool
  sizing becomes a [deployment](../operations/deployment.md)-page value once
  [FR-25](../product/roadmap.md)'s rig sizes it. If a pooler ever enters, it
  runs in transaction mode only — transaction-scoped `pg_advisory_xact_lock` and `SET LOCAL`
  are verified compatible with transaction pooling (the lock releases at commit, exactly when
  the backend may be reassigned; it is session-level advisory locks that are incompatible —
  [netdata](https://www.netdata.cloud/guides/pgbouncer/pgbouncer-advisory-locks-transaction-mode/),
  [netguru](https://www.netguru.com/codestories/advisory-locks); the discipline C6 already
  mandates, [commitments](../product/commitments.md)) — and protocol-level prepared statements
  under transaction mode need PgBouncer ≥ 1.21, an **unverified** pin the deployment page
  carries if that day comes.

- **d5 — Batch submit is the per-tenant throughput feature, not a convenience.**
  [FR-12](../product/roadmap.md)'s batch endpoint signs and inserts N events under **one**
  lock acquisition and **one** quorum commit, amortizing the entire structural floor of d1.
  The single-submit per-tenant ceiling (≈ one commit latency per event, the Context
  arithmetic) is accepted and documented, not fought. Aggregate multi-tenant throughput is a
  different quantity: tenants do not share locks, and Postgres group commit amortizes flushes
  across concurrently committing tenants — [FR-7](../product/roadmap.md) measures that
  scaling.

- **d6 — Attachment IO streams end to end.** Client → encrypt stream → object store, with
  digests computed over the stream; no full-body buffering anywhere on the path. The
  Postgres-versus-bucket size threshold is pinned where
  [ADR-0004](ADR-0004-bodies-and-attachments.md)'s open points resolve
  ([FR-15](../product/roadmap.md)).

- **d7 — Latency budgets are per-ring targets tied to FR-25, and the quorum floor is outside
  them.** Per the roadmap rules, no number for an unmeasured quantity appears in any page:
  the targets for submit, get, recall, supersede and attachment IO are declared in
  [FR-25](../product/roadmap.md), whose exit criterion produces measured p50/p95/p99 against
  them, published with date, hardware and settings. The quorum-synchronous commit is
  explicitly excluded from "optimizable": [commitments](../product/commitments.md) §2's
  availability-for-integrity trade is senior to any budget, so no target may be met by
  weakening durability.

- **d8 — Build and runtime posture.** tokio multi-threaded runtime; the global allocator
  (system, jemalloc or mimalloc) is chosen by rig measurement, not by claim; thin LTO in
  release profiles; PGO deferred until the macro rig exists to profile against.

- **d9 — Cluster-mode hot path: the lead owns admission and the chain; a follower serves the
  whole read path ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d13), and its hot
  path is recall and get, only.** The topology itself — profiles, lease, election, failover — is
  [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md)'s and is not respecified here;
  failover measurement is [FR-24](../product/roadmap.md). What this record decides is the hot
  path over that topology:
    - **No follower composition contains the signer port** — `oml-server-follower` composes
      KMS serving decrypt but not the signing key, a composition fact in the C7 pattern
      ([ADR-0006](ADR-0006-pure-rust.md) d6,
      [commitments](../product/commitments.md) C7). Default deployment is lead-only.
    - **A write arriving at a follower is transparently forwarded** to the lead over a pooled
      internal HTTP/2 channel, and the response relayed. A `307` redirect is rejected: agent
      HTTP stacks are unreliable about re-sending POST bodies on redirect, a redirect leaks
      topology into every client, and a streamed attachment upload cannot replay a consumed
      body — the provider ring's dumb-client premise
      ([ADR-0022](ADR-0022-two-ring-api.md)) argues against topology-aware clients. The hop
      cost is measured, not asserted (FR-25 scenario 9); clients may point at the lead
      directly as an optimization, never as a protocol requirement.
    - **Follower read routing:** own-held reads — and any submit-then-get sequence — route to
      the lead and the primary, buying read-your-writes where an agent's loop visibly breaks
      without it (held rows are author-only anyway). Promoted-row recall and get serve from
      the follower's local replica with **staleness equal to replication lag, exposed as a
      follower metric** — stated, not hidden. An **LSN fence** (the submit response carries
      the commit LSN; a follower read can wait for replay to pass it) is the named escape
      hatch if read-your-writes recall is ever demanded; it is not built for v1.
    - **Follower read-logging is an asynchronous batched insert to the primary**, never a
      synchronous write inside a follower's read path; the enlarged unanchored tail this
      buffering creates is stated where [commitments](../product/commitments.md) §2 states
      the read-log bound, and the split's remaining open shape is
      [RS-13](../product/roadmap.md).
    - **The single-lead ingest ceiling is a named revisit trigger.** A single lead caps
      admission prep (scan, digests, encryption, KMS wrap) at one node's cores and its KMS
      request concurrency — and since per-core crypto is GB/s-class (Context), the realistic
      limiter is KMS wrap calls per submit, whose quotas are unverified. If the rig shows
      lead CPU or KMS concurrency saturating before per-tenant ceilings do, multi-writer
      (already lock-safe — correctness never needed a single writer; the lock did that) or
      KMS-call batching reopens — on rig evidence, not preference.

## Consequences

- **FR-25 is this record's measurement harness**, and nothing here is a number until it runs.
  The harness has three layers, each published with date, hardware and settings per the
  roadmap rules:
    - *Criterion micro-benchmarks:* ed25519 sign/verify; SHA-256 and AES-256-GCM at 1 KB,
      64 KB, 1 MB and 16 MB; canonical envelope serialization; scan-rule pass per KB (feeds
      [FR-15](../product/roadmap.md)).
    - *Commit-path integration bench:* lock-held time and end-to-end commit latency across
      `synchronous_commit ∈ {on, remote_apply}` × injected standby round-trip times (the EDB
      methodology, run on OML's own shape — produces [FR-18](../product/roadmap.md)'s
      numbers); separately, the measured KMS GenerateDataKey/Decrypt latency distribution in
      the target region, since no published figure exists.
    - *Load-rig macro scenarios*, a Rust rig driving the real server: (1) single-tenant
      sustained submit — the serialization ceiling; (2) 1/10/100 tenants concurrent submit —
      group-commit amortization ([FR-7](../product/roadmap.md)); (3) batch submit versus N
      single submits — the d5 lever quantified; (4) recall p50/p95/p99 at stated corpus sizes
      and index-versus-RAM ratios, filtered and unfiltered ([FR-4](../product/roadmap.md) —
      EDB's pgvector report found HNSW throughput varying 5–7x across its tested memory
      range, [EDB pgvector benchmarks 2026](https://www.enterprisedb.com/sites/default/files/2026-06/pgvector_vs_VectorChord_benchmarks.pdf),
      which makes RAM ratio a required axis); (5) hydration with cold versus warm DEK cache —
      the d3 fan-out quantified; (6) mixed 90/10 recall/submit at pool-saturation
      boundaries; (7) allocator A/B under scenario 6; (8) read scaling at one lead plus 0–4
      followers under concurrent write load, with the replication-lag distribution — the d9
      staleness bound's actual size; (9) forwarded-write overhead, follower versus direct;
      (10) standby serving conformance — after a `remote_apply` commit, an HNSW and a
      tsvector query on the sync standby return the new promoted row, plus the honest
      negative case on an async replica; (11) lead failover window and fork-check duration
      (joins [FR-24](../product/roadmap.md)).
- **Clients must reuse connections.** Agents are long-lived clients, and connection
  keep-alive/reuse is the real HTTP-layer performance property — both rings' API pages carry
  it as a client requirement; hyper serves HTTP/2 and the defaults stand until the rig says
  otherwise.
- **The unverified ledger.** This record leans on claims that are marked, not trusted; each
  is to be verified with a fetch date or replaced by a measurement before it is cited as
  fact anywhere: TechEmpower's benchmark suite reportedly archived read-only in March 2026
  (secondary sources only — verify the repository banner; consequence: every current
  framework comparison is self-published); the axum-versus-actix ~10–15% gap
  (self-published); PgBouncer ≥ 1.21 for prepared statements in transaction mode; SHA-256
  per-core throughput (no clean SHA-NI figure found — the criterion harness measures it);
  AWS KMS latency distribution and request quotas (no published figure — measure); typical
  same-AZ/cross-AZ and intra-cluster round-trip times (general knowledge, context only); the
  pgvector version threshold for serving HNSW queries on a standby (claimed 0.7.0 by a weak
  source — check the pgvector release notes, pin in
  [deployment](../operations/deployment.md), covered by scenario 10); iterative-scan GUCs
  applying on a hot standby (same conformance case); the causal-reads proposal's merge
  status; monoio's benchmark numbers (vendor-run).
- The hot-path rules ripple into the pages that own the mechanisms: the pre-`BEGIN`
  exclusions and the quarantine GC sweep in [admission](../architecture/admission.md); the
  DEK cache, hydration discipline and follower read policy in
  [retrieval](../architecture/retrieval.md); the sqlx facts and the lead/follower
  composition split in [ADR-0006](ADR-0006-pure-rust.md); the custody and read-log ripples
  in [commitments](../product/commitments.md), [security](../architecture/security.md) and
  [storage](../architecture/storage.md) where those pages state them.

## Rejected alternatives

- **actix-web in place of axum.** The quantified edge is ~10–15% on raw JSON throughput,
  self-published and methodologically weak
  ([markaicode 2025](https://markaicode.com/rust-web-frameworks-performance-benchmark-2025/),
  [reintech 2026](https://reintech.io/blog/axum-vs-actix-web-vs-rocket-rust-framework-comparison-2026));
  no maintained neutral benchmark exists to settle it (TechEmpower reportedly archived —
  unverified ledger). A framework delta of that size is invisible behind a path dominated by
  quorum commits and index queries, and the tower middleware ecosystem carries the two-ring
  routing, idempotency and auth layers.
- **monoio / glommio (io_uring runtimes).** The published wins are vendor-run, appear at
  high core counts on syscall-bound echo workloads
  ([monoio benchmark docs](https://github.com/ukernel/monoio/blob/master/docs/en/benchmark.md)),
  and tokio wins the low-connection latency case there; OML waits on Postgres, not on
  syscalls, and thread-per-core fights the per-tenant-lock workload shape.
- **simd-json.** KB-scale payloads on a path with millisecond-scale storage waits;
  serde_json is not the bottleneck. Revisit only on rig evidence.
- **QUIC / HTTP3.** Long-lived agent clients on stable connections gain nothing over
  HTTP/2; rejected for v1.
- **rkyv / bincode.** Irrelevant: the API is JSON and no internal wire format exists —
  outbox payloads live in Postgres.
- **Adopted as measured options, not claims:** the jemalloc/mimalloc allocator swap enters
  as a rig A/B (scenario 7), thin LTO enters release profiles, and PGO is deferred until the
  macro rig exists — none of the three is asserted to help until the rig says so.
