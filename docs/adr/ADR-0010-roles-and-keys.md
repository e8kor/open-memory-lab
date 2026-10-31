<!--
id: adr/ADR-0010-roles-and-keys
tier: engineering
status: accepted design · amended by ADR-0025
answers: why exactly five roles exist; how API keys are issued, scoped and banned; where provider-level bootstrap authority lives
-->

# ADR-0010 — Roles and keys: reader, writer, dreamer, admin, operator

**Status:** accepted design · 2026-10-29 · amended by [ADR-0025](ADR-0025-the-agent-only-memory-surface.md)

## Context

OML authenticates agents with API keys that carry a role and workarea-scoped grants. OML is
multi-tenant, has no web UI, and its callers are agents and crews first; the administration surface is a CLI
speaking the same HTTP API. The role set, the issuance rules, the provider-level bootstrap and
the ban mechanism all need one record. The role-by-verb matrix, grant resolution and deny
semantics are owned by [roles and keys](../architecture/roles-and-keys.md) and
[workareas](../architecture/workareas.md); this record makes the decisions those pages document.
Everything here is decided, not built.

## Decision

- **d1 — Five roles, each a verb ceiling.** `reader`, `writer`, `dreamer`, `admin` at tenant
  level, `operator` at provider level. A role is a ceiling on reachable verbs
  ([ADR-0001](ADR-0001-the-constitution.md) d1), never a hint to a handler: reader recalls,
  reads, searches and reads graphs within scope, with every read logged
  ([retrieval](../architecture/retrieval.md)); writer remembers and learns (held on arrival),
  uploads attachments, proposes supersessions of its own claims and writes steering rows (held
  and reviewed like any claim), and never records a verdict; dreamer proposes and reviews (d5);
  admin administers and never reviews (d6); operator provisions tenants and never touches
  content (d2). The full matrix lives in
  [roles and keys](../architecture/roles-and-keys.md).

- **d2 — Operator role and bootstrap ceremony.** The operator is the provider-level role: it
  creates tenants and issues each tenant's first admin key, and it can never write, read or
  review tenant content — the provider's own staff sit under the same verb ceiling as any
  caller. Bootstrap: on first start, a Helm job mints a show-once root operator key; the journal
  records the issuance as issued-without-value (the key value appears exactly once, on the
  operator's terminal, and is never stored or journalled). The ceremony transcript is mocked up
  in [CLI sessions](../cli/sessions.md).

- **d3 — Issuer ceiling.** A key is issued with a role no higher than its issuer's. Operator
  issues operator keys (its own rotation path) and each tenant's first admin key; admin issues
  reader, writer, dreamer and admin within the tenant. There is no escalation path by issuance: authority can be delegated
  sideways or down, never up. Issuance and revocation are journalled acts; key values are shown
  once and stored only as hashes.

- **d4 — Bans are journalled admission filters.** An admin may ban an author or a source, and
  lift the ban; both are journalled acts naming who acted and on what. A ban is checked at
  admission — a banned identity's submission is refused at the door, not silently dropped
  later — and bans never reach backward to alter what was already promoted (demotion is a
  separate, journalled act). The CLI previews a ban's reach with `--dry-run` before it takes
  effect ([CLI reference](../cli/reference.md)).

- **d5 — Dreamer is one role with two capability classes, split at promotion.** Class (a),
  propose: edit-as-proposal (a new attributed submission carrying `derives_from`) and connection
  proposals, claim↔claim and workarea↔workarea. Class (b), review: verdicts and connection
  ratification. The split is enforced where it matters — at promotion time: a dreamer's verdict
  is refused when that dreamer's key or principal appears anywhere in the row's lineage closure,
  pending and ratified edges alike ([ADR-0001](ADR-0001-the-constitution.md) d2), and a
  dreamer's own proposals need another reviewer. One key may legitimately hold both classes;
  what it may never do is exercise both on the same lineage. The closure check is only as good
  as the declared graph (ADR-0001 d6).

- **d6 — Admin is not a reviewer.** Admin verdicts on content are refused by the server, and the
  rule is enforced structurally at issuance: the service refuses to mint or amend a key that
  combines the admin role with dreamer-class grants (typed refusal `access.role_exclusive`,
  [errors](../api/errors.md)) — a person holding both duties holds two keys. An
  admin needing to write an ordinary claim uses a writer-scoped key; that write is held and
  non-author-reviewed like any other. "Edit or create by hand" is this path and no other —
  free-form manipulation of content exists only inside the simulation channel
  ([ADR-0011](ADR-0011-simulations.md)), marked as such. Admin's own verbs — issue/revoke keys,
  create/relate/re-parent/status workareas, reassign claims, ban, demote, erase, crew metadata,
  run simulations — are enumerated in [roles and keys](../architecture/roles-and-keys.md).

- **d7 — Keys are the agent credential; identity metadata feeds the independence axis.** Every
  key is journalled with its tenant, role, grants and the key→crew→external-manager metadata
  that [ADR-0012](ADR-0012-admission-and-review.md)'s independence axis resolves at promotion
  time. Human sign-in (OAuth) for the CLI is a tracked research row on the
  [roadmap](../product/roadmap.md), written to the honesty discipline of
  [ADR-0018](ADR-0018-the-documentation-set.md) — spec revision and client support verified with
  a fetch date before any commitment; v1 ships bearer keys everywhere.

## Consequences

- The role matrix in [roles and keys](../architecture/roles-and-keys.md), the
  [HTTP API](../api/http-api.md) authorization table and the
  [MCP plugin](../api/mcp-plugin.md)'s agent-safe verb subset are all projections of d1; a verb
  appearing in one and not the others is a defect.
- d2 and d3 make key provenance a tree rooted at the bootstrap key: every key's issuance chain
  is reconstructible from the journal, which is what the evidence export's grants history
  serves ([audit and lineage](../architecture/audit-and-lineage.md)).
- d5's cost is paid at promotion time: every verdict computes a lineage closure. The closure
  query's cost at scale is unmeasured — the [roadmap](../product/roadmap.md) carries Postgres
  scale ceilings as an FR.
- d6 forces an admin who also works as a crew member to hold two keys. That is the point: the
  journal then shows which capacity acted, and reciprocal-approval statistics
  ([ADR-0012](ADR-0012-admission-and-review.md)) can see key pairs, not blurred identities.
- Bans checked only at admission (d4) mean a ban is not retroactive cleanup; an admin who wants
  the banned author's promoted rows gone demotes or erases them explicitly, each act journalled.

## Rejected alternatives

- **A single superuser role.** One root role that can issue, review, write and erase collapses
  R1 and R2 into trust in whoever holds it, and makes the journal's attribution worthless — every
  act reads "root". The five-role split costs ceremony (two keys for one person who does two
  jobs) and buys a journal in which capacity is legible and the reviewer-author separation is
  checkable, not asserted.
- **OAuth-only identity, including for agents.** Keys are the agent credential: agents and crews
  cannot hold an interactive sign-in, and token refresh against a third-party IdP puts that IdP
  in the write path's availability budget. OAuth for humans and the CLI is the future research
  row of d7, taken up under the verification discipline of
  [ADR-0018](ADR-0018-the-documentation-set.md), not assumed.
- **Dreamer as two separate roles (proposer, reviewer).** One crew member legitimately does
  both jobs; splitting the role forces every such member to hold two keys and tempts crews to
  share them, which destroys attribution. The separation that matters is not at the key, it is
  at promotion: the lineage-closure check (d5) refuses the one combination that violates R2,
  and lets every other combination stand.
