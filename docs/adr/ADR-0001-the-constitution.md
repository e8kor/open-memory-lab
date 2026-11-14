<!--
id: adr/ADR-0001-the-constitution
tier: engineering
status: accepted design · amended by ADR-0025, ADR-0037, ADR-0030, ADR-0044, ADR-0080, ADR-0089
answers: what the four constitutional rules R1–R4 commit, and what each of the three bounded exceptions permits, marks and requires: an in-progress subtree's held rows to the keys in it and on its ancestor chain, own-root same-person approval, and an admin's fixes with the admin read
-->

# ADR-0001 — The constitution: four rules, three bounded exceptions

**Status:** accepted design · 2026-10-29 · amended by [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md), [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) (d5, same-person approval named, 2026-11-14), [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) (d5, the in-progress exception widened, 2026-11-14), [ADR-0089](ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) (d5, 2026-11-14)

**Release:** partly in v0.1.0 and v0.6.0; the rest not built ([releases](../changelog.md))

## Context

Open Memory Lab's governing rules are the product: four rules that every surface, schema and
service exists to enforce, plus three bounded exceptions. They are stated in
OML's own terms — roles, workareas, journalled tenant policy. This record is the single
declaration site for the four rules, R1–R4, and for their exceptions; every other page links
here and restates nothing. The rules were written before any code: each is a design commitment
the build is held to. The Release line names the releases that built parts of them and says the
rest is not built; [releases](../changelog.md) says which part each release built.

## Decision

- **d1 — R1, the verb ceiling.** The surface an agent can call never promotes, signs, demotes
  or deletes. Each role's reachable verbs are a ceiling, enforced per role and per transport:
  the role matrix is owned by [roles and keys](../architecture/roles-and-keys.md), and the MCP
  facade exposes only the agent-safe verb subset
  ([MCP plugin](../api/mcp-plugin.md)).

- **d2 — R2, the reviewer is never the author.** Enforced at promotion time against the
  author **and** the derivation lineage closure of the row — pending and ratified edges alike —
  not merely at key issuance. A dreamer whose key or principal appears anywhere in that closure
  is refused the verdict. The rule extends to workarea edges: the ratifier of an edge is never
  its proposer ([workareas](../architecture/workareas.md)). Enforcement mechanics live in
  [admission](../architecture/admission.md).

- **d3 — R3, the gate changes visibility, not ranking.** Nothing is readable by others until
  it is ratified. Review never reorders results, and steering rows, which precede hits, are
  never ranked ([retrieval](../architecture/retrieval.md)). Its bounded exceptions are d5's.

- **d4 — R4, no silent deletion.** Erasure is a journalled act performed by crypto-shredding;
  demotion keeps the bytes. The erasure inventory and its honest boundary are owned by
  [audit and lineage](../architecture/audit-and-lineage.md).

- **d5 — Three bounded exceptions, scoped here and nowhere else: loose in development, strict in
  production.** The organising principle (the operator, 2026-11-14: "lets make sure decisions are
  loose for in-development\in-progress areas, but strict once we are publishing and preparing for
  production"):
  - **Development scopes**, a `development` workarea and an in-progress subtree, carry the
    relaxations below: a team sees held work, a person approves their own agents' work in their
    own root, and an admin's fix is confirmed by its own person.
  - **Production scopes**, a `committed` or `validated` workarea, a published release, and
    everything a key bound to `latest` or a pin reads, are strict: every verdict comes from an
    identity outside the version's lineage that is never the author's person, a reviewer crew's
    agent key included (a dreamer grant under the review policy, not an agent-surface verb; R1
    unchanged) for the knowledge class (the operator, 2026-11-14: "Agents approve knowledge"); a
    verdict on the instruction class (ADR-0080 d16), every admin-fix confirmation (ADR-0089 d8)
    and every release publication (ADR-0085 d4) come from a different person; and no unreviewed
    content is served, except an admin fix's marked overlay, which a different person must
    confirm.

  Outside the three exceptions R2 and R3 hold without exception. Each is named here on the day it
  is made, marks what it serves indelibly, and is widened only by a new decision on this record.
  *(Amended 2026-11-14 by the operator: the development self-signing exception of 2026-10-29,
  never built, is retired, and this list replaces the earlier amendments.)*

  1. **In-progress sharing** ([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md) d2, d3, widened by
     [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d16). Of R3's first sentence only, in development scopes only.
     Inside a workarea a person put in progress, a held version of any kind is served, and an
     instruction offered, marked unreviewed, to keys with reader access scoped in that subtree or
     on its ancestor chain and bound to `tip` (ADR-0085 d6); never to a sibling, another branch,
     federation or a wide source, which read only promoted rows of committed or validated
     workareas or a published release; never a version the scan held or has not scanned, and never
     a held retirement. R2 holds there without exception.
  2. **Own-root same-person approval** ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) d8; ADR-0045 d11,
     d20; ADR-0046 d2; ADR-0055 d7; ADR-0082 d7). Of R2 at the person level: R2 excludes the keys
     and principals of a row's lineage, not its person, so in a person's own root their review key may approve a write by any of their own keys,
     agents' included, and may clear a scan hold finding on such a write (ADR-0074 d26, accepted
     2026-11-14 within this scope). Every such approval is marked `same_user` to every reader
     (ADR-0077 d2). A user root never leaves development (ADR-0085 d17), so the whole root is a
     development scope. Outside the root, a version any key of a person wrote
     counts no verdict and no hold clearance from any key of the same person (ADR-0082 d7).
  3. **Admin fixes, with the admin read** ([ADR-0089](ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d14;
     [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21). Of R2's order and R3's first sentence, for admin keys
     only. A fix holds only while every one of these is true: the admin key is `held_by: human`;
     a reason is given; the act is one of ADR-0089 d2's fix acts (demote, edit, retire, release
     rollback, import rollback, erasure), except that a `validated` workarea refuses an edit or a
     retirement (ADR-0089 d12); every reader sees the fix's mark (ADR-0089 d7); the act is
     journalled and queued for confirmation; and, while unconfirmed, the version is excluded from
     every release cut. Such a fix is served before any non-author review and is confirmed or
     reverted afterwards: in a development scope by any person holding dreamer there, the admin's
     own person included, marked `same_user`; in a production scope, and for a published
     release's overlay, only by a person other than the admin's own (never `same_user`) (ADR-0089 d8). The conditions above
     govern fixes. Separately, any admin key reads claim text, held rows included, tenant-wide, on
     the admin routes alone, never on an agent surface, each read journalled and bound to an
     investigation's reason (ADR-0089 d10). That read is the relaxation of R3 that ADR-0045 d21
     already made, recorded here.

  R1 has no exception: no agent key gains anything under any of the three. ADR-0088, deferred,
  would be the fourth.

- **d6 — The honest limit on R2.** Undeclared derivation — a writer omitting `derives_from` —
  is not detectable by the service. Lineage-closure exclusion is exactly as good as the
  declared graph, and every page that claims closure enforcement says so plainly. The
  documentation discipline that holds this sentence in place is
  [ADR-0018](ADR-0018-the-documentation-set.md).

## Consequences

- Every architecture page enforces or links these rules; none restates them. The verb ceiling
  binds [roles and keys](../architecture/roles-and-keys.md), the
  [HTTP API](../api/http-api.md) and the [MCP plugin](../api/mcp-plugin.md); closure
  enforcement binds [admission](../architecture/admission.md); the in-progress exception's
  workarea mechanics bind [workareas](../architecture/workareas.md).
- Each exception's mark is a schema commitment: `unreviewed` on every served held item,
  `same_user` on every own-root approval and every development-scope confirmation of a fix, and
  the fix's mark (ADR-0089 d7) on every fix. The `self_signed` mark is never set; every record
  that described it carries a dated note that the development self-signing exception is retired
  (swept 2026-11-14).
- R2's cost is liveness: review is a dependency, and the system fails closed when no eligible
  reviewer exists — a held item waits or expires under its workarea's review policy
  ([admission](../architecture/admission.md)). Review throughput is unmeasured; the
  [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap) carries the review-throughput issue.
- d6 is a standing documentation obligation: a page claiming lineage-based guarantees without
  the undeclared-derivation caveat is a defect under
  [ADR-0018](ADR-0018-the-documentation-set.md).
