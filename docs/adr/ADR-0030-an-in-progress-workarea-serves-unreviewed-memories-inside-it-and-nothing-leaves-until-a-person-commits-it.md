<!--
id: adr/ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it
tier: engineering
status: accepted design · 2026-11-05
answers: what an in-progress workarea is and who may mark it (a person's key, never an agent); the one boundary rule every scoped read applies; which keys read other authors' unreviewed memories inside, labelled how; how R3 is relaxed there and only there, journalled; why ranking still reads no verdict and what telemetry records; what the commit is and why it needs zero unreviewed; how a curator agent narrows the unreviewed work inside and never widens it; how anchors, `oml sync` and draft workareas interact; how an in-progress workarea is abandoned or retired with nothing deleted; the surfaces, codes, migration, doctor lines and laws
-->

# ADR-0030 — An in-progress workarea serves unreviewed memories inside it, a curator agent may only narrow them, and nothing leaves until a person commits it

**Status:** accepted design · 2026-11-05 · revision 2, rebuilt on the v0.5.8 API; revision 1
([#12](https://github.com/e8kor/open-memory-lab/pull/12), 2026-10-31, proposed) is preserved at tag
`archive/feat/in-progress-workareas`

**Release:** planned for v0.6.0 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0001 (d3, d5: a second bounded relaxation of R3, inside an opted-in scope only),
ADR-0009 (d3: a transition on a node inside an in-progress subtree waits for its commit),
ADR-0025 (d3, d4: the readability test and `include_unreviewed`'s reach inside),
ADR-0039 (d4: `include_unreviewed`'s default inside; the claim item gains `unreviewed`),
ADR-0044 (d10: the served node set beside `universe`'s claims), ADR-0045 (d1, d9: the user
page's action `workarea_mode`), ADR-0055 (d1: the commit is the
act bulk review prepares), ADR-0057 (d14: a draft workarea may be in progress)

## Context

A crew of agents on one task needs, within the minute, what its other agents wrote. Under R3 a
held write is read by its author alone until a non-author approves it
([ADR-0001](ADR-0001-the-constitution.md) d3), so every crew waits on review. Competing memory
services serve an agent's write to every reader at once and review nothing. The operator decided
on 2026-11-05 to take that immediacy for agent writes where a person opts in, and to keep the
gate: nothing written inside reaches anyone outside until a person has reviewed all of it.

Revision 1 of this record (#12, with its domain core in #13) was written against the v0.3.0
surface: an admin mark, a commit that was the `development` → `committed` transition, a grant
attribute `unreviewed: allow | block`, and its own recall flag. Since then
[ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) rewrote the API and recorded that this
record owes a rebase onto `include_unreviewed` (its d4 and Consequences);
[ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
and [ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)
put each person's work under a user root, where the person's own keys propose and ratify and a
same-person step is marked `same_user`;
[ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
made review of a listed set one act of a person's key; [ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
added anchors, `expected_version` and `oml sync` with a draft workarea per pull request; and
[ADR-0054](ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md)
measures every recall. This revision rebuilds the decision on that surface.

**The operator's decisions** (2026-11-05), not reopened here: a person's key marks a workarea in
progress, never an agent's (R1); unreviewed memories of other authors are visible only inside
that workarea and its children, labelled `unreviewed`, never in a read from outside or from a
parent; nothing leaves until a person's commit, which needs zero unreviewed and is prepared by
bulk review; R3 is relaxed only inside the opted-in scope, explicitly and journalled; ranking
never reads verdicts. And, the same day: a **curator** agent cleans up inside an in-progress
scope and may only narrow — retire, reject, flag, propose — never promote out; widening needs a
person.

Questions revision 1 left open are settled below at their recommended answers, each marked
*recommended answer*. Decided; built on `feat/in-progress-r2` for v0.6.0.

## Decision

- **d1 — A workarea's `mode` is `standard` or `in_progress`; only a person's key sets it, and
  setting it is journalled.** `workareas.mode` defaults to `standard`. `POST
  /v2/workareas/{workarea}/mode` with `{mode}` sets it and journals `workarea.mode_set`
  `{workarea_id, mode, from}` under the calling key; `oml workareas mode <workarea>
  in_progress|standard` and the admin and user pages' toggle ride it. The caller must hold a
  **dreamer key `held_by: human`** with dreamer effective access on the node — the predicate
  ADR-0055 d1 gives bulk review, for the same reason: the mark changes who reads what other keys
  wrote, so it is a review-class act and a person's attestation. Every other key is refused
  before any lookup with `access.verb_ceiling`, `details.reason: mode_is_human_only`: a writer or
  reader key (R1), an admin key (an admin is never a reviewer, D9), a service key and any dreamer
  key held by an agent, a curator's included. No MCP tool, argument or resource sets it.
  Inside a user root the user's own reviewer key sets it (ADR-0046 d2). *Recommended answer:* the
  mark may be set at any time on an existing workarea, as revision 1's d1 also chose, since the
  workareas people would mark already hold rows.

    Setting `in_progress` is refused with `workarea.in_progress_refused` and a `details.reason`
    when the node is not `development` (`status`), is `universe` (`universe`), or has an ancestor
    or a descendant already in progress (`nested`): subtrees never nest, so every node has at
    most one **in-progress root**, its nearest ancestor-or-self in progress. A node is *inside*
    while it has one. Setting `in_progress` on a node already in progress, or `standard` on one
    already standard, is a no-op that journals nothing.

- **d2 — One boundary rule decides every scoped read: a row on node D is served to a read scoped
  at N only if D has no in-progress root, or N has the same one.** A **scoped read** — recall,
  the scoped listing, the claim-edge and brief reads, the events tail — serves the *served node
  set*: the nodes of subtree(N) where the key reads, less every node whose root differs from N's.
  A read scoped at a parent of an in-progress workarea, or anywhere else outside it, therefore
  leaves the whole subtree out, promoted rows included, until the commit (d6): nothing leaves.
  A **read by id** (get, the lineage of one row, an attachment) is at the row's own node and
  passes the boundary, so a key with reader access on an ancestor can get an inside claim by id;
  d3 decides whether a held one is served. A write's `derives_from` target is read at the
  write's node, so a target inside a subtree the write is not inside answers `access.not_found`
  (d7).
  `oml-domain` holds the rule as one pure function beside `effective_roles`, over the marks the
  tree read already returns; nothing is stamped on a memory row, so a mark, a commit and a later
  re-mark write no row. The rule is applied to the node list the store statement receives, so
  each retriever filters before its own `LIMIT` and before any statistic it scores with, never
  after (ADR-0049 d2): a read outside returns the same rows, order and scores whatever is
  written, approved or curated inside (property `P-R3-BOUNDARY`).

- **d3 — Inside, a key with reader access reads other authors' held memories, labelled
  `unreviewed`; this is the constitution's second bounded relaxation, of R3's first sentence,
  and nowhere else.** A key reads another key's held version on node D exactly when D has an
  in-progress root, reader is in the key's effective access on D, and the read is by id or
  scoped inside that root (d2). Served held versions are of kinds `memory` and `derived_fact`;
  a held `steering` or `guardrail` version, and a held retirement version, stay author-only,
  since steering and guardrails are served first and never cut and must be reviewed text
  (ADR-0025 d8; ADR-0037). Rejected, demoted and erased versions are served as everywhere else
  (not at all), so what review or a curator turned down never returns to an agent's context.
  Everywhere outside an in-progress subtree, and in every `committed` and `validated` workarea,
  R3 holds without exception: a held row is its author's and its reviewer's alone. R2 holds
  inside without exception: every held version is still reviewed by a non-author, never
  `self_signed`. The relaxation is explicit — it exists only while a journalled
  `workarea.mode_set` puts the node in progress — and every read that serves an unreviewed row
  records it in the read log as any served id is. ADR-0001 d5 gains this as its second bounded
  exception, by reference to this decision; no other page widens it.

- **d4 — Every claim item carries `unreviewed`, and inside an in-progress scope
  `include_unreviewed` defaults to `true`.** `unreviewed` is `true` exactly when the served
  version is `held`, the caller's own held rows included, on every surface that serves a claim
  item (HTTP, MCP `oml_read`, the SDKs, the CLI, the pages). Recall's `include_unreviewed`
  (ADR-0039 d4) becomes optional: absent, it is `true` when the scope node is inside an
  in-progress subtree and `false` elsewhere; `false` narrows to promoted rows; `true` outside
  still adds only the caller's own held rows (ADR-0025 d4). Held hits join the one total order
  of ADR-0025 d8 and count against `k` and `max_tokens`, as every hit does. The session-start
  hook already asks `include_unreviewed: true` and marks each held hit by its state.
  `expected_version` (ADR-0057 d19) is unchanged: another author's held version never changes
  the answer, inside or out, so a version conflict never discloses a held row a key could not
  otherwise read and never makes a writer chase an unreviewed rival.

- **d5 — Ranking still reads no verdict, no mode and no curator act; telemetry labels what was
  served.** A held row inside is scored by the same functions as a promoted one (ADR-0049 d2;
  ADR-0052), so approval never moves it, and the usage term reads `usage_state` alone (ADR-0054
  d12). The metrics gain three families, each with closed labels: `oml_recall_unreviewed_hits_total`
  (hits served with `unreviewed: true`), `oml_workarea_mode_changes_total{mode}` and
  `oml_curation_acts_total{act}`, where `act` is `reject`, `retire`, `flag` or `write`
  (d9). `recall_events` keeps its columns; its `hit_ids` already name held versions served. No
  metric carries a workarea, key or claim label (ADR-0054 d1's series budget).

- **d6 — The commit is a person's act that sets `standard`, and it is refused while anything in
  the subtree is unreviewed.** Setting `standard` on an in-progress root is its **commit**:
  refused with `workarea.held_remaining`, `details.workarea` naming the root as the request
  spelled it and carrying no count, while any version anywhere in its subtree is `held`,
  dead-lettered ones included; rejected, demoted, retired and erased versions do not block. The
  test runs in the committing transaction under the chain lock, as every capture takes it, so no
  held version lands between the test and the commit; one `EXISTS` over the subtree's nodes
  through `memories_workarea (tenant_id, workarea_id, state)`, unmeasured beyond the build's test
  (FR-25). The person empties the subtree with bulk review (ADR-0055): `oml review list
  --workarea <root>` then `oml review approve --ids -` or `reject --ids - --reason …`, each item
  judged and signed as one verdict, R2 per item, `same_user` marked where it applies. From the
  commit no node of the former subtree has a root, so its promoted rows reach every scope.
  *Recommended answer:* the commit is one act of one person, not ADR-0009's two-identity
  transition, because it attests nothing new — every row it releases was already approved by a
  non-author under R2 — and because a workarea stays `development` and may be marked again for
  the next iteration. A status transition proposed or ratified on any node inside is refused
  `workarea.in_progress_refused` with reason `commit_first`, so the ladder never moves under an
  open subtree.

- **d7 — Nothing crosses the boundary before the commit.** A claim edge (any kind, including
  `brief`) whose two endpoints have different in-progress roots, or one root and none, is refused
  `workarea.in_progress_refused` with reason `edge_crosses`, answered after both endpoints pass
  readability, so a key that cannot read an endpoint gets `access.not_found` and learns nothing.
  A `derives_from` target inside, named by a write outside, is unreadable at the write's node
  and answers `access.not_found` as any unreadable target does; a write inside may derive from
  one outside. A supersession lands in its target's workarea, so it never crosses. Reassignment
  and re-parent are not built; their records take this rule when they are.

- **d8 — A key may carry the `curator` profile: a dreamer key held by an agent, which inside an
  in-progress scope may narrow and never widen.** `POST /v2/keys` and `oml keys issue` take
  `profile: "curator"`, journalled in `key.issued`, fixed for the key's life; the key must be
  `role: dreamer` and `held_by: agent`, with its own key and principal, and anything else is
  `request.validation` naming `/profile`. A curator key's grants may be reader, writer and
  dreamer, the one widening of ADR-0025 d2's admissible set, so that one agent can read, write
  and reject. *Recommended answer:* a profile on the key, revived from ADR-0027's member profiles
  (#10, archived at `archive/feat/ontology-crews`), rather than a sixth role, because R1's verb
  ceiling is per role and a curator's ceiling is a dreamer's narrowed by scope, not a new rung.

- **d9 — A curator's acts, each only inside an in-progress scope, each journalled, attributed
  and reversible.** Every act names rows whose workarea is inside an in-progress subtree; a
  row or a target outside every one is refused `access.verb_ceiling` with reason
  `curator_outside_in_progress`, before anything is written. A curator never acts on a row its
  own key or principal authored or appears in the lineage of (R2 per key and principal):
  `review.author_in_lineage`.

    | Act | Route | What it does | Reversed by |
    |---|---|---|---|
    | reject | `POST /v2/review/verdicts` with `verdict: reject` and a `reason` | a held item becomes `rejected`, journalled `review.reject` under the curator key, bytes kept (R4) | a person writing the claim again, which may name the rejected version in `derives_from` |
    | retire | `POST /v1/memories/{id}/supersede` with `retirement: true` | a held retirement proposal for a promoted claim inside, an ordinary held write | a person rejecting the proposal; after approval, a person's new version of the claim |
    | flag | `POST /v2/edges` `{kind: claim, label: contradicts}` | a pending `contradicts` edge between two claims of the same subtree, journalled `claim_edge.proposed` | a person rejecting or never ratifying it |
    | write | `POST /v1/memories` | a consolidated or summary claim, an ordinary held write, `derives_from` its sources | review, like any write |

    A curator's `approve` is refused `access.verb_ceiling` with reason `curator_narrow_only`,
    inside or out: a curator **never promotes**, so nothing it touches leaves the scope except
    through a person's bulk approval of the curated residue and the commit (d6). Every other
    review-class verb is refused it with the same reason: `review next` (a curator chooses its
    targets from what it reads), bulk review, the mode, transitions, edge and brief
    ratification. *Recommended answer:* a retirement is a held proposal a person approves, not an
    immediate hide, because a retirement version is a write and R2 forbids its author signing it;
    a duplicate that is still held is narrowed at once by rejecting it, which is the common case
    inside. The exception the operator named, mechanical facts a curator re-derives
    deterministically (ADR-0047 d9), waits for that record's build and is not built here.
    Ranking reads no curator act (d5).

- **d10 — A reference curator peer runs outside the server, over the SDK, with its own key,
  cursor and budget.** Not built here. It holds one curator key per in-progress scope it serves,
  tails events from its cursor (`GET /v2/events?after_seq=`), reads the scope's held rows with
  the scoped listing, and spends a budget of acts per hour that its operator sets. Its first
  signal is deterministic: the MinHash near-duplicate check of ADR-0050's amendment (lane
  `feat/remember-dedup`), which proposes a rejection of the newer of two near-duplicates with a
  reason naming the older, and a `contradicts` flag only where a model the operator chose says
  so. It never sees a row outside its scope, since its key's acts are refused there.

- **d11 — Anchors, `oml sync` and draft workareas work unchanged inside, and a draft workarea is
  the first case to mark.** An anchor is part of each version (ADR-0057 d2) and is served on an
  unreviewed item as on any other; `stale` and the origin filters apply inside as outside. `oml
  sync`'s diff reads the key's own rows and the promoted tips (ADR-0057 d8), so marking its
  workarea changes no plan. *Recommended answer:* `oml-design-pr-<n>` (ADR-0057 d14) may be
  marked in progress by the operator's reviewer key, so agents working on the pull request read
  its draft decisions at once; at merge or close the operator rejects or approves what is held,
  `--retire-all` proposes the retirements, those are approved in bulk, and the commit then
  releases nothing a reader at `oml-design` would see twice.

- **d12 — Abandonment and retirement delete nothing.** *Recommended answer:* no verb clears the
  mark without the commit's precondition. Work abandoned inside is rejected in bulk by a person
  (ADR-0055 d8) with a reason; its bytes stay (R4), served to no one but their authors. A
  workarea left in progress with nothing held may simply be committed; one left with held rows
  stays in progress, read only inside, for as long as nobody commits it. An abandoned scope stays
  visible: its root is named on every node read inside it (`in_progress_root`), and `oml doctor`
  reports it for a key whose default workarea lies inside.

- **d13 — The surfaces.** `GET /v2/workareas/{workarea}` and the tree read gain `mode` and
  `in_progress_root` (`{id, name}` or null); `POST /v2/workareas/{workarea}/mode` (d1, d6); the
  claim item gains `unreviewed` (d4); `RecallRequest.include_unreviewed` becomes optional (d4);
  `KeyIssue.profile` and the key read's `profile` (d8). `oml_workarea` (ADR-0039 d9) serves the
  two workarea fields, so an agent learns that it works in progress; no MCP tool is added. CLI:
  `oml workareas mode`, `oml keys issue --profile curator`. Pages: the user page's workarea view shows each node's mode,
  and its closed action set gains `workarea_mode`, acting as the person's `<user>-review` session
  key (amending ADR-0045 d1, d9). Codes, each in the errors tables and the `ErrorCode` enum:
  `workarea.in_progress_refused` (409, reasons `status`, `universe`, `nested`, `commit_first`,
  `edge_crosses`), `workarea.held_remaining` (409); `access.verb_ceiling` gains reasons
  `mode_is_human_only`, `curator_narrow_only` and `curator_outside_in_progress`. The SDKs are
  regenerated from the contract.

- **d14 — Migration and doctor.** One forward-only migration adds `workareas.mode` (`text NOT
  NULL DEFAULT 'standard'`, checked) and `api_keys.profile` (nullable, checked); both default to
  today's behaviour, so the migration rewrites no row's meaning and is safe on live. It is
  numbered after main's highest and the open lanes' (`0027_in_progress.sql`, noted for
  renumbering at merge). `oml doctor` gains: whether the key's default workarea lies inside an
  in-progress workarea, and its root; for a person's reviewer key, that it may set the mode and
  how many held items it can review block the commit (one page of `GET /v2/review/held`); and a
  curator key whose default workarea is not in progress, which can do nothing there. Naming
  in-progress roots idle 30 days (d12) waits for a tenant-wide read of the marks and is opened
  as a follow-up.

- **d15 — Laws, each proven red first by breaking its clause.**
    1. Unreviewed rows are visible inside an in-progress scope, to another author's reader key,
       labelled `unreviewed`, and invisible from outside it, from its parent, and to another
       tenant (`P-R3-BOUNDARY`, store and use case).
    2. Only a person's key sets the mode: an agent, service, writer, reader and admin key are
       refused.
    3. The commit is refused while any unreviewed row remains and succeeds at zero, after which
       the promoted rows reach the parent.
    4. No ranking input reads a verdict, the mode or a curator act: a held row scores as it does
       once promoted.
    5. R3 holds everywhere else: outside an in-progress subtree another author's held row is
       never served.
    6. A curator key cannot promote; cannot act outside an in-progress scope; cannot curate its
       own writes; and each act is journalled under its key and reversible as d9 states.

## Consequences

- **What a crew gets**: its agents read each other's work at once, labelled, while review keeps
  its own pace, and a curator agent may thin the backlog without being able to release any of it.
- **A rejection claws nothing back** from the readers inside: what agents read and derived from
  stays read, as the read log records; `unreviewed: true` is the mitigation, and the `oml-memory`
  skill tells agents to weigh such claims accordingly. Undeclared derivation stays undetectable
  (ADR-0001 d6).
- **Marking an existing workarea takes its promoted claims from every read scoped outside it**
  until the commit; that is the "nothing leaves" rule, and the cost of marking a workarea that
  others already read.
- **The commit waits on review**: one held claim no eligible reviewer can judge holds the whole
  subtree. A person may reject it.
- **A lake writer** (proposed ADR-0058, #75) must apply d2 at its source read, so no row inside
  reaches a consumer zone before the commit; it is not built on main, and this record binds it.
- **Revision 1's grant attribute `unreviewed: allow | block` is dropped**: a key that should not
  read unreviewed work is given no reader grant inside, which is the access model's one
  mechanism.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **An admin key, or any dreamer, sets the mode** | The mark changes what other keys read; an admin never reviews (D9), and an agent dreamer would let an agent widen R3, which R1 keeps off agent hands. |
| **The commit is ADR-0009's `development` → `committed` transition** (revision 1, d12) | It needs a second identity for an act that attests nothing new, and it ends iteration: a committed node cannot be marked again. |
| **Promoted rows inside stay visible outside before the commit** | One subtree would release by two rules, and a reader above would see part of a scope's work without the rest. |
| **The boundary stamped on rows** (a root column on memory rows behind row security) | Every mark, commit and re-mark would rewrite rows, and the release would depend on each rewrite having run. |
| **A grant attribute `unreviewed: allow \| block`** (revision 1, d6) | A second access mechanism beside grants; a key that must not read inside is given no grant there. |
| **Held steering or guardrails served inside** | Steering and guardrails are served first and never cut; the least checked text would sit in the most privileged place. |
| **A curator that approves inside, or promotes mechanical facts now** | The operator chose narrow only; a curator promoting would let an agent release work, which R1 forbids. ADR-0047 d9's exception waits for that build. |
| **A curator's retirement takes effect at once** | A retirement version is a write; signing it under its own author breaks R2. A held duplicate is narrowed at once by rejection instead. |
| **A sixth role, `curator`** | R1's ceiling is per role and is a ladder; a curator is a dreamer narrowed by scope, which a profile states without a new rung. |
| **A verb that clears the mark without the commit's test** | It would release whatever was promoted inside while held work remained, skipping the one precondition that makes "nothing leaves" true. |
| **Default `include_unreviewed: false` inside** | Immediacy is the point of opting in; an agent configured once would miss its crew's work silently. |
