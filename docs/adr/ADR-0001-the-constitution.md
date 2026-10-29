# ADR-0001 — The constitution: four rules, one bounded development exception

**Status:** accepted design · 2026-10-29

## Context

Open Memory Lab's governing rules are the product: four rules that every surface, schema and
service exists to enforce, plus one bounded exception for development work. They are stated in
OML's own terms — roles, workareas, journalled tenant policy. This record is the single
declaration site for the four rules, R1–R4, and for the one exception; every other page links
here and restates nothing. No code exists: each rule is a design commitment the build will be
held to, not a property of running software.

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
  never ranked ([retrieval](../architecture/retrieval.md)).

- **d4 — R4, no silent deletion.** Erasure is a journalled act performed by crypto-shredding;
  demotion keeps the bytes. The erasure inventory and its honest boundary are owned by
  [audit and lineage](../architecture/audit-and-lineage.md).

- **d5 — The one bounded exception, scoped here and nowhere else.** R2 and R3 hold
  unconditionally in `committed` and `validated` workareas. A `development` workarea MAY relax
  them by journalled tenant policy. Rows written under the relaxation are marked
  `self_signed`, are readable only inside that workarea, never promote beyond it, and never
  enter the silver or gold lake zones. The exception is named in the constitution on the day it
  is made, not discovered later; the mark is indelible; and a marked self-signature is recorded
  but never called a review. No other page may widen this scope; a change to it is a new
  decision on this record.

- **d6 — The honest limit on R2.** Undeclared derivation — a writer omitting `derives_from` —
  is not detectable by the service. Lineage-closure exclusion is exactly as good as the
  declared graph, and every page that claims closure enforcement says so plainly. The
  documentation discipline that holds this sentence in place is
  [ADR-0018](ADR-0018-the-documentation-set.md).

## Consequences

- Every architecture page enforces or links these rules; none restates them. The verb ceiling
  binds [roles and keys](../architecture/roles-and-keys.md), the
  [HTTP API](../api/http-api.md) and the [MCP plugin](../api/mcp-plugin.md); closure
  enforcement binds [admission](../architecture/admission.md); the exception's workarea
  mechanics bind [workareas](../architecture/workareas.md); the silver/gold exclusion of
  `self_signed` rows binds [storage](../architecture/storage.md) and the schema pages.
- The `self_signed` mark is a schema and lake commitment: the Postgres row and every lake row
  must carry it so the silver/gold exclusion is checkable by the reconciler, not asserted.
- R2's cost is liveness: review is a dependency, and the system fails closed when no eligible
  reviewer exists — a held item waits or expires under its workarea's review policy
  ([admission](../architecture/admission.md)). Review throughput is unmeasured; the
  [roadmap](../product/roadmap.md) carries the review-throughput FR.
- d6 is a standing documentation obligation: a page claiming lineage-based guarantees without
  the undeclared-derivation caveat is a defect under
  [ADR-0018](ADR-0018-the-documentation-set.md).
