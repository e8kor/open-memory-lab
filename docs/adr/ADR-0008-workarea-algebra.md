<!--
id: adr/ADR-0008-workarea-algebra
tier: engineering
status: accepted design · amended by ADR-0025
answers: why workareas are one algebra — per-tenant universe tree, node-id grants, deny semantics, edges — rather than several separate objects
-->

# ADR-0008 — Workarea algebra: per-tenant universe tree, node-id grants, deny semantics, edges

**Status:** accepted design · 2026-10-29 · amended by [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0026](ADR-0026-names-are-keys.md)

## Context

Where memory lives must itself be governed, and by one object, not several. Splitting the job —
say, between a flat operator-created container that bounds grants and review policy, and a path
inside it that an agent opens by writing into it — puts a seam through every write: multiple
checks fed by different folds, grants naming containers by string, and relations between areas
bolted on later. OML is multi-tenant
(ADR-0007), has no single operator, and must answer subtree questions — "grant this crew
everything under `experiments`", "freeze this whole branch" — that a flat set of named spaces
cannot express.
This record decides one object: the **workarea**, a governed node in one tree per tenant. A
workarea exists before it is written to — created by a journalled admin act, never conjured by a
typo, so a write naming an unknown workarea is refused — and a claim lives in exactly one
workarea. The rules this record decides are declared in full, with worked examples, on
[architecture/workareas.md](../architecture/workareas.md); this record states the decisions and
what was rejected. Nothing here is built.

## Decision

- **d1 — One tree per tenant, rooted at `universe`.** Every workarea of a tenant is a node in
  exactly one tree, rooted at that tenant's `universe` node. The universe root is permanently
  `committed`, cannot be frozen, deleted or re-parented, and refuses direct claim writes — claims
  live in crew workareas beneath it. It exists so that paths, subtree grants and subtree denies
  have a well-defined top, not so that anything is stored at the top. The tree never crosses a
  tenant boundary; cross-tenant "whole world" mapping is explicitly future (federation,
  RS-3 in the [roadmap](../product/roadmap.md)).

- **d2 — Inclusion is single-parent.** Every node except the universe has exactly one parent.
  Cycles are impossible by construction, every node has one derivable path from the root, and
  every subtree question — containment, grant coverage, deny masking, the child-status rule of
  [ADR-0009](ADR-0009-status-ladder.md) — has exactly one answer.

- **d3 — Grants bind to node ids, never paths.** A grant names a workarea node id; the path
  (Postgres `ltree`) is derived from the tree at check time. Renaming or re-parenting a node never
  invalidates or rescopes a grant, and a path string is never an authority — anything that
  compares paths is a derived view as of the current permission epoch, reproducible from
  journalled state.

- **d4 — Effective access is a union of attenuated grants, then deny-wins.** Effective access =
  the union of a key's grants, each grant first attenuated by its own deny entries, minus anything
  named by a global per-key deny object. A deny entry lives inside a grant, names a subtree by
  node id, and masks it within that grant only; a global per-key deny is a distinct journalled
  object with deny-wins semantics over any union. Every effective-access computation is
  reproducible from journalled grants, and a permission epoch invalidates caches on any change.
  The formula, its evaluation order and a worked example are declared on
  [architecture/workareas.md](../architecture/workareas.md).

- **d5 — Relations are labelled edges that confer zero access, governed under R2.** `related` is a
  separate edge kind between two workarea nodes with a closed label set (`relates_to`,
  `continues`, `derived_from`, `mirrors`). An edge is recorded structure for lineage and
  navigation, never an input to access computation. Edges follow the propose/ratify split:
  proposing requires `dreamer(propose)` effective access on at least one endpoint and reader
  effective access on both; ratifying requires `dreamer(review)` on **both** endpoints; the
  ratifier is never the proposer — **R2 extends to edges**
  ([ADR-0001](ADR-0001-the-constitution.md) d2).

- **d6 — Re-parenting is a journalled admin act with a single-transaction path rewrite.** The
  move event, the parent-pointer change and the descendant `ltree` rewrite commit in one Postgres
  transaction, so no read observes a tree in which a path and its parent pointer disagree. Because
  of d3 no grant is touched; the act bumps the permission epoch because cached subtree resolutions
  computed against the old paths are now wrong. The cost is a lock proportional to the moved
  subtree, accepted. Re-parenting into a `validated` destination is refused
  ([ADR-0009](ADR-0009-status-ladder.md)).

- **d7 — A workarea records its crew; the record is operated by nobody.** Crew name, charter URI
  and external manager are recorded metadata — OML never contacts the manager, schedules the crew
  or validates the charter beyond storing it. Crew-metadata update is a journalled admin act and
  is permitted on `validated` areas: freeze covers claim content and promotions, not workarea
  metadata. The reviewer-independence axis
  ([architecture/admission.md](../architecture/admission.md)) resolves reviewer crew and manager
  from this journalled metadata at promotion time, which is why its updates must be journalled
  acts rather than free edits.

## Consequences

- The schema realizes d1–d7 as `workareas` (id, tenant, parent_id, derived `ltree` path, status,
  crew jsonb), `workarea_edges` (pending/ratified, proposer, ratifier), `grants` (key, role,
  workarea node id, deny entries, epoch) and `key_denials` (global per-key deny objects) —
  [data/postgres-schema.md](../data/postgres-schema.md). RLS by tenant applies to all of them
  (ADR-0007).
- Every access check in the system — write admission, recall scoping, graph reads, edge acts —
  computes d4's formula and nothing else. There is no second access mechanism to keep consistent,
  and every answer is reproducible from journalled grants at a named permission epoch, which is
  what the evidence export relies on
  ([architecture/audit-and-lineage.md](../architecture/audit-and-lineage.md)).
- d3 makes rename and re-parent cheap for authority and pays for it in d6's path rewrite: the
  derived-path column must be maintained transactionally, and the lock cost of moving a large
  subtree is real. Lake rows keep `workarea_id` as the stable key with path-at-write as history,
  so re-parenting never rewrites the lake ([architecture/storage.md](../architecture/storage.md)).
- d4's per-grant deny masking means a deny does not compose across grants: operators who want an
  absolute prohibition must use the global per-key deny, and the documentation must say so where
  grants are administered ([cli/reference.md](../cli/reference.md)). This is a deliberate trade:
  grants stay self-contained statements of scope at the cost of one more object kind.
- d1 defers the stated end goal: relating what separate tenants' crews learned requires a design
  for claims crossing a tenant boundary — review, attribution, erasure — and is tracked as
  RS-3 in the [roadmap](../product/roadmap.md). Until that row closes, every page that mentions
  the whole-world goal marks it future.

## Rejected alternatives

- **DAG membership (a workarea with multiple parents).** Letting one area sit under two parents
  would let one crew's space appear in two branches without edges. Rejected because access
  computation becomes ambiguous: with two parents, a node's path is not unique, so "the subtree a
  grant covers" and "the subtree a deny masks" stop having one answer, and a deny inherited down
  one path can collide with a grant inherited down the other with no principled resolution order.
  d5's labelled edges give the expressiveness (one area `mirrors` or `continues` another) without
  touching access, which is the actual need the DAG would have served.
- **Path-bound grants.** Binding a grant to a path string is simpler to implement and to read in
  an audit row. Rejected on the rename-rescoping failure: the moment a node is renamed or
  re-parented, a path-bound grant either silently detaches from the subtree it was issued for or
  silently attaches to whatever now occupies the old path — both are access changes nobody
  journalled. Name-keyed state has a second failure: once rows, keys and links are recorded by
  name, a name becomes an identity that can never safely be reused. Node ids make the grant
  follow the object, and the path stays a derived view (d3).
- **One global universe across tenants.** A single shared tree with tenants as top-level branches
  would make the "whole world" goal a present-tense property. Rejected because it contradicts
  tenancy as designed: RLS keys every row by `tenant_id` (ADR-0007), and a tree whose root spans
  tenants is a row no tenant's policy can own — every subtree computation at the root would read
  across boundaries that D6's isolation exists to make uncrossable. The whole-world goal is real
  and stays stated, as federation between per-tenant universes, explicitly future: RS-3 in the
  [roadmap](../product/roadmap.md).
