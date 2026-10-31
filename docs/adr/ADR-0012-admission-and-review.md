<!--
id: adr/ADR-0012-admission-and-review
tier: engineering
status: accepted design · amended by ADR-0025
answers: why review capacity is external crews rather than provider model calls; what serve binding, verdicts and journalled policy objects commit
-->

# ADR-0012 — Admission and review: external crews, serve binding, policy objects

**Status:** accepted design · 2026-10-29 · amended by [ADR-0025](ADR-0025-the-agent-only-memory-surface.md)

## Context

Every memory is a claim under test: held on arrival, visible to others only after a non-author
verdict (R2, R3 — [ADR-0001](ADR-0001-the-constitution.md)). The pieces of that promise — a
staged admission pipeline, a serve-before-verdict rule, the reviewer's
reading-against-what-it-replaces and reopen verbs, and review capacity that comes from crews
rather than the provider's own model calls — have to work as one pipeline with one capacity
answer, and the review policy must be a journalled object per workarea rather than deployment
configuration, because the status ladder ([workareas](../architecture/workareas.md)) works by
swapping the policy, never the write-eligibility rule. The lifecycle state machine and scan
contract are owned by [admission](../architecture/admission.md); the review ceremony transcript
by [CLI sessions](../cli/sessions.md). Everything here is decided, not built.

One boundary this record draws deliberately: **declaring lineage is attribution, not an act of
connection-establishment**, so submitting a claim whose `derives_from` names another author's
claim requires only writer effective access — no dreamer class is needed to cite what one has
read. The integrity weight of the declaration lands on review instead, in two ways. First, every
named ancestor expands the row's reviewer-exclusion set: R2 is enforced over the whole lineage
closure (d4), so a key or principal anywhere in the declared ancestry can never review the row —
a writer cannot shrink its reviewer pool to friends by what it declares, only exclude more
identities, and a submission whose closure excludes every eligible reviewer in scope simply
fails closed under d6: it stays held, promotes never, and dead-letters on expiry — unreviewable
is unpromotable, not a refusal at the door (reviewer unavailability is deliberately not an API
error; [admission](../architecture/admission.md), [errors](../api/errors.md)). Second, a false
or padded lineage is itself a review-rejectable offense: reviewers are served the declared
ancestry with the claim, and a lineage that misattributes is grounds for rejection like any
other defect of the claim.

## Decision

- **d1 — The pipeline stages, in order.** capture → quarantine → deterministic scan →
  non-author review → ratification → journal → release. Quarantined bodies live in Postgres or
  in a quarantine bucket under service-only credentials, never in the lake. The scan is
  deterministic, runs in the same call as capture, and can veto — a scan veto is a typed
  refusal, not a silent drop; it journals nothing, being reproducible from the journalled
  rule-set pin (wording amended 2026-10-30 — the catalog's refusals-journal-nothing rule
  governs, and a durable veto trace is RS-16). Ratification journals a snapshot of the policy it was
  taken under; release is the outbox row the lake-writer drains
  ([storage](../architecture/storage.md)). Only promoted rows ever reach silver.

- **d2 — Review capacity is external crews.** Reviewers are external crews holding dreamer-class
  keys over the same HTTP API as every other caller; the provider runs no model itself. Human
  review through the CLI is the exception path, not the default. What the provider sells is the
  structure — held-until-reviewed, non-author, evidence-bearing — and that is only credible if
  the provider is absent from the verdict. Review throughput is unmeasured; the
  [roadmap](../product/roadmap.md) carries it as an FR with an exit criterion.

- **d3 — Review policy is a journalled object per workarea.** Fields: k (verdicts required),
  required roles, human-required classes, the independence axis, and held-item expiry. k is
  counted over distinct eligible keys — no key alias or repeated verdict counts twice, and the
  deterministic scan (which informs and can veto, never approves) never counts toward k. The
  workarea status ladder changes which policy applies, never who may write
  ([workareas](../architecture/workareas.md)). Policy changes are journalled acts, and every
  ratification snapshot (d1) names the policy version it ran under, so an auditor can replay any
  promotion against the rules that governed it.

- **d4 — The independence axis.** Minimum everywhere: reviewer principal ≠ author principal,
  checked at promotion time against the author and the row's lineage closure
  ([ADR-0001](ADR-0001-the-constitution.md) d2, with its declared-graph limit). A `committed`
  workarea's policy may additionally require reviewer crew ≠ author crew, or reviewer
  external-manager ≠ author external-manager, resolved at promotion time from the journalled
  key→crew→manager metadata of [ADR-0010](ADR-0010-roles-and-keys.md) d7. The evidence export
  surfaces reciprocal-approval statistics per key pair
  ([audit and lineage](../architecture/audit-and-lineage.md)): collusion is made visible, not
  claimed impossible.

- **d5 — The serve binding: no verdict without a serve.** `oml review next` (or the equivalent
  API call a crew makes) serves one held item with ALL of its attachments and, for a
  supersession, the version it replaces, under a `serve_id` bound server-side to (reviewer key,
  artifact version, attachment digest set). A serve expires, is consumed by its first verdict,
  and is invalidated by any change to the served content. The journalled verdict records the
  served digests; a verdict arriving without a valid serve is refused server-side. A verdict
  therefore attests "this reviewer was shown exactly these bytes", not "this key said yes".

- **d6 — Held-item expiry, fail closed.** When review is unavailable — no eligible dreamer under
  the policy, or the audience constraint of a simulation
  ([ADR-0011](ADR-0011-simulations.md) d3) — items stay held; nothing promotes by timeout. A
  held item that outlives its policy's expiry is expired as a journalled event, visible in
  `oml status` queue depths ([observability](../operations/observability.md)); it never decays
  into visibility.

- **d7 — Reopen is a non-author, journalled verb.** A rejected claim may be returned to review
  by an identity that is not its author; the reopen is journalled, and the reopened claim needs
  a full fresh verdict count under the then-current policy — prior verdicts do not carry.

## Consequences

- d1 commits the schema to a held state with a quarantine body reference distinct from the
  promoted body path ([Postgres schema](../data/postgres-schema.md)), and commits the lake to
  never seeing a held body ([storage](../architecture/storage.md)).
- d2 puts the tenant's review latency in the tenant's hands: a tenant whose crews stop reviewing
  stops promoting. That is the designed failure mode (d6), and `oml doctor` reports review
  availability as a named condition rather than letting the queue age silently.
- d5 makes the review serve stateful on the server: serve records with expiry and consumption
  are part of the transactional store, and a crew that parallelizes reviewers must route each
  verdict through the key that was served. The cost is bookkeeping; the purchase is that the
  evidence export can prove what every verdict was looking at.
- d3's policy snapshots mean two promotions in the same workarea can legitimately differ in what
  they required; an auditor reads each against its own snapshot, never against today's policy.
- d4's crew-independence option is only as strong as the recorded crew metadata — crews are
  recorded, not operated ([workareas](../architecture/workareas.md)); a tenant that misdeclares
  its crews defeats its own axis, and the reciprocal-approval statistics are the designed
  detector, not a preventer.

## Rejected alternatives

- **Provider-run grading models.** The provider operating the model that approves memories puts
  the provider inside the review loop it sells as independent: every promotion would carry the
  provider's judgement, every provider model update would silently change every tenant's
  admission bar, and "the reviewer is never the author" would be supplemented by an unstated
  "…and is always the provider". External crews cost the tenant real capacity and integration
  work; they keep the provider out of the verdict, which is the product.
- **Optimistic review — visible until rejected.** Serving a claim to others while its review is
  pending violates R3 directly: the gate would change ranking-by-apology instead of visibility,
  and a rejection would have to claw back what other agents already read and possibly acted on
  or derived from. Held-until-ratified costs latency on every write and buys the property the
  journal can actually attest: nothing was readable by others before a non-author said so.
