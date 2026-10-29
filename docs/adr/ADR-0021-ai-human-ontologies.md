<!--
id: adr/ADR-0021-ai-human-ontologies
tier: engineering
status: accepted design
answers: why shared vocabularies are governed claims on the existing pipeline rather than a separate ontology service
-->

# ADR-0021 — AI+Human ontologies: vocabularies as governed claims

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. The v1 slice is
[FR-19](../product/roadmap.md); every other commitment below carries its own roadmap handle.

## Context

Crews that share memory need to share words. The moment two crews cite "churn-risk" and mean
different things, recall returns the wrong memories with perfect provenance. The classical answer
is an ontology service — and an ontology service, reduced to essentials, *is* a governed memory
provider for vocabulary: a term is a claim someone proposed and someone else accepted; deprecation
is supersession; a release is a pinned snapshot; a mapping is a cross-reference that needs
authority on both sides. OML already has every one of those mechanisms. This record therefore
hosts vocabularies **as claims** — adding identifier discipline, three artifact kinds, one scanner
plugin and a thin read surface — and never builds a second subsystem.

Two stances bind every decision below:

- **AI-first with humans in the loop.** Every role is designed to be held by an agent; human
  principals enter through the same roles and the same pipeline; and the human loop is placed by
  policy — `human_required_classes` ([admission](../architecture/admission.md)) — never by a
  parallel mechanism.
- **On top of the provider floor.** Every vocabulary feature defaults off; with all of them off,
  OML is a full memory provider (the FR-10..12 floor, [roadmap](../product/roadmap.md)). The
  features only ever extend that base: ordinary writes gain zero latency and zero new obligations
  unless a workarea's policy opts in. A memory provider on steroids — the steroids are additive.

## Decision

- **d1 — Terms are claims; vocabularies are workareas.** Three new artifact kinds — `term`,
  `term_mapping`, `vocab_release` — join memory, derived_fact, steering and the reserved
  `dataset_manifest`. A vocabulary is a dedicated workarea subtree
  ([workareas](../architecture/workareas.md)); the term lifecycle rides the one admission
  pipeline: pending is `held`, active is a promoted tip with `term_status: active`, deprecated is
  a promoted tip with `term_status: deprecated`. `term_status` is a **structural** clear
  payload/metadata field on term submissions, never body text, so the deprecation marker survives
  crypto-shredding and is deterministically readable. No new storage subsystem, no second
  pipeline.

- **d2 — Identifier discipline: CURIEs over a journalled, tenant-scoped prefix registry.**
  `vocab_prefixes` (`tenant_id`, `prefix`, `workarea_id`, `registered_event_id`) with
  UNIQUE (`tenant_id`, `prefix`) and the standard RLS block — tenant-scoped, because a global
  unique would leak existence across tenants. The table is classified as a fold of the journal;
  registration happens in the vocabulary-workarea creation act (`workarea.create`'s payload gains
  an optional `prefix` field). Workarea creation is already admin-only, and that is the whole
  prefix brake — no new issuer-ceiling-like mechanism. Because registering a prefix is
  irreversible in meaning, the CLI confirms permanence before the act — the preview-every-mass-act
  rule of [cli/reference.md](../cli/reference.md), extended there to irreversible single acts. A term's
  `claim_key` is `PREFIX:localname`, and the permanence rule — an extension of the claim-lineage
  rules, owned by oml-domain and stated here at ADR level — is: a CURIE is **minted at first
  promotion**, and once minted it is permanent. The definition body may be crypto-shredded, but
  the kind=term lineage row and its `claim_key` are never fully erased, and the CURIE is never
  re-mintable with a different meaning. A never-promoted proposal — rejected or dead-lettered —
  releases the localname; its journal trail persists under the artifact id. A writer race on
  minting `PREFIX:name` resolves first-promoted-wins: the loser gets a typed `claim_key` conflict
  at capture, and its usable paths are edit-as-proposal on the winner, or a different localname
  plus an intra-vocabulary mapping (both endpoints in one vocabulary is legal; the mapping
  authority of d6 is trivially satisfied). `oml:` is a reserved prefix in every tenant's
  namespace: the provider-published upper vocabulary (`is_a`, `part_of`, `replaced_by`,
  `consider`, …) is shipped product data versioned with the release, not journal content
  ([RS-10](../product/roadmap.md)) — and the docs say so plainly.

- **d3 — Deprecation, never deletion — with the successor declared in the deprecating
  submission.** A term retires by supersession with a deprecation version
  (`term_status: deprecated`); its successor pointer is a ratified claim edge labelled
  `oml:replaced_by` (exactly one) or `oml:consider` (one or more) from the deprecation version to
  the successor term(s) — edges survive erasure, materialize into gold lineage, and get non-author
  ratification for free. The successor edge is declared **in** the deprecation submission, exactly
  as `derives_from` is: the edge rides the submission atomically, one serve and one verdict cover
  the version and the edge together, and the scanner's finding class sees the successor. It is
  never two separately ratified acts — a deprecation that promoted before its successor pointer
  did would be a dangling retirement, and the design refuses to let that state exist.
  `replaced_by` is safe for automated rewiring; `consider` requires judgment.

- **d4 — Releases are journalled pins, served as diffs — never copies.** Kind `vocab_release`:
  the body carries the vocabulary workarea id, the pinned `seq` (≤ the release submission's own
  seq), and a Merkle root over the then-active term set — leaf = (`claim_key`, version,
  `term_status`, plaintext_digest), so the pin covers definition bytes, not just identity. The
  Merkle construction is RS-4's one specification, shared with
  [ADR-0020](ADR-0020-dataset-onboarding.md). Trunk vocabulary lives at `committed`; releases
  accumulate as claims; `validated` (frozen) is vocabulary retirement — a freeze, not a
  revocation: terms stay resolvable forever, and nothing retro-invalidates past verdicts, because
  every scan verdict recorded the release id and `scanner_version` it was taken against (d8). At
  release ratification the `is_a` closure over ratified edges is **materialized** into a
  per-release closure table: traversal and expansion become lookups over precomputed,
  journalled-provenance rows (the serialize-don't-compute contract holds); the storage cost is
  O(terms × depth) per release, priced and bounded by release cadence. And because a human cannot
  judge a Merkle root, the **serve of a `vocab_release` is a diff** against the previous release —
  terms added, deprecations with their successors, `is_a` edges added and removed, counts — and
  the diff artifacts enter the served digest set, so the evidentiary contract records exactly what
  the reviewer saw ([admission](../architecture/admission.md)).

- **d5 — Hierarchy is ratified claim edges, with cycle refusal as a domain invariant — over
  `replaced_by` too.** Term edges are typed from `oml:` (the edge-label vocabulary is open today;
  this closes it for term edges). Cycle refusal is a domain invariant at ratification, like the
  supersession tip-CAS: a deterministic transaction-time check in oml-domain over the ratified
  `is_a` edges plus the candidate, refusing with typed `vocab.cycle_refused`
  ([errors](../api/errors.md)). The invariant extends to `replaced_by` edges: a deprecation whose
  successor pointer would close a replacement cycle is refused the same way. Supersession *chains*
  forming over time are legitimate; a successor that is itself already deprecated at ratification
  time is not refused but surfaced as the scanner finding class `vocab.deprecated_successor` —
  judgment, not law. The invariant narrows, so it is constitution-compatible; it lives in
  oml-domain, so it is an ADR-level rule, not a plugin.

- **d6 — Mappings are SSSOM-shaped claims, never merges.** Kind `term_mapping`: subject CURIE,
  object CURIE, predicate ∈ {`exactMatch`, `closeMatch`, `broadMatch`, `narrowMatch`,
  `relatedMatch`}, justification ∈ {human-curated, lexical, logical}, note. Authority: this
  **extends** the workarea-edge rule ([workareas](../architecture/workareas.md)) to term_mapping
  claim edges — an extension, not a reuse — because a ratified mapping becomes shared structure of
  two vocabularies: ratification requires dreamer(review) on **both** endpoints' vocabularies and
  ratifier ≠ proposer. Both terms persist; a mapping is revocable by supersession. Cross-tenant
  mappings are [RS-3](../product/roadmap.md), future.

- **d7 — AI-first, human-in-the-loop, placed by policy — and "human" is an attestation.** Any
  writer key proposes (agent or human; provenance distinguishes); dreamer crews review at AI
  speed; the human loop is placed by `human_required_classes`. The mechanism: `vocab_release`
  routes natively (classification by kind); the two body-field classes route through the vocab
  scanner's deterministic finding classes — `vocab.deprecation_replaced_by`,
  `vocab.mapping_exactMatch` — which the shipped default policy names in
  `human_required_classes`. The coupling is stated: the human gate for those two acts is only as
  strong as the scanner being configured; scanner selection is journalled
  ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d4), so an auditor can always answer
  "was the human gate active" — auditable, not unmissable. A **human verdict rests on a
  journalled, issuance-time attestation**: a key/principal attribute `held_by: human`, attested by
  the issuing admin and shown in evidence exports; `human_required_classes` is satisfied only by
  verdicts from keys carrying it. The honesty boundary rides in the same breath: the server
  verifies the attestation, never the humanness
  ([roles-and-keys](../architecture/roles-and-keys.md)). Collusion visibility covers this surface
  too: the reciprocal-approval statistics explicitly cover **edge ratifications**
  (ratifier↔proposer pairs) and slice by kind, and the evidence export gains per-vocabulary churn
  — terms added and deprecated per release, median vocabulary review latency.

  The shipped defaults (every row tenant-tunable):

  | Act | Shipped default |
  |---|---|
  | new term | AI-only |
  | term edit | AI-only |
  | `is_a` / `part_of` ratification | AI-only (batch-gated at release) |
  | deprecation with `replaced_by` | HUMAN |
  | deprecation consider-only | tenant-choice (shipped AI-only) |
  | `exactMatch` mapping | HUMAN (tenant-tunable for campaigns) |
  | `closeMatch` / `broadMatch` / `narrowMatch` / `relatedMatch` | AI-only |
  | `vocab_release` | HUMAN, serve = diff |
  | vocabulary retirement | existing two-party status ceremony |
  | zero-human configuration | blessed under the d12 conditions |

- **d8 — Validation is a scanner plugin; typing is annotation; determinism is pinned.** Claims
  gain an optional `terms: [CURIE, …]` field — clear, non-erasable annotation metadata: it enters
  `signed_bytes`, semantically labels even erased claims, and is listed in the erasure inventory's
  attribution row. Workarea policy gains `vocabulary_policy {vocabulary, release, enforcement:
  require | annotate | off}` — **default off** ([admission](../architecture/admission.md)). Grant
  shape is ordinary: a vocabulary workarea grants writer to the crews it serves, through the same
  grant mechanics as any workarea — no vocabulary-specific grant kind. The vocab scanner checks
  that cited CURIEs exist and are active against the **pinned release**, whose id the scan verdict
  records alongside `scanner_version`; `enforcement: require` is refused without a pinned release
  — trunk-current checking is annotate-only, recorded as the fold at submission seq. The two modes
  have intended users: `require` is for stable, pinned vocabularies; an evolving crew runs
  `annotate`, where citing a not-yet-active term records the finding `vocab.term_pending`
  (distinct from `vocab.unknown_curie`: the CURIE exists but is held) and never blocks — submit
  now, re-cite after promotion. `vocab.inactive_curie` carries the successor CURIE(s) and relation
  in its error details, so a one-hop self-heal is mechanical. CURIE citations fail closed within
  the writer's effective read scope, exactly as `derives_from` does. The scanner can hold or veto,
  never promote (plugin law) — and it is **not in the default scanner set**: a tenant that never
  registers a vocabulary never runs it (equivalently: it is an O(1) no-op on submissions with no
  `terms` field under no `vocabulary_policy`).

- **d9 — A thin read surface, on top of the provider floor.** Four read-only endpoint groups —
  `GET /v2/vocab/suggest?q&workarea_id` (deterministic prefix+label typeahead over granted
  vocabulary workareas); `GET /v2/vocab/terms/{curie}`, returning `term_status` and successor
  pointers so a client holding a deprecated CURIE self-heals without a second lookup;
  `GET /v2/vocab/terms/{curie}/hierarchy?direction=parents|children|ancestors|descendants`
  (lookups over d4's per-release closure table); `GET /v2/vocab/mappings?term=` — all
  reader-ceiling, cursor-paginated per the convention, failing closed as absence
  ([http-api](../api/http-api.md)). MCP: **one** new tool, `oml_vocab`, with an action field
  (`suggest | get | hierarchy | mappings`), keeping the catalog at ten tools — owned as a count
  change in [ADR-0014](ADR-0014-http-api-and-mcp-facade.md)'s terms. Proposing terms rides
  `oml_remember` (kind `term`). Recall gains a `terms: [CURIE, …]` filter and
  `expand_terms: none | descendants` (default `none`); expansion resolves per CURIE against the
  release pinned by the vocabulary that owns the expanded prefix; steering order, `k` and
  `max_tokens` (FR-11) are untouched — expansion widens a filter before retrieval, never reorders
  results. Term, term_mapping and vocab_release rows are **excluded** from the shared FTS and
  vector indexes by default (a vocabulary workarea's policy may opt in, mirroring ADR-0020 d11);
  `RecallRequest.kinds` already excludes them — kept deliberate and stated.

- **d10 — What this is not.** No annotation or NER service: `suggest` is deterministic matching,
  and interpreting claim content to propose terms is client-side crew work. No reasoner: no OWL
  semantics, only the materialized transitive closure of ratified `is_a`. No auto-import of
  external ontologies in v1: an external ontology enters as an
  [ADR-0020](ADR-0020-dataset-onboarding.md) dataset import whose mapping spec declares
  `kind=term` (the spec gains a kind mapping as a cheap reservation) — post-v1, after FR-13; the
  import profile is [RS-11](../product/roadmap.md). Cross-tenant vocabularies:
  [RS-3](../product/roadmap.md).

- **d11 — Curation economics are structural.** A vocabulary workarea requires a named curator
  crew at creation. `oml doctor` flags a vocabulary with no active dreamer grant — the orphaned
  vocabulary is the junk-drawer failure mode, surfaced, and the flag names "propose freeze" as the
  next action. Doctor also warns on review availability: a workarea whose policy names
  `human_required_classes` with no attested-human dreamer key in scope is flagged before the queue
  silently fills. Vocabulary review load lands on [FR-2](../product/roadmap.md)'s measured
  throughput. Proposal floods are bounded by what already exists, not by a new mechanism:
  journalled rate and quota grant attributes, `held_item_expiry`, and queue depth in `oml status`
  and `oml doctor`. And simulations cannot mint vocabulary: vocabulary workareas refuse
  `provenance=simulation` submissions of kind term/term_mapping/vocab_release (a deterministic
  scan-class veto) — otherwise a demoted simulation row would squat a permanent CURIE forever.

- **d12 — The zero-human configuration is blessed, under four named conditions.** A 100%-agent
  tenant may tune every human_required class off and run vocabularies end to end with zero humans.
  The documentation blesses that configuration explicitly, under: (1) the `replaced_by` cycle
  invariant of d5, which is unconditional; (2) releases are never auto-cut on cadence — a release
  is always an explicit ratified act; (3) d7's statistics coverage — edge ratifications in the
  reciprocal-approval pairs, sliced by kind — present in the export; (4) the doctor orphan and
  churn signals active. Under those four, vocabulary harms degrade to visible, auditable and
  reversible — the same posture ordinary claims already hold.

## Consequences

- **Surface summary.** Schema: `vocab_prefixes` (tenant-unique, RLS, a fold of the journal);
  `memories.terms text[]`; the structural `term_status` payload field; the per-release closure
  table; kinds enums gain `term`, `term_mapping`, `vocab_release` at every enumeration site
  (OpenAPI enums ×4 with `RecallRequest.kinds` deliberately unextended, MCP `oml_remember` kinds, schema comment, silver comment —
  [postgres-schema](../data/postgres-schema.md)). Events: `workarea.create` payload gains
  `prefix`; submission kinds extend; **no new event families**
  ([event catalog](../data/event-catalog.md)). Errors: a new `vocab` family —
  `vocab.cycle_refused` (409), `vocab.prefix_conflict` (409), `vocab.unknown_curie` (422),
  `vocab.inactive_curie` (422, details: `successors[]`, `relation`); counts updated mechanically
  ([errors](../api/errors.md)). Scanner finding classes (not API errors): `vocab.term_pending`,
  `vocab.deprecation_replaced_by`, `vocab.mapping_exactMatch`, `vocab.deprecated_successor`. API:
  four read-only endpoint groups plus the recall `terms`/`expand_terms` additions; MCP +1 tool
  (ten total). Plugin: the vocab scanner, whose finding classes double as human-gate routes.

- **The honest costs.** The one non-zero default cost to a tenant that never touches vocabularies
  is **+1 MCP tool** in every `tools/list` — on the order of 100 tokens — accepted and stated:
  tenant-conditional catalogs would break ADR-0014's `tools/list` contract. With shipped defaults,
  an active vocabulary generates **≈7 human verdicts per week** — a design estimate, not a
  measurement; the measured figure is FR-20's to produce. The campaign path
  for bursts is a journalled temporary policy change (for example, tuning
  `vocab.mapping_exactMatch` out of `human_required_classes`), reverted after; batch-serve
  ergonomics for mapping campaigns is a possible future FR, not v1. The `exactMatch` blast radius
  in v1 is **client-side only** — expansion is descendants-only, so a wrong mapping misleads
  clients that chose to follow it, not recall itself — stated honestly; it stays human-required
  anyway, because a ratified mapping is shared structure of two vocabularies and future features
  build on it.

- **Roadmap rows opened**: [FR-19](../product/roadmap.md) (vocabulary v1 slice: prefixes, terms,
  deprecation edges, suggest, the validation scanner, releases + closure);
  [FR-20](../product/roadmap.md) (expansion and mapping quality measurement, including the verdict
  load); [RS-10](../product/roadmap.md) (the `oml:` upper vocabulary content);
  [RS-11](../product/roadmap.md) (the external-ontology import profile over ADR-0020, plus the
  mapping spec's kind field).

- **ADR-0020 touch**: the import mapping spec gains a `kind` mapping; RS-4's Merkle specification
  now also covers release pins.

## Rejected alternatives

- **A separate ontology subsystem or database.** Every property the gate guarantees — non-author
  review, journalled history, erasure with an inventory, tenancy, RLS — would have to be rebuilt
  and then kept consistent with the original. Terms-as-claims gets all of it by construction, and
  the one pipeline stays the one pipeline.
- **OWL reasoning.** A reasoner makes promotion latency and refusal semantics depend on inference
  over an open-world logic — unpriceable and unexplainable in an evidence export. The materialized
  `is_a` closure answers the queries agents actually ask, as lookups.
- **An annotation/NER service.** Interpreting content is model work, and the provider runs no
  model ([admission](../architecture/admission.md)); it would also put the provider in the
  author's seat for the annotations. Suggest stays deterministic; interpretation is client-side
  crew work.
- **Auto-cut releases on a cadence.** A release that nobody decided is a pin nobody attests;
  cadence-cut releases would also gut d12's zero-human blessing, which leans on releases being
  explicit ratified acts.
- **Global prefix uniqueness.** A cross-tenant UNIQUE on prefixes leaks namespace existence
  across tenants through registration failures. Uniqueness is per tenant; cross-tenant meaning is
  RS-3's problem.
- **Per-row vocabulary without identifier permanence.** Terms as ordinary claims with erasable,
  re-mintable keys would let `PREFIX:name` change meaning silently — the one failure an identifier
  system exists to prevent. Permanence-at-first-promotion is the smallest rule that closes it.
