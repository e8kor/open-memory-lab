<!--
id: adr/ADR-0009-status-ladder
tier: engineering
status: accepted design
answers: why a workarea's status is a forward-only ladder of development, committed, validated, and why medallion words stay in the lake
-->

# ADR-0009 — The status ladder: development, committed, validated; forward only; medallion words stay in the lake

**Status:** accepted design · 2026-10-29

## Context

A workarea mirrors a crew's work, and that work matures: it is explored, then governed, then
finished. A single terminal "archived" or "retired" state cannot express that arc; OML needs
three stages a lifecycle with only an end cannot say:
a stage where a tenant may deliberately relax review inside a bounded, marked exception
([ADR-0001](ADR-0001-the-constitution.md) d5); a working stage with full review; and a frozen
stage that an auditor can rely on — "what this area held when it froze is what it holds now,
minus journalled demotions and erasures". This record decides the ladder. Its rules, the
transition matrix and the per-status refusal matrix are declared on
[architecture/workareas.md](../architecture/workareas.md); this record states the decisions and
what was rejected. Nothing here is built; every refusal named below is a design
commitment with a typed error in the taxonomy ([api/errors.md](../api/errors.md)), not a property
of running software.

## Decision

- **d1 — Three statuses, and no others.** `development`, `committed`, `validated` (frozen).
  `development`: review runs as everywhere, and tenant policy MAY additionally permit marked
  `self_signed` rows under the one bounded constitutional exception, scoped in
  [ADR-0001](ADR-0001-the-constitution.md) d5 and widened nowhere. `committed`: full non-author
  review required; promotions allowed; the permanent status of the universe root. `validated`:
  frozen — refuses new writes, promotions into it, reassignment in or out, child creation and
  re-parenting; serves reads.

- **d2 — One write-eligibility rule across all statuses.** Who may write into a workarea is
  decided by effective access ([ADR-0008](ADR-0008-workarea-algebra.md) d4) and by nothing else.
  **Status changes only the review policy applied to what is submitted — never grant
  resolution.** `validated`'s write refusal is the admission policy of that status, received
  identically by every holder of write access as a typed error, not an edit to anyone's grants.
  Audit questions about access are answered from grants and permission epochs alone; audit
  questions about a refused write are answered from status and policy snapshots alone.

- **d3 — Transitions are forward only, proposed and ratified by different identities.**
  `development` → `committed` → `validated`, proposed by an admin or a dreamer with effective
  access on the node, ratified by a **second, non-proposing** dreamer or admin — R2 applies to
  transitions — and journalled with a snapshot of the review policy in force, so any later
  promotion audit is answered against the policy that actually governed it. Backward transitions
  do not exist: no verb, journalled or otherwise, moves a workarea down the ladder. Work that
  must continue after validation is a new child elsewhere in the tree, typically with a
  `continues` edge.

- **d4 — The child-status constraint.** An area may become `validated` only when every child is
  already `validated`, and creating a new child under a `validated` area is refused. Together
  these make "frozen" mean the whole subtree, by construction rather than by patrol: a validated
  node can never acquire an unfrozen descendant.

- **d5 — A frozen area admits exactly three mutations, all journalled.** Demote, erasure
  (crypto-shred, [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md)) and crew-metadata update
  ([ADR-0008](ADR-0008-workarea-algebra.md) d7). The first two exist because governance and legal
  reality do not freeze — a claim found wrong is demoted, a claim that must go is erased, each
  under its own journal event — and the third because freeze covers claim content and promotions,
  not workarea metadata. Everything else a frozen area is asked to do is refused.

- **d6 — Medallion words name physical lake zones only.** `bronze`, `silver`, `gold` are the
  names of Delta Lake zones, declared in [architecture/storage.md](../architecture/storage.md)
  and [data/delta-tables.md](../data/delta-tables.md), and nothing else. The status ladder keeps
  its own words. No page, error, log line or CLI output may call a workarea "gold" or a lake zone
  "validated". The two vocabularies touch at exactly one point, stated where the lake is
  declared: unreviewed content never enters a consumer-readable zone.

## Consequences

- `status` is one column on `workareas` ([data/postgres-schema.md](../data/postgres-schema.md)),
  and every refusal in d1 is a typed error naming the status rule it enforces
  ([api/errors.md](../api/errors.md)). Transition events with policy snapshots join the event
  catalog ([data/event-catalog.md](../data/event-catalog.md)).
- d2 keeps a single source of access truth. Nothing recomputes grants on transition, so freezing
  a large subtree is a metadata act, not a permission migration — and conversely, revoking a
  crew's access to a frozen area is an ordinary grant act, unrelated to status.
- d3's second ratifier makes a status change cost two identities. A one-admin tenant cannot
  freeze an area alone; that is the same liveness cost R2 imposes on promotion
  ([ADR-0001](ADR-0001-the-constitution.md)) and it is accepted for the same reason: a frozen
  area is an evidence claim, and an evidence claim signed by one identity that also proposed it
  is worth less to the auditor reading the export
  ([architecture/audit-and-lineage.md](../architecture/audit-and-lineage.md)).
- d4 orders the freeze of a deep tree leaves-first. An admin freezing a branch performs the
  transitions bottom-up; the CLI previews the affected set under its mass-operation dry-run
  default ([cli/reference.md](../cli/reference.md)).
- d5 commits the lake path: demote and erasure in a frozen area still flow through the outbox as
  retract operations ([architecture/storage.md](../architecture/storage.md)), so gold tables
  curated per validated workarea are not immutable snapshots — they are current state under
  journalled demotion and erasure, and the reconciler checks them as such
  ([architecture/reconciliation.md](../architecture/reconciliation.md)).
- d6 is a standing documentation and review obligation under the fixed vocabulary of
  [ADR-0018](ADR-0018-the-documentation-set.md): a surface that leaks "gold" for a workarea or
  "validated" for a zone is a defect.

## Rejected alternatives

- **Per-status grant models** — a second grant table or modifier set per status, so that
  `validated` areas would carry their own, narrower grants. Rejected because it creates two
  sources of access truth: every check would reconcile the grant state with the status-local
  state, every audit answer would depend on which table the checker consulted, and a transition
  would become a permission migration with its own failure modes. d2 keeps one truth —
  [ADR-0008](ADR-0008-workarea-algebra.md) d4 decides access, status decides policy — and the
  cost is that `validated`'s refusals must be implemented as admission policy rather than falling
  out of grant resolution.
- **Reversible freezing** — an unfreeze verb, journalled like any other act. Rejected because it
  voids the guarantee that is the point of freezing: "this area was validated on date D" is worth
  something to an auditor only if nothing promoted into it afterwards, and a journalled thaw
  window still forces every consumer of the export to verify that no thaw occurred — the
  guarantee degrades from a structural property into a query. Work that must continue gets a new
  child and a `continues` edge (d3); the frozen node stays frozen.
- **A fourth `archived` status** — a terminal state after `validated` for areas nobody should
  read anymore. Rejected because its two candidate meanings are already covered: stop serving
  the content — that is demotion of the rows, journalled and reversible per row; make the
  content cease to exist — that is erasure by crypto-shredding
  ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md)). Both are among d5's permitted mutations inside `validated`.
  A status that hides an entire area without doing either would be a read-path visibility switch
  with no journalled per-row account — exactly the silent disappearance R4 exists to prevent.
