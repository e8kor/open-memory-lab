<!--
id: changelog
tier: engineering
status: design draft for review · decided, not built
answers: how the design spec is versioned, what 0.1.0-design and 0.2.0-design cover, and what each revision changed
-->

# Spec changelog

**Status:** design draft for review · decided, not built

This page is the revision record of the design specification itself. The current spec version
is **0.2.0-design**, matching `info.version` in the [OpenAPI document](api/openapi.yaml). Until
2026-10-30 no code existed; since the PoC entry below, an entry may also record an
implementation milestone where it changes what the repository holds.

## How this changelog is kept

- **One entry per design PR.** A merged design change adds one dated entry naming the ADRs and
  pages it touched; a change with no entry is a defect.
- **`info.version` bumps on any /v1-surface change.** A change to the provider ring's
  contract — paths, request or response shapes, the error taxonomy, idempotency or pagination
  semantics — bumps the version in `docs/api/openapi.yaml`, and this page records the bump
  alongside the change that forced it.
- **The repo is tagged `spec-vX.Y` at each published revision whose records are accepted**,
  starting with `spec-v0.1` at implementation start, so an implementation can state exactly which
  revision of the design it was built against; a revision carrying a `proposed` record is
  tagged when that record is accepted.

## 0.2.0-design

### 2026-10-30 — The release image for linux/amd64 and linux/arm64 (implementation)

An implementation milestone; no decision changes. `.github/workflows/release.yml` now builds the
image as [CI and toolchain](engineering/ci-and-toolchain.md) decides: one leg per platform on a
native runner (`ubuntu-24.04`, `ubuntu-24.04-arm`), each pushed by digest with its own SBOM and
provenance, then a merge job that assembles one index with `docker buildx imagetools create`,
asserts both platforms are in it, and signs the index digest with `cosign sign --recursive`. The
binary tarballs stay `x86_64-linux`, and the release notes say so. `ci.yml` gains an `image` job
that builds the image for both platforms on every pull request and runs its `oml --version` on
each, and `scripts/check_release.py` fails the pull request if `release.yml` stops building both
platforms, assembling the index or signing its digest. No tag has run the new pipeline yet; the
`v0.1.0` image is `linux/amd64` alone.

### 2026-10-30 — the agent-only memory surface (ADR-0025)

[ADR-0025](adr/ADR-0025-the-agent-only-memory-surface.md) (proposed) decides the surface
FR-10 and FR-11 need and the access model it rests on. It proposes amending ADR-0001 (d5),
ADR-0008 (d4, d6), ADR-0010 (d1, d3), ADR-0012 (d7), ADR-0013 (d1, d2, d5), ADR-0014 (d2),
ADR-0018 (d4) and ADR-0022 (d4, d6); each of those records' Status lines gains the back-pointer
when ADR-0025 is accepted.

- **Decided:** grant admissibility per key role and one four-step refusal order, so one
  writer-role key with writer and reader grants remembers and recalls; issuance that carries
  grants and denies and discloses the value only after the last grant commits; "own" as the
  authoring key; `include_own_held` on recall, with held text matched only through an
  author-fenced index; the budget token (counter `oml-tc/1`) and the `max_tokens` cut with a
  `budget` object; applicable steering and a total hit order; a recall logs the ids served and a
  digest of the whole request; supersession moves the tip only at ratification, by a
  compare-and-set bound to the successor's workarea and `claim_key`, as one `lifecycle.supersede`,
  with the authors of rival held successors kept off each other's review; the author's
  retire-proposal as a supersession carrying `retirement: true`, open to the same keys as any
  supersession; distinct version ordinals, so competing held successors coexist and the tip
  compare-and-set decides between them; own read on the target version; a self-signed version
  never replacing a reviewed tip (d17); the `claim_key` release after a never-promoted rejection
  and the refusal of a reopen that would collide (d18); `simulation.write` journalling
  `body_bytes`, `replaces` and `retirement` like every submission event; own simulation rows read
  only inside the simulation's audience; `oml_get` and `oml_supersede` and one claim-item shape;
  the own-work listing across every workarea the key wrote to; the workarea read with the caller's
  children, effective verbs and permission epoch; per-family visibility on the events tail with a
  bounded scan and `next_after_seq`, where a `lifecycle.*` event reaches readers only while the
  version's latest move between held and promoted is a promotion, reaches its author in any case,
  and names no reviewer, admin or claim workarea the key holds nothing on; a request digest over
  method, route, path parameters and body, with supersede's idempotency record scoped to the key
  alone, looked up before the target's readability so a committed supersession always replays, and
  claimed only once the target is known readable, every target-dependent refusal ordered after
  that; a `held_by: service` key refused every review-class verb; `derives_from` and `replaces`
  served only as far as the caller can read; a successor's kind changing only within its class,
  and nothing but its kind inherited from the target; a row-count check on every `held_fts`
  removal; effective access computed on every request with no cache, so a revocation applies to
  the key's next request served by the lead, and on follower-served reads after replication lag
  (proposing to amend ADR-0008 d4 and d6); request bodies that refuse unknown fields.
- **`info.version` 0.1.0-design → 0.2.0-design**, because `/v1` request and response shapes
  change. No operation, error code, event type or claim state is added: the taxonomy and the
  operation set are unchanged. The MCP catalog goes from ten tools to twelve.
- **Roadmap:** FR-30 (the budget-token count against model tokenizers, unmeasured), FR-31 (the
  `claim_key` collision, the version ordinal, the tenant-wide journal positions and the lineage
  read's redacted ids as residual existence and count signals), FR-32 (a service key's grant
  confers every verb of its role, not only the one act its job needs) and RS-17 (revising one's
  own claim before its review) are opened. FR-10's exit criterion is reworded to the
  retire-proposal ADR-0025 d11 decides, a supersession of the claim rather than a verb of its
  own. FR-10 and FR-11 stay open until the agent-only-memory scenario passes in CI.
- **The release image** is decided as one index for `linux/amd64` and `linux/arm64`, signed on the
  index digest; a page edit, no decision change.
- **Pages touched:** the [decision register](adr/index.md) (register row; D4, D6, D7, D9, D11,
  D12, D13 and D14 cells), `mkdocs.yml`, [llms.txt](llms.txt), [openapi.yaml](api/openapi.yaml),
  [HTTP API](api/http-api.md), [errors](api/errors.md), [the agent plugin](api/mcp-plugin.md),
  [mcp-tools.json](api/mcp-tools.json) (the two new tools, `oml_my_work`, `oml_workarea_info` and
  `_meta`, with `_meta.pending_design` naming what the served `oml_recall` and `oml_events_tail`
  entries still lack; those entries gain only what the build already serves — the `k` maximum,
  the `after_seq` default, and no `scope` on the tail),
  [retrieval](architecture/retrieval.md), [roles and keys](architecture/roles-and-keys.md),
  [workareas](architecture/workareas.md), [admission](architecture/admission.md),
  [audit and lineage](architecture/audit-and-lineage.md),
  [event catalog](data/event-catalog.md), [Postgres schema](data/postgres-schema.md),
  [roadmap](product/roadmap.md), [CLI reference](cli/reference.md),
  [CI and toolchain](engineering/ci-and-toolchain.md), [upgrades](operations/upgrades.md),
  [security](architecture/security.md), [configuration](operations/configuration.md)
  (`auth.epoch_cache_bound` removed with the effective-access cache it bounded, which ADR-0025
  rejects),
  [commitments](product/commitments.md), [compliance](product/compliance.md),
  [concepts](concepts.md), [the overview](index.md) and this page; outside `docs/`, `CONTRIBUTING.md`, `README.md` and
  `plugins/oml/README.md` (version and tool counts).
- **Not built.** Nothing above is implemented by this entry; the PoC's divergences are named in
  ADR-0025's Context, and the code lands in later changes, each with its own entry. A
  `spec-v0.2` tag follows the maintainer's acceptance.

## 0.1.0-design

Reconstructed from the repository history; all dates 2026-10-29. The set was written and
revised in one sustained effort, so the entries below group commits by what they decided
rather than pretending a longer calendar.

### 2026-10-30 — The agent plugin's first tools, CI and the release pipeline (implementation)

An implementation milestone; no decision changes. The [agent plugin](api/mcp-plugin.md) is served
at `/mcp` for four of its ten tools — `oml_remember`, `oml_learn`, `oml_recall`,
`oml_events_tail` — as the stateless facade [ADR-0014](adr/ADR-0014-http-api-and-mcp-facade.md)
decides, each tool running its ring endpoint's own handler. The pinned MCP revision is
**2025-06-18**, checked against modelcontextprotocol.io (basic/transports, server/tools) on
2026-10-30. The remaining six tools answer as not built. A PoC key carries one role, so the Claude
Code plugin under `plugins/oml` connects twice, with a writer key and a reader key; the
single-key shape waits on grants of several roles per key. The merge gates and release artifacts
of [CI and toolchain](engineering/ci-and-toolchain.md) now run, except the MSRV build, the sqlx
offline check, the container tests against MinIO, the scheduled `cargo audit` and the bench job.
The docker profile's environment names were corrected to the ones
[configuration](operations/configuration.md) declares.

### 2026-10-30 — Scan-veto journalling decided; a transition precondition added

Two design points the Live Atlas review rounds forced into the open, now decided:

- **A scan veto journals nothing.** `submission.scan_veto` is a refusal like any other refusal:
  deterministic, reproducible from the journalled rule-set pin, and never a write into the
  signed chain (hostile input must not grow it). The wording is now uniform across
  [admission](architecture/admission.md), the
  [event catalog](data/event-catalog.md#what-is-deliberately-not-an-event),
  [errors](api/errors.md), [concepts](concepts.md), the [HTTP API](api/http-api.md),
  ADR-0012 (amended) and ADR-0020; whether refused submissions deserve a durable trace outside
  the chain is RS-16 on the [roadmap](product/roadmap.md).
- **`committed` → `validated` is refused while a simulation is fenced on the subtree** — the
  simulation must be ended (journalled) first, since taint can never enter a `validated` area
  ([workareas](architecture/workareas.md), transition matrix).

### 2026-10-30 — Live Atlas visualization

- The published site gains the [Live Atlas](atlas/index.html): an interactive 3D visualization
  of the crew-harness story — one crew per workarea, recall pulses pulled from production
  areas, remember pulses into the crew's own area, supersession with pulled evidence, ladder
  transitions, and the integration lattice between production areas — with drill-in modes for
  the admission reactor and the chain ledger. All data is mocked client-side in the shapes of
  the [event catalog](data/event-catalog.md); three.js is vendored (MIT), nothing loads from a
  CDN at runtime. Replaying a real PoC `/v2/events` tail is a planned data-source swap.

### 2026-10-30 — The PoC lands (M1 slice)

- The critical path's M1 slice is implemented in this repository (commit `54dadfd`): the Cargo
  workspace of [workspace-plan](engineering/workspace-plan.md), the signed chain reproducing
  [chain-format](data/chain-format.md)'s test vectors byte-for-byte, the one-transaction
  commit point, the standalone-profile server with startup fork check, and an independent CLI
  chain verifier — demonstrated end to end by `scripts/poc-demo.sh`. The spec remains the
  authority; where the PoC diverges it says so in code comments and the README.

### 2026-10-29 — Documentation finalization

A coordinated finalization pass over the whole set, encoding decisions taken for
implementation readiness:

- The [decision register](adr/index.md) gains the D1–D17 ownership table (which page declares
  which decision handle), the ADR status lifecycle, and an [ADR template](adr/TEMPLATE.md);
  this changelog and its going-forward rules are introduced.
- The closed error taxonomy grows 45 → 46 with `request.unsupported_media` (HTTP 415, family
  `request`, not retryable), carried on all three synced surfaces
  ([errors](api/errors.md), [openapi.yaml](api/openapi.yaml),
  [HTTP API](api/http-api.md)).
- The chain signing format is pinned: `signed_bytes` is RFC 8785 (JCS) canonical JSON of the
  enumerated envelope fields, `hash` is SHA-256 over them, `sig` is pure Ed25519 (RFC 8032)
  over `signed_bytes`; genesis is `prev_hash` = 32 zero bytes with `tenant.genesis` at seq 1
  ([chain format](data/chain-format.md)).
- The API key wire format is fixed (`oml_` prefix + 43 base62 characters, 256-bit random,
  stored as SHA-256 of the full key), and key-lifecycle journal events for signing-key and
  KEK rotation are added to the [event catalog](data/event-catalog.md).
- Operational commitments are pinned: forward-only `sqlx` migrations run by the bootstrap
  workload, a 24-hour committed floor on the idempotency replay window, positional pagination
  cursors with a named TTL setting, `simple` as the v1 FTS configuration with NFC
  normalization at admission, and one [runbooks page](operations/runbooks.md) with one
  anchored section per condition.
- Glossary entries for steering, the overlay tip and the fold join
  [Concepts](concepts.md).
- FR-26 (retention + legal hold), FR-27 (tenant offboarding ceremony verbs), FR-28 (operator
  verbs: dead-letter re-enqueue, erase replay, standalone migrate), FR-29 (attachment/bundle
  Range support) and RS-15 (per-tenant FTS configuration scope) added to the
  [roadmap](product/roadmap.md).

### 2026-10-29 — Install profiles, cluster mode, performance foundation

[ADR-0023](adr/ADR-0023-install-profiles-and-cluster-mode.md) (three install profiles, one
conformance bar, lead/follower cluster mode) and
[ADR-0024](adr/ADR-0024-performance-foundation.md) (hot-path discipline, the DEK serving
cache as a copy class, batch submit, latency targets as FR-25 rows).

### 2026-10-29 — The two-ring API

[ADR-0022](adr/ADR-0022-two-ring-api.md): one keyed listener, the /v1 provider ring under a
compatibility covenant and the /v2 lab ring, the token/header/body attribute-placement rule;
the [HTTP API](api/http-api.md) restructured around the rings.

### 2026-10-29 — AI+Human ontologies

[ADR-0021](adr/ADR-0021-ai-human-ontologies.md): vocabularies as governed claims — terms with
permanent CURIEs, releases as journalled pins, SSSOM mappings, the vocab scanner
([ontologies](architecture/ontologies.md)).

### 2026-10-29 — Critical review and dataset onboarding

The commitments register ([commitments](product/commitments.md)), the provider floor, dataset
onboarding ([ADR-0020](adr/ADR-0020-dataset-onboarding.md)), and the critical path
([critical path](product/critical-path.md)). Separately, the FR-9 market-claim
re-verification was refreshed with a 2026-10-29 fetch date.

### 2026-10-29 — Review-blocker resolution

All review blockers from the first read-through resolved; the [Concepts](concepts.md) page
and [ADR-0019](adr/ADR-0019-hexagonal-architecture-and-plugins.md) (hexagonal architecture
and plugins) added, the hexagonal-architecture presentation corrected, Mermaid sequence
diagrams fixed (semicolons escaped as `#59;`), and accidentally committed session telemetry
removed from the working tree and ignored (commit 19b4ee6); it remains in the repository's
earliest history pending a maintainer decision on a rewrite.

### 2026-10-29 — Initial design set

The founding commit: ADR-0001 through ADR-0018, the architecture, data, API, CLI and
operations pages, the [OpenAPI document](api/openapi.yaml), the [roadmap](product/roadmap.md)
as honesty ledger, and the GitHub Pages site built with `mkdocs --strict`.
