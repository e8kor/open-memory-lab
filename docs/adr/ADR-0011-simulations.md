# ADR-0011 — Simulations are a fenced provenance channel with taint propagation

**Status:** accepted design · 2026-10-29

## Context

An admin needs a place to exercise the system with synthetic or hypothetical memories — load
tests, crew rehearsals, what-if corpora — without those rows ever being mistaken for the
tenant's real memory. OML already has one pattern for admitting a rule-bending row honestly:
the `self_signed` mark ([ADR-0001](ADR-0001-the-constitution.md) d5) admits a signature that is
not a review under an indelible label every later reader can discount. Simulations generalize
that pattern from "a signature that is not a review" to "a row that is not a real
memory": the mark travels with the row through every store, and the question "what did this
simulation touch" has a mechanical answer. The containment predicate's placement in each read
path is owned by [simulations](../architecture/simulations.md) and
[retrieval](../architecture/retrieval.md); this record makes the decisions. Everything here is
decided, not built.

## Decision

- **d1 — The fence exists as a row, not a convention.** A `simulations` table — id, tenant,
  state, audience — gives the fence mechanical existence. The audience is a journalled list of
  key ids and grant references; creating a simulation, changing its audience and ending it are
  journalled admin acts. A simulation write that names no simulation row is refused.

- **d2 — Every simulation write is an ordinary journalled event with an indelible mark.** It is
  attributed, signed into the tenant's chain, and carries `provenance: simulation` plus its
  `simulation_id` in the journal event, the Postgres row and the lake row. There is no second,
  lighter write path: the mark is the only difference between a simulated submission and a real
  one at capture time.

- **d3 — Simulation writes pass the full admission pipeline.** Capture, quarantine, scan,
  non-author review, ratification ([ADR-0012](ADR-0012-admission-and-review.md)) — nothing is
  waived. Review comes only from dreamer keys in the simulation's audience; a simulation with no
  dreamer in its audience can promote nothing. Fail closed is the design: an admin cannot use
  the channel to inject promoted rows on their own authority
  ([ADR-0010](ADR-0010-roles-and-keys.md) d6).

- **d4 — Containment at every read path.** Simulation rows never enter `validated` workareas.
  They are served only to keys in the audience: every read path — FTS, vector, fetch, graph —
  applies the predicate (provenance ≠ simulation OR key ∈ audience). Simulation rows stay out of
  the shared FTS and vector indexes and are indexed per simulation, so a leak would require a
  missing predicate *and* a wrong index, not either alone.

- **d5 — Taint propagates through lineage.** A claim whose lineage closure touches a simulation
  row carries that `simulation_id` as sim-derived, is refused promotion into `validated`, is
  demoted or re-reviewed when its simulation ends, and is listed in the simulations section of
  every evidence export. The closure is the declared graph, with the limit
  [ADR-0001](ADR-0001-the-constitution.md) d6 states: undeclared derivation is not detectable.

- **d6 — Nesting is forbidden by construction.** A submission whose `derives_from` includes a
  simulation row MUST carry that row's `simulation_id`, server-enforced. There is no
  simulation-within-a-simulation and no half-in row: one end-act can therefore demote the whole
  closure of one simulation.

- **d7 — No backdating.** Simulated events get real chain timestamps; a simulation that wants
  to model another time declares it in a `simulated_time` field. The journal's clock is never
  lent to fiction.

- **d8 — Ending a simulation is a journalled act, and exports keep the separation.** The
  end-act demotes the simulation's rows and triggers re-review of sim-derived taints (d5).
  Demotion keeps bytes (R4); if the tenant wants the bytes gone, erasure is its own journalled
  crypto-shredding act ([audit and lineage](../architecture/audit-and-lineage.md)). Evidence
  exports exclude simulation rows by default and, when included, list them separately by
  `simulation_id` — a simulation can never pad the real corpus in evidence.

## Consequences

- d2 and d4 are schema and index commitments: `provenance` and `simulation_id` columns in the
  journal payload envelope, `memories`, and every lake table
  ([Postgres schema](../data/postgres-schema.md), [Delta tables](../data/delta-tables.md)), plus
  per-simulation FTS/vector indexes instead of rows in the shared ones
  ([retrieval](../architecture/retrieval.md)). The reconciler can check the containment
  mechanically — a simulation row in a shared index or a `validated` workarea is a finding, not
  a debate.
- The cost of d3 is that simulations are as slow as reality: they consume real review capacity
  from their audience's dreamers. That is accepted — a simulation that skipped review would not
  exercise the system it claims to test, and comparability with production behaviour is the
  channel's purpose.
- d5's re-review obligation means ending a large simulation creates a review backlog sized by
  its taint closure. The end-act's `--dry-run` preview in the [CLI](../cli/reference.md) shows
  that reach before the act, per the preview-every-mass-act rule.
- Lake retraction at simulation end rides the existing retract path — a MERGE applied in strict
  outbox order ([storage](../architecture/storage.md)); simulation end introduces no new lake
  machinery.
- Throughput and the practical size ceiling of a simulation are unmeasured; the
  [roadmap](../product/roadmap.md) FR rows on review throughput and Postgres scale bound this
  channel too.

## Rejected alternatives

- **Simulations as separate deployments.** A scratch OML per experiment contains perfectly but
  kills comparability: different data volumes, different key populations, different review
  latencies — the experiment no longer measures the system it was run to inform, and nothing in
  the production journal records that the experiment happened. The fenced channel costs
  predicate discipline on every read path and buys experiments that run against the real corpus'
  shape under the real pipeline, with evidence in the real journal.
- **Unmarked injection under admin trust.** Letting an admin write "test" rows that look real,
  on the promise to delete them later, falsifies the journal: for the duration, the chain attests
  memories nobody holds. The journal's value is that it never says something that was not so;
  one tolerated fiction ends that. This is the same reasoning that makes admin-is-not-reviewer
  ([ADR-0010](ADR-0010-roles-and-keys.md) d6) hold.
- **Erasure of simulation rows at simulation end.** Automatic erasure violates R4: deletion
  would be a side effect of ending a simulation rather than its own journalled act, and the
  evidence that the simulation happened — what it wrote, who reviewed it, what it tainted —
  would vanish with the bytes. Demotion keeps bytes and keeps the record; erasure remains
  available as a separate, deliberate, journalled crypto-shredding act.
