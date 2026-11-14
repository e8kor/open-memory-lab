<!--
id: adr/ADR-0001-the-constitution
tier: engineering
status: accepted design · amended by ADR-0025, ADR-0037, ADR-0030, ADR-0080, ADR-0089
answers: what the four constitutional rules R1–R4 commit, and what each bounded exception permits, marks and requires: development self-signing, an in-progress subtree's held rows to the keys in it and on its ancestor chain, and an admin's fixes; and how an admin reads claim text
-->

# ADR-0001 — The constitution: four rules and their bounded exceptions

**Status:** accepted design · 2026-10-29 · amended by [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md), [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md), [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) (d5, the in-progress exception widened, 2026-11-14), [ADR-0089](ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) (d5, 2026-11-14)

**Release:** partly in v0.1.0 and v0.6.0; the rest not built ([releases](../changelog.md))

## Context

Open Memory Lab's governing rules are the product: four rules that every surface, schema and
service exists to enforce, plus bounded exceptions, the first for development work. They are stated in
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

- **d5 — The bounded exceptions, scoped here and nowhere else.** The first: R2 and R3 hold
  unconditionally in `committed` and `validated` workareas. A `development` workarea MAY relax
  them by journalled tenant policy. Rows written under the relaxation are marked
  `self_signed`, are readable only inside that workarea, never promote beyond it, and never
  enter the silver or gold lake zones. The exception is named in the constitution on the day it
  is made, not discovered later; the mark is indelible; and a marked self-signature is recorded
  but never called a review. No other page may widen this scope; a change to it is a new
  decision on this record. *(Amended 2026-11-05 — a second bounded exception, of R3's first
  sentence only, inside a workarea a person put in progress: [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md) d3. R2 holds there
  without exception. Widened 2026-11-14 by ADR-0080 d16: a held version of any kind in the
  in-progress subtree is served, marked unreviewed, to keys with reader access scoped in that
  subtree or on its ancestor chain and bound to `tip` (ADR-0085 d6); never to a sibling, another
  branch, federation or a wide source, never a version the scan held or has not scanned, and
  never a held retirement.)* *(Amended 2026-11-14 — a third bounded exception, of R2's order
  and R3's first sentence, for admin keys only, in every workarea status:
  [ADR-0089](ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d14. It overrides "R2 and R3 hold
  unconditionally in `committed` and `validated` workareas" for exactly this, and holds only while
  every one of these is true: the admin key is `held_by: human`; a reason is given; the act is one
  of ADR-0089 d2's fix acts (demote, edit, retire, release rollback, import rollback, erasure),
  except that a `validated` workarea refuses an edit or a retirement (ADR-0089 d12); every reader
  sees the fix's mark (ADR-0089 d7); the act is journalled and queued for a person of another
  principal to confirm or revert; and, while unconfirmed, the version is excluded from every
  release cut. Such a fix is served before any non-author review and is reviewed afterwards. The
  conditions above govern fixes. Separately, an admin key reads claim text, held rows included,
  tenant-wide, on the admin routes alone, never on an agent surface, journalled, and only when it
  is `held_by: human` (*operator to confirm*); with ADR-0089 d10 each read is bound to an
  investigation's reason. That read is the relaxation of R3 that
  [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21 already made, recorded here. R1 is
  unchanged: no agent key gains anything. No other record widens this exception.)*

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
  [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap) carries the review-throughput issue.
- d6 is a standing documentation obligation: a page claiming lineage-based guarantees without
  the undeclared-derivation caveat is a defect under
  [ADR-0018](ADR-0018-the-documentation-set.md).
