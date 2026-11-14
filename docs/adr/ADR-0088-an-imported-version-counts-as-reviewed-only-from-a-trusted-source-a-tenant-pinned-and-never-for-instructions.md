<!--
id: adr/ADR-0088-an-imported-version-counts-as-reviewed-only-from-a-trusted-source-a-tenant-pinned-and-never-for-instructions
tier: engineering
status: proposed
answers: whether a verdict given on another server may count on this one; what a trusted source server is and who pins it; what an import must verify before a source verdict counts; which kinds never count; why this is a constitutional amendment; and why it is deferred
-->

# ADR-0088 — An imported version counts as reviewed only from a trusted source a tenant pinned, and never for instructions

**Status:** proposed · deferred · 2026-11-14: left out when the operator accepted ADR-0084 to ADR-0087
and ADR-0089 the same day; it waits until ADR-0084 is built and the operator decides R2 on import

**Amends:** ADR-0001 (d5: a fourth bounded exception, of R2, after ADR-0089's third; for verified verdicts from a trusted source
server), ADR-0084 (d6: an import from a trusted source may land promoted members)

## Context

[ADR-0084](ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md)
lands every imported version held and has it reviewed again on the target. A team that publishes
on a staging server and imports each release into production reviews every item twice. This record
says when the first review may count on the second server. It changes what R2 means on the target,
where the reviewer is a key the target has never seen ([ADR-0001](ADR-0001-the-constitution.md) d2
checks the lineage closure against keys the server knows), so it amends the constitution and is kept
apart from ADR-0084 so that the bundle can be built without it.

"Trusted source server" here is a pinned server. It is a different thing from ADR-0072's trusted
workarea (`crates/oml-domain/src/federation.rs:29`, `is_trusted_source`). Every surface says
"trusted source server" in full.

## Decision

- **d1 — A tenant pins a trusted source server: server id, source tenant id, its journal public key
  and the source workarea path.** A person's dreamer key proposes the pin and another person's
  ratifies it, journalled as `import.source_pinned`. Admin keys never pin (R1). The signing key is
  deployment-wide (ADR-0073 d1), so the pin names the source tenant as well: a verdict from another
  tenant of the same server never counts.

- **d2 — A source verdict counts only when everything verifies.** All of these must hold:
  - the bundle verifies under ADR-0084;
  - the verdict's source event signature checks against the pinned key;
  - the event names the pinned tenant;
  - the verdict's key was `held_by: human`, not `same_user`, and not `self_signed`; *(Self-signing retired 2026-11-14 (ADR-0001 d5); no row carries the mark.)*
  - the reviewed digest equals the imported version's digest.

  A version failing any check lands held, as ADR-0084 d6 says.

- **d3 — Instructions never count: skills, guardrails, steering and tests are reviewed again on the
  target.** These four kinds tell the target's agents what to do or decide what the target's
  releases are held to, so the person answerable for them must be someone the target knows: a
  reviewer on another server cannot be asked, revoked or audited by the target, and a mistake in
  them acts on every agent at once rather than on one recall.

- **d4 — A counted version lands promoted, marked, inside an in-progress workarea, and is published
  only by a release on the target.** It carries `approver: "imported"` with the pin's id. The
  target's own release (ADR-0085 d4) is the act that serves it, by two persons on the target.

- **d5 — A rotated source key breaks the pin until it is pinned again; a doctor line names it.**

## Consequences

- ADR-0001 d5 gains its fourth bounded exception, numbered after ADR-0089's third, by reference to this record.
- Deferred: nothing here is built before ADR-0084 and the operator's answer to "may a verdict from
  another server count at all".

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Pin by server key alone** | The key is deployment-wide, so any tenant on the source could produce verdicts the pin would accept. |
| **Count verdicts for every kind** | Skills, guardrails, steering and tests direct agents or define gates; a reviewer the target cannot see is not enough for instructions. |
| **One admin pins** | A pin is standing trust that lets content arrive reviewed, for every future import; an admin's acts are single fixes, marked and confirmed afterwards (ADR-0089), not standing trust. |
| **Land promoted outside in progress** | It would be served the moment it is written, with no person on the target having acted. |
