<!--
id: adr/ADR-0046-the-lifecycle-and-claim-edges-ship-in-v0-5-2-inside-user-roots-with-no-new-tool-and-the-brief-waits-on-its-own-record
tier: engineering
status: proposed
answers: how the status ladder and claim edges, built on two branches before the v0.4.0 rewrite, fit user roots (the proposed ADR-0044) and the three-tool API (ADR-0039) for v0.5.2; which surface serves them; what the build reads where the design is silent; why the workarea brief is not built with them; which questions the maintainer still has to answer
-->

# ADR-0046 — The lifecycle and claim edges ship in v0.5.2 inside user roots, on HTTP and the CLI with no new tool, and the brief waits on its own record

**Status:** proposed · 2026-11-04

**Release:** planned for v0.5.2 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0008 (d5), ADR-0009 (d3), ADR-0021 (d5), ADR-0039 (d2)

## Context

**Two features were built before the API they would ride.** The status ladder of
[ADR-0009](ADR-0009-status-ladder.md) and the claim edges of
[ADR-0008](ADR-0008-workarea-algebra.md) d5 and [ADR-0021](ADR-0021-ai-human-ontologies.md) d5
were built on the branches `feat/transitions` and `feat/claim-edges` as domain functions, use
cases and Postgres folds, with no route, tool or CLI verb, because the v0.4.0 rewrite
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)) was about to move both surfaces. Both are
now replayed onto v0.4.1 for v0.5.2. Their own pages state the readings they took:
[workareas](../architecture/workareas.md#readings-the-build-takes) for the ladder, the
[schema](../data/postgres-schema.md) and the [changelog](../changelog.md) for claim edges.

**The plan of 2026-11-04 put three things in v0.5.2.** The workarea lifecycle, labelled links
between memories, and a brief for each workarea, after v0.5.0 (user roots, ADR-0044, and the web
pages, ADR-0045, both proposed on their own branches) and v0.5.1 (guardrails, ADR-0037,
proposed) ([releases](../product/releases.md#planned-releases)). The maintainer decided on
2026-11-03 that the brief is built from ratified edges, not from a `claim_key` prefix.

**Three records meet here and none of them says how.** ADR-0044 d1 says a user root "starts in
`development` like every new workarea, and climbs the status ladder like any other", d6 leaves
transitions and edges "where ADR-0008 and ADR-0009 put them", and d8 keeps the reviewer
exclusion per key and principal rather than per person. ADR-0039 d8 forbids a fourth served
tool without a record, d9 merges the claim-edge and workarea-edge routes into one `/v2/edges`
family and folds the status read into the workarea read, while `mcp-tools.designed.json` still
holds `oml_relate_propose`. No record designs the brief: the
[use-case call map](../engineering/use-case-call-map.md) lists UC-BR-01 as "decided, not
designed".

This record states the reconciliation the build follows and names every point it leaves to the
maintainer. Decided, not built: the domain, use cases and store of both features are built on
`feat/v0.5.2`; no route, tool or CLI verb serves them yet.

## Decision

- **d1 — User roots and every workarea under them climb the ladder; `universe` stays
  `committed`.** ADR-0044 d1 already says so, and the ladder needs no new rule for it: a user
  root is an ordinary node with `user_root: true`, so proposal, ratification, the child-status
  rule (ADR-0009 d4) and the freeze (d5) apply to it unchanged. `universe` refuses every
  transition, as built (`workarea.transition_refused`, reason `universe`), and ADR-0044 d7's
  curated claims change nothing there. The cost is the one ADR-0009 d4 already carries, sharper
  inside a user root: once a user root is `validated`, its user can create no workarea anywhere,
  since ADR-0044 d6 lets a user key create only inside its own root. Whether a user root may
  reach `validated` at all is left open (question 1).

- **d2 — Inside a user root, the user's own dreamer keys propose and ratify, and independence is
  per key and principal.** ADR-0009 d3 names "an admin or a dreamer with effective access on the
  node"; under ADR-0044 d3 a user's dreamer keys hold their grants inside their root, so they
  propose transitions and claim edges there, and a tenant dreamer key reaches a user root only
  through a grant an admin journals. The ratifier must be another key with another principal, as
  built (`review.ratifier_is_proposer`), which is ADR-0044 d8's per-credential reading applied to
  transitions and edges: a person holding two dreamer keys under two principals can propose with
  one and ratify with the other. This amends ADR-0009 d3's "second, non-proposing" by stating that
  the identity it means is the key and principal, not the person. Widening it to the user is
  left open (question 2).

- **d3 — The lifecycle is served on HTTP and the CLI, and to agents through the workarea read
  only.** Proposing and ratifying ride the designed routes `POST /v2/workareas/{workarea}/transitions`
  and `…/transitions/{transition_id}/ratify` ([HTTP API](../api/http-api.md#workareas)) and the
  designed verbs `oml workareas status propose|ratify|pending`
  ([CLI](../cli/reference.md#oml-workareas-tree-edges-status)). The rung, the review policy in
  force and the live proposals become the fields `status`, `policy` and `pending_transitions` of
  `GET /v2/workareas/{workarea}`, which `oml_workarea` already rides, as ADR-0039 d9 designed
  them; the built status read is that read's source, not a route of its own. No MCP tool
  proposes or ratifies a transition: a transition is a governed act, and an agent that wants one
  asks its user, as ADR-0044 d6 has it for workarea creation.

- **d4 — Claim edges ride `/v2/edges` with `kind: claim`, and no tool proposes one in v0.5.2.**
  The build's idempotency digests name `/v2/edges` and `/v2/edges/{edge_id}/ratify` and carry
  `kind: claim` in the body, so the request the routes will take is the request the use cases
  already digest (ADR-0039 d9). `oml_relate_propose` leaves `mcp-tools.designed.json`'s plan for
  v0.5.2: ADR-0039 d8 needs a record saying why no parameter, resource or hook carries it, and
  none exists. This amends ADR-0039 d2's list of designed tools. An agent's edge proposal is
  left open (question 4); until it is answered, edges are proposed and ratified by people and
  services through HTTP and the CLI.

- **d5 — An edge is served only to a key that reads both of its endpoints, so a claim in
  `universe` never shows an edge into a user root.** The built read selects an edge only when both
  endpoints are readable to the key: promoted, or the key's own on a node where its reader access
  holds. A reader of `universe` alone
  therefore never sees an edge from a curated claim to a claim in alice's root, and ADR-0044 d9's
  rule that a universe claim is served with no lineage below `universe` holds for edges as it
  does for `derives_from`. The widening of the stored lineage closure that an edge proposal
  performs (ADR-0001 d2) is never served; it only excludes reviewers. Whether a user key may
  propose an edge with an endpoint in `universe` at all is left open (question 3).

- **d6 — A claim-edge label is any non-empty NFC string, with the `oml:` labels reserved for
  term relations.** This is the built reading of the open detail
  [http-api](../api/http-api.md#edges) states, and it narrows ADR-0021 d5 nowhere: `oml:` labels
  join terms only, and `oml:about`, `oml:replaced_by` and `oml:consider` are refused as
  standalone proposals and at ratification, as built. "Supports" and
  "contradicts" on the [README](https://github.com/e8kor/open-memory-lab#readme) are examples,
  not a closed set. Closing the set is left open (question 5).

- **d7 — The brief is not built with v0.5.2's first change set; it waits on a record of its
  own.** The maintainer's "brief by ratified edges" fixes the mechanism's family and nothing
  else: no record names the relation that makes a memory part of a workarea's brief (claim edges
  join two claims, workarea edges two workareas, and a brief joins a claim to a workarea), who
  proposes and who ratifies membership, how many memories a brief holds, or how it reaches an
  agent under ADR-0039 d8. Building it would take each of those readings silently. It stays
  planned for v0.5.2, behind questions 6 to 8.

## Consequences

- **Migrations.** The replayed migrations are renumbered `0016_workarea_transitions.sql` and
  `0017_claim_edges.sql`, after main's 0013, leaving 0014 and 0015 to the v0.5.0 lanes
  (`0015_web.sql` is on `feat/web-console-build`). sqlx applies an unapplied lower version, so the
  order in which the three lanes merge does not matter to a fresh database; a database migrated
  by one lane's build and then another's is a development database only.
- **Vocabulary domain modules ride along.** The claim-edge code needs `oml-domain`'s `vocab`,
  `vocab_edges` and the narrowed dreamer grant (`LabelledGrant`), built for the parked ADR-0027;
  those three modules are replayed and nothing else of the vocabulary work is (suggest, the
  scanner, releases and their vectors stay on their branches). No route serves them.
- **What v0.5.2 still has to build.** The two transition routes and the transition fields of the
  workarea read; the three `/v2/edges` operations for `kind: claim`; the CLI verbs of d3 and a
  claim-edge verb; openapi.yaml, the SDKs and the contract checks for each; and the brief once
  its record exists.
- **At acceptance** ADR-0008, ADR-0009, ADR-0021 and ADR-0039 gain `· amended by ADR-0046` on
  their Status lines.

## Open questions for the maintainer

1. **May a user root reach `validated`?** (a) Yes, as ADR-0044 d1 reads: a frozen root leaves its
   user nowhere to create work, since ADR-0044 d3 grants a user key nothing outside its root. (b) A user root climbs to
   `committed` and refuses `validated` with reason `user_root`, as `universe` refuses everything
   past `committed`; its children climb freely. (c) A user root never leaves `development`.
   **Recommended: (b)**, since freezing a person's whole space is never what freezing a piece of
   work means, and the child-status rule still freezes any finished subtree inside it.
2. **Is a transition or edge ratified by another key of the same user independent enough?**
   (a) Yes, per key and principal, as ADR-0044 d8 chose for review and d2 above reads.
   (b) No: inside a user root the ratifier must not be a key of the same user, so a solo user
   cannot move their own work alone. (c) Allowed, but the event is marked `same_user`, as
   ADR-0001 d5 marks `self_signed`. **Recommended: (c)**: a `validated` area is an evidence claim
   an auditor reads, and the mark keeps a solo user working while saying what the claim is worth.
3. **May a user key propose an edge with one endpoint in `universe`?** (a) Yes, under the
   ordinary rule (dreamer on one endpoint, reader on both); d5 keeps it out of every other
   reader's view. (b) No: refused `access.grant_refused`, because the maintainer decided users
   neither write nor propose to `universe`. **Recommended: (b)** for v0.5.2, failing closed; it
   can be reopened when a user needs to cite a curated claim.
4. **How does an agent propose a claim edge?** (a) It does not in v0.5.2: HTTP and the CLI only.
   (b) `oml_write` gains a field, for example `links: [{to, label}]`, proposing edges from the
   claim it writes in the same request. (c) A fourth tool, `oml_relate_propose`, with the record
   ADR-0039 d8 requires. **Recommended: (a) now, (b) next**: (b) keeps the three-tool catalog and
   covers the case agents meet, linking the claim they are writing.
5. **Should claim-edge labels be a closed set?** (a) Open, as built (d6). (b) Closed:
   `supports`, `contradicts`, `relates_to`, `continues`, plus the `oml:` term relations.
   (c) Open, with that list documented as the recommended labels and the brief's label (question
   6) reserved. **Recommended: (c)**.
6. **What relation makes a memory part of a workarea's brief?** (a) A reserved claim-edge label,
   `oml:brief_of`, from each member to one anchor claim of a new kind `brief` in the workarea.
   (b) A new edge kind, claim-to-workarea, with label `brief`, on the same `/v2/edges` family.
   (c) A steering-like kind `brief` on the claim itself, with no edge. **Recommended: (b)**: it
   names the workarea directly, needs no anchor claim, and is ratified like every edge; (c)
   contradicts the "ratified edges" decision.
7. **Who curates a brief, and how large is it?** (a) Any dreamer on the workarea proposes, a
   second ratifies, no limit. (b) The same, with a cap of a few members served in a fixed order.
   (c) Admins only. **Recommended: (b)**, with the cap stated as a budget in tokens, as recall's
   is, rather than a count.
8. **How does the brief reach an agent?** (a) The session-start hook prints it before the recent
   work (ADR-0029). (b) A field of `oml_workarea` and `GET /v2/workareas/{workarea}`. (c) Both.
   **Recommended: (c)**: the hook is the "on entering an area" moment, and the read is the same
   data for an agent that changes workarea mid-session, with no new tool (ADR-0039 d8).
9. **May a reviewer reject a held row in a `validated` area?** Open on the
   [workareas](../architecture/workareas.md) page since the ladder was built; the build refuses
   approve only. (a) Refuse approve and reject alike, and serve no such row from `review/next`.
   (b) Admit reject, since a rejection promotes nothing. **Recommended: (b)**: it keeps the queue
   drainable without promoting anything into a frozen area.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Serve the transitions and claim edges as MCP tools** (`oml_transition`, `oml_relate_propose`) | ADR-0039 d8 requires a record showing that no parameter, resource or hook carries a capability; the workarea read already carries the status, and a transition is a governed act an agent should ask a person for. |
| **Keep the built `/v2/claim-edges` routes** | ADR-0039 d9 merged them into `/v2/edges` with `kind`; a second family would be one more surface to remove before FR-21 closes. |
| **Build the brief now on a reading of each open point** | Seven readings with no record behind them, each a hidden default the maintainer would inherit; the record costs one review and saves a rewrite. |
| **Keep the replayed migration numbers 0010 and 0011** | They sit below main's 0012 and 0013, so a reader of the directory would take them for older than the connections tables, and a database already at 0013 would apply them out of order. |
