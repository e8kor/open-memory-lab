<!--
id: changelog
tier: engineering
status: design draft for review · decided, not built
answers: how the design spec is versioned, what 0.1.0-design covers, and what each revision changed
-->

# Spec changelog

**Status:** design draft for review · decided, not built

This page is the revision record of the design specification itself. The current spec version
is **0.1.0-design**, matching `info.version` in the [OpenAPI document](api/openapi.yaml). Until
2026-10-30 no code existed; since the PoC entry below, an entry may also record an
implementation milestone where it changes what the repository holds.

## How this changelog is kept

- **One entry per design PR.** A merged design change adds one dated entry naming the ADRs and
  pages it touched; a change with no entry is a defect.
- **`info.version` bumps on any /v1-surface change.** A change to the provider ring's
  contract — paths, request or response shapes, the error taxonomy, idempotency or pagination
  semantics — bumps the version in `docs/api/openapi.yaml`, and this page records the bump
  alongside the change that forced it.
- **The repo is tagged `spec-vX.Y` at each published revision**, starting with `spec-v0.1` at
  implementation start, so an implementation can state exactly which revision of the design it
  was built against.

## 0.1.0-design

Reconstructed from the repository history; all dates 2026-10-29. The set was written and
revised in one sustained effort, so the entries below group commits by what they decided
rather than pretending a longer calendar.

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
