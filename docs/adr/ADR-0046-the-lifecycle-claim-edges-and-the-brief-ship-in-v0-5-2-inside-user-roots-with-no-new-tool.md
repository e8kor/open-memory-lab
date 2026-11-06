<!--
id: adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool
tier: engineering
status: accepted design · 2026-11-04
answers: how the status ladder and claim edges, built on two branches before the v0.4.0 rewrite, fit user roots (the proposed ADR-0044) and the three-tool API (ADR-0039) for v0.5.2; which surface serves them; what the workarea brief is, who curates it, how large it is and how it reaches an agent; what the maintainer answered on 2026-11-04
-->

# ADR-0046 — The lifecycle, claim edges and the brief ship in v0.5.2 inside user roots, with no new tool

**Status:** accepted design · 2026-11-04 · proposed and accepted on 2026-11-04 with release v0.5.2, its nine questions answered that day · amended by [ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md) (d2: `same_user` compares the person behind each key and marks a claim's approval too)

**Release:** introduced in v0.5.2 ([releases](../changelog.md))

**Amends:** ADR-0008 (d5), ADR-0009 (d1, d3, d5), ADR-0021 (d5), ADR-0029 (d2), ADR-0039 (d2, d9)

## Context

**Two features were built before the API they would ride.** The status ladder of
[ADR-0009](ADR-0009-status-ladder.md) and the claim edges of
[ADR-0008](ADR-0008-workarea-algebra.md) d5 and [ADR-0021](ADR-0021-ai-human-ontologies.md) d5
were built on the branches `feat/transitions` and `feat/claim-edges` as domain functions, use
cases and Postgres folds, with no route, tool or CLI verb, because the v0.4.0 rewrite
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)) was about to move both surfaces. Both are
now replayed onto v0.4.1 for v0.5.2. Their own pages state the readings they took:
[workareas](../architecture/workareas.md#readings-the-build-takes) for the ladder, the
[schema](https://github.com/e8kor/open-memory-lab/tree/main/migrations) and the [changelog](../changelog.md) for claim edges.

**The plan of 2026-11-04 put three things in v0.5.2.** The workarea lifecycle, labelled links
between memories, and a brief for each workarea, after v0.5.0 (user roots, ADR-0044, and the web
pages, ADR-0045, both proposed on their own branches) and v0.5.1 (guardrails, ADR-0037,
proposed) ([releases](../changelog.md)). The maintainer decided on
2026-11-03 that the brief is built from ratified edges, not from a `claim_key` prefix.

**Three records meet here and none of them says how.** ADR-0044 d1 says a user root "starts in
`development` like every new workarea, and climbs the status ladder like any other", d6 leaves
transitions and edges "where ADR-0008 and ADR-0009 put them", and d8 keeps the reviewer
exclusion per key and principal rather than per person. ADR-0039 d8 forbids a fourth served
tool without a record, d9 merges the claim-edge and workarea-edge routes into one `/v2/edges`
family and folds the status read into the workarea read, while `mcp-tools.designed.json` still
holds `oml_relate_propose`. No record designs the brief: the
[use-case call map](../api/mcp-plugin.md) lists UC-BR-01 as "decided, not
designed".

This record states the reconciliation the build follows. It was proposed on 2026-11-04 with nine
open questions, and the maintainer answered all nine that day by taking each recommendation; the
decisions below carry the answers, and [the answers](#the-maintainers-answers-of-2026-11-04) list
them by question. Decided; built on `feat/v0.5.2` except where the [consequences](#consequences)
say otherwise.

## Decision

- **d1 — User roots and every workarea under them climb the ladder to `committed`, never to
  `validated`; `universe` stays `committed`.** A user root is an ordinary node with
  `user_root: true` (ADR-0044 d1), so proposal, ratification and the child-status rule apply to it,
  with one exception: a proposal or ratification moving a user root to `validated` is refused
  `workarea.transition_refused` with reason `user_root`, as `universe` is refused with reason
  `universe`. A frozen user root would leave its user nowhere to create work, since ADR-0044 d3
  grants a user key nothing outside its root; its children climb freely, so a finished subtree
  inside it still freezes. This amends ADR-0009 d1 by one more status rule. The cost: a person's
  whole space can never be presented as frozen evidence; only a subtree of it can.

- **d2 — Inside a user root, the user's own dreamer keys propose and ratify, independence is per
  key and principal, and a ratification by another key of the same user is marked `same_user`.**
  ADR-0009 d3 names "an admin or a dreamer with effective access on the node"; under ADR-0044 d3 a
  user's dreamer keys hold their grants inside their root, so they propose and ratify there. The
  ratifier must be another key with another principal (`review.ratifier_is_proposer`), as
  ADR-0044 d8 reads review. Where proposer and ratifier keys name the same user, the
  `workarea.status_transition` and an edge's `review.ratify` payload carry `same_user: true`;
  otherwise `false`. A solo user can move their own work, and an auditor reading the export sees
  what the step is worth. This amends ADR-0009 d3's "second, non-proposing" by stating that the
  identity it means is the key and principal, and adds the mark.

- **d3 — The lifecycle is served on HTTP and the CLI, and to agents through the workarea read
  only.** `POST /v2/workareas/{workarea}/transitions` proposes and
  `POST /v2/workareas/{workarea}/transitions/{transition_id}/ratify` ratifies
  ([HTTP API](../api/http-api.md#workareas)); `oml workareas status propose|ratify|pending` drive
  them ([CLI](../cli/reference.md#oml-workareas-tree-edges-status)). The review policy in force and
  the live proposals are the fields `policy` and `pending_transitions` of
  `GET /v2/workareas/{workarea}`, which `oml_workarea` already rides (ADR-0039 d9). No MCP tool
  proposes or ratifies a transition: a transition is a governed act, and an agent that wants one
  asks its user, as ADR-0044 d6 has it for workarea creation.

- **d4 — Claim edges ride `/v2/edges` with `kind: claim`; in v0.5.2 people and services propose
  them, never an agent's tool.** The edge family is `GET /v2/edges`, `POST /v2/edges` and
  `POST /v2/edges/{edge_id}/ratify` (ADR-0039 d9), and `oml edges list|propose|ratify` on the CLI.
  `oml_relate_propose` leaves `mcp-tools.designed.json`: this amends ADR-0039 d2's designed tools.
  The agent's way to propose an edge is planned as a field of `oml_write`, `links: [{to, label}]`,
  proposing edges from the claim it writes in the same request; it is designed by a later record
  and not built in v0.5.2.

- **d5 — An edge is served only to a key that reads both of its endpoints, and a user key never
  proposes an edge with an endpoint in `universe`.** The edge read selects an edge only when both
  endpoints are readable to the key: promoted, or the key's own on a node where its reader access
  holds. A reader of `universe` alone therefore never sees an edge into a user root, and ADR-0044
  d9's serving rule holds for edges as for `derives_from`. A user key's claim-edge proposal with
  either endpoint in `universe` is refused `access.grant_refused` with `needed_role: dreamer`
  before anything is journalled, failing closed on the maintainer's rule that users neither write
  nor propose to `universe`. A brief edge from a user key is refused the same way when its claim
  or its workarea is `universe`.

- **d6 — A claim-edge label is any non-empty NFC string; `supports`, `contradicts`,
  `relates_to` and `continues` are the recommended labels, and the `oml:` labels stay reserved.**
  The [HTTP API](../api/http-api.md#edges) documents the four as recommended, not as a closed
  set: a tenant may use its own. `oml:` labels join terms only (ADR-0021 d5), and `oml:about`,
  `oml:replaced_by` and `oml:consider` are refused as standalone proposals and at ratification.
  `brief` is not a claim-edge label: it names the brief edge kind (d7).

- **d7 — A workarea's brief is its ratified brief edges: a new edge kind, `brief`, from one claim
  to one workarea, on `/v2/edges`.** A brief edge `{kind: brief, from_artifact_id, to_workarea_id}`
  says "this claim is part of this workarea's brief". Its label is `brief` and fixed. It is
  stored apart from claim edges, in `brief_edges`, and widens no lineage closure: membership in a
  brief says where a claim is read first, not what it derives from. An edge of either kind confers
  zero access. This amends ADR-0008 d5 by a third edge kind beside claim edges and workarea edges.

- **d8 — Any dreamer on the workarea proposes a brief edge, a second identity ratifies it, and the
  brief served is cut by a token budget.** The proposer holds dreamer effective access on the
  workarea and reads the claim, which must be promoted; the ratifier holds dreamer effective
  access on the workarea, is not a `held_by: service` key, and is never the proposer's key or
  principal (`review.ratifier_is_proposer`); d2's `same_user` mark applies. The brief served is
  the ratified edges whose claim is still the served tip and readable by the key, in ratification
  order, then by `artifact_id`, cut by the budget token count of
  [retrieval](../architecture/retrieval.md) (counter `oml-tc/1`) under recall's prefix rule: the
  first member that does not fit ends the brief. The budget is the server setting
  `brief.max_tokens` (`OML_BRIEF__MAX_TOKENS`), default 400; the number is unmeasured, tracked as
  #145. A superseded or retired claim drops out of the brief by itself, since only served tips
  are members.

- **d9 — The brief reaches an agent through the workarea read and the session-start hook.** The
  workarea read, and so `oml_workarea`, serves `brief` beside `policy` and `pending_transitions`:
  `{items, budget}`, each item the claim item of ADR-0025 d12 with its text. The session-start
  hook (ADR-0029) reads its key's default workarea and prints that brief before the recent work and
  the recall, under a heading of its own, and nothing when the brief is empty or the server cannot
  be reached. This amends ADR-0029 d2 by one more section. No tool is added (ADR-0039 d8).

- **d10 — A reviewer may reject a held row in a `validated` area; approval stays refused.** A
  rejection promotes nothing into the frozen area, so it is not a mutation d5 has to admit, and it
  keeps the review queue drainable after a freeze. Approval is refused `workarea.frozen`, as
  built. This amends ADR-0009 d5's closed list by naming reject as outside it.

## Consequences

- **Migrations.** The replayed migrations are `0017_workarea_transitions.sql` and
  `0018_claim_edges.sql`, and the brief edges are `0019_brief_edges.sql`. The release order
  numbers the lanes: 0014 user roots and 0015 the web pages (v0.5.0), 0016 guardrails (v0.5.1),
  then 0017 onward for v0.5.2.
  sqlx applies an unapplied lower version, so the merge order of the lanes does not matter to a
  fresh database.
- **Vocabulary domain modules ride along.** The claim-edge code needs `oml-domain`'s `vocab`,
  `vocab_edges` and the narrowed dreamer grant (`LabelledGrant`), built for the parked ADR-0027;
  nothing else of the vocabulary work is replayed. No route serves them.
- **Over ADR-0044.** d1's `user_root` refusal reads the workarea row's `user_root`, d2's
  `same_user` compares the proposing key's `user`, read from its key row, with the ratifier's
  (since ADR-0055 d7, the person behind each key, a session key's `session_user` included),
  and d5's universe refusal reads the key's `user`; all three are built over v0.5.0's user roots.
- **The web pages show none of this in v0.5.2.** Neither ADR-0045 nor this record puts the
  lifecycle, edges or the brief on the user page or the admin page; a read-only view of a
  workarea's status, its waiting proposals and its brief on the user page is planned, for a later
  record, and until then the CLI and the API serve them.
- **Limits.** The brief edges of one workarea are read whole and paged in the use case; the count
  per workarea is unbounded, tracked with the budget as #145. A brief member must be promoted, so
  a brief starts empty in a workarea with nothing reviewed.
- **New failure modes and their doctor lines.** `oml doctor` names a default workarea whose brief
  is empty and one whose pending transitions wait with no second dreamer able to ratify.
- **At acceptance** ADR-0008, ADR-0009, ADR-0021, ADR-0029 and ADR-0039 gained
  `· amended by ADR-0046` on their Status lines.

## The maintainer's answers of 2026-11-04

| Question | Answer | Where |
|---|---|---|
| 1. May a user root reach `validated`? | No: `committed` at most | d1 |
| 2. Is a ratification by another key of the same user independent enough? | Allowed, marked `same_user` | d2 |
| 3. May a user key propose an edge with an endpoint in `universe`? | No, refused | d5 |
| 4. How does an agent propose a claim edge? | Not in v0.5.2; an `oml_write` `links` field later | d4 |
| 5. Should claim-edge labels be a closed set? | Open, with four recommended labels | d6 |
| 6. What relation makes a memory part of a brief? | A claim-to-workarea edge kind, `brief` | d7 |
| 7. Who curates a brief, and how large is it? | Any dreamer proposes, another ratifies; a token budget | d8 |
| 8. How does the brief reach an agent? | The session-start hook and the workarea read | d9 |
| 9. May a reviewer reject a held row in a `validated` area? | Yes; approve stays refused | d10 |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Serve the transitions and claim edges as MCP tools** (`oml_transition`, `oml_relate_propose`) | ADR-0039 d8 requires a record showing that no parameter, resource or hook carries a capability; the workarea read already carries the status, and a transition is a governed act an agent should ask a person for. |
| **Keep the built `/v2/claim-edges` routes** | ADR-0039 d9 merged them into `/v2/edges` with `kind`; a second family would be one more surface to remove before #150 closes. |
| **A user root that reaches `validated`** (question 1 a) | It freezes its user out of creating any work; the child-status rule already freezes a finished subtree inside it. |
| **A user root that never leaves `development`** (question 1 c) | It would keep ADR-0001 d5's development exception open over a person's whole space for good. |
| **Refuse a ratification by another key of the same user** (question 2 b) | A solo user could never move their own work; the mark tells the auditor the same thing without stopping the work. |
| **Let a user key link into `universe`** (question 3 a) | The maintainer's rule is that users neither write nor propose to `universe`; failing closed can be reopened when a user needs to cite a curated claim. |
| **A fourth tool for edges** (question 4 c) | ADR-0039 d8; the `links` field covers the case agents meet, linking the claim they are writing. |
| **A closed label set** (question 5 b) | Tenants name their own relations; documenting four keeps most edges comparable without refusing the rest. |
| **A reserved claim-edge label to an anchor claim** (question 6 a) | It needs an anchor claim per workarea and widens lineage closures that have nothing to do with derivation. |
| **A `brief` kind on the claim itself** (question 6 c) | It contradicts the maintainer's "brief by ratified edges", and a claim could then sit in one brief only. |
| **No cap, or a count cap** (question 7 a) | Every session pays for the brief; a token budget bounds that cost in the unit recall already uses. |
| **Admins curate briefs** (question 7 c) | An admin key holds no grant and reads no claim content (ADR-0010 d6). |
| **The brief through the hook alone, or the read alone** (question 8 a, b) | The hook is the "on entering an area" moment; the read is the same data for an agent that changes workarea mid-session. |
| **Refuse reject in a `validated` area** (question 9 a) | A held row left by a freeze would stay held for good, and a rejection promotes nothing. |
| **Keep the replayed migration numbers 0010 and 0011** | They sit below main's 0012 and 0013 and the earlier lanes' 0014 to 0016, so a database already migrated would apply them out of order. |
