<!--
id: adr/ADR-0030-in-progress-workareas
tier: engineering
status: proposed
answers: what an in-progress workarea is and how an admin marks one; the one boundary function every read calls, computed at read time; which keys read the held claims inside, marked, and how a reader grant blocks that; how R2 holds inside; what any serving index must guarantee outside; how the whole subtree contributes at a commit that nothing held may precede
-->

# ADR-0030 — An in-progress workarea serves its held claims to its readers at once, marked, and contributes its whole subtree at a commit that nothing held may precede

**Status:** proposed · 2026-10-31

**Amends:** ADR-0001 (d3, d5), ADR-0008 (d4, d6), ADR-0009 (d1, d2, d3), ADR-0012 (d1), ADR-0013 (d1, d4), ADR-0018 (d4), ADR-0019 (d1), ADR-0022 (d2), ADR-0024 (d1), ADR-0025 (d1, d2, d3, d4, d5, d8, d9, d12, d13, d14, d15), ADR-0028 (d1, d2), ADR-0029 (Context, d2)

## Context

A crew of agents on one project needs, within minutes, the memory its other agents wrote. Under
R3 a held write is readable by its author alone until a non-author ratifies it
([ADR-0001](ADR-0001-the-constitution.md) d3), so a crew waits on review, whose throughput is
unmeasured ([FR-2](../product/roadmap.md)). Reopening for one bounded case the *optimistic
review* that [ADR-0012](ADR-0012-admission-and-review.md) rejected amends its d1, under which
ratification was the first moment another key read a claim. ADR-0001 d5 makes a change to its
exception's scope a new decision on that record, which this record proposes.

**The maintainer's decisions**, fixed on 2026-10-31 and not reopened here:

1. An in-progress workarea is a `development` workarea that a journalled admin act opts in.
2. Inside it, every key with read access on the workarea recalls all its memories in any state;
   held rows are mixed in at their usual rank, because review never reorders, marked
   `state: "held"` and `reviewed: false`. The scan still runs at admission.
3. R2 is kept: review is deferred, never self-signed, and a non-author reviews every memory.
4. Contribution is whole-workarea: nothing in the subtree, promoted claims included, reaches a
   read scoped outside it until its commit, which a second identity ratifies and which is
   refused while any memory inside is held. Rejected memories do not block it.
5. Each reader grant carries `unreviewed: allow | block`, default `allow`, applying only inside
   in-progress subtrees: a grant on `universe` sets it for the whole tree, a grant per node for
   that node, and a block wins. `oml keys issue` and `oml keys issue-agent` gain a flag to
   block. A key-level setting is rejected.
6. Transitions stay forward only ([ADR-0009](ADR-0009-status-ladder.md) d3).

Review of an earlier draft, which stamped the boundary on index rows, did not converge, so the
maintainer also fixed the mechanism readings that the decisions marked "maintainer's reading"
take. Proposed, not built.

## Decision

- **d1 — An admin marks a `development` workarea in progress, at creation or later; a node's
  root is its nearest marked ancestor-or-self still in `development`.** `POST /v2/workareas`
  with `in_progress: true` sets the mark, journalled in `workarea.create`, and so does
  `POST /v2/workareas/{workarea_id}/in-progress`, journalled as `workarea.in_progress_marked`
  (d11). Both keep the admin key role's ceiling (R1), and no verb clears the mark. A node is
  **inside** its root's subtree while it has a root, so the root's move to `committed`, the
  commit (d12), ends the subtree. The mark is not a status.

    The later act is the maintainer's reading: the maintainer's crew workarea `crew/oml` already
    exists and holds rows. Either act is refused with `workarea.in_progress_refused` unless every
    node of the subtree is `development`, none is a vocabulary workarea, and neither the node nor
    an ancestor or descendant has a root; `in_progress: true` with `prefix` is
    `request.validation`. So every node inside stays `development` until the commit, and
    `committed` and `validated` workareas keep R3 without exception. Marking an existing workarea
    applies d4 and d5 at once to the rows already in it, by the one journalled act and no row
    write.

- **d2 — Until the commit nothing crosses a subtree's boundary, and only its root moves up the
  ladder.** Each case is `workarea.in_progress_refused`:

    - a status transition proposed or ratified on a node whose root is another node;
    - a reassignment whose source and destination have different roots;
    - a re-parent of a node whose subtree holds no root that changes its root, and a re-parent of
      a node whose subtree holds a root to a destination that has one: subtrees never nest;
    - a child created under a node that has a root, with `in_progress: true` or a `prefix`;
    - a claim-edge proposal or ratification whose endpoints have different roots, answered after
      both endpoints pass readability, each read at its own node (d4), since neither act resolves
      a workarea.

    A `derives_from` target inside, named by a submission outside, needs no rule (maintainer's
    reading): a submission reads its ancestors at its resolved workarea, so d4 hides the target and
    the answer is `access.not_found`, as for any claim the key cannot read. A claim inside may
    derive from one outside. This amends ADR-0008 d6 (the re-parent gains the refusal) and
    ADR-0009 d3 (a transition was proposable on any node).

- **d3 — The constitution gains a second bounded relaxation, of R3's first sentence, inside an
  in-progress subtree only, and R2 holds there without exception.** "Nothing is readable by
  others until it is ratified" holds everywhere outside; inside, the keys d5 admits read held
  claims, marked, and nothing inside reaches a read scoped outside until the commit (d4). "Review
  never reorders results" holds inside too (d7, d8).

    R2 is kept (maintainer's reading): a promotion, by `lifecycle.promote` or
    `lifecycle.supersede`, of a version inside counts at least one non-author verdict, applies
    `k` = max(policy `k`, 1) whatever the
    [review policy](../architecture/admission.md#review-policy-objects) says, and never sets
    `self_signed` whatever tenant policy says. The server checks it where the `self_signed` mark
    is made, beside the R2 lineage check; property **`P-R2-INPROGRESS`** states it. This amends
    ADR-0001 d3 and d5, and ADR-0019 d1: `oml-domain` encodes the second relaxation and its floor.

- **d4 — One boundary function decides every read at read time, from the workarea tree and the
  marks.** This is the maintainer's reading, declared here once; every other page links here. A
  read at node N may serve a row whose workarea is D only if D has no root, or N has D's root: D
  lies outside every in-progress subtree, or N lies inside D's. A **scoped read** (recall, the
  scoped listing, the graph reads, the events tail) serves the **served node set**, the nodes of
  subtree(N) the key reads today that pass at N, so a read scoped outside leaves every subtree
  out whole. A **read with no scope** (get by id, the lineage read, the attachment fetch) is at
  the row's own node, which always passes, and d5 decides who reads a held row. Related rows, a
  lineage closure's and every read's `derives_from` and `replaces`, take the decision at the
  subject's node or the scope node, and a submission reads its `derives_from` targets at its
  resolved workarea. A closure member or edge endpoint that fails the boundary test is omitted,
  never elided to a redacted id, and an edge is served only when both of its endpoints pass.
  Grants, denials, the simulation predicate ([ADR-0013](ADR-0013-reads-served-from-postgres.md)
  d3) and the `self_signed` scope apply beside it. The key's own rows obey it too: scoped
  outside, `include_own_held` and the listing serve none from inside, while get and `own=true`
  still do. The review queue, review next, the verdict and an attachment fetch under a
  `serve_id` are not reads under d4 or d5, so neither the boundary nor a block (d6) narrows
  them: they keep ADR-0025 d1's dreamer effective access on the row's workarea unchanged.

    `oml-domain` holds it as one pure function beside `effective_roles`, giving the decision for
    the roots of N and D and the served node set over a list of nodes; every read path in
    `oml-app` calls it. Nothing is stamped on a memory row, so a re-parent updates no row and the
    commit rewrites nothing. This amends ADR-0009 d2: who reads a row is answered from grants,
    permission epochs, the journalled marks and the roots' statuses.

- **d5 — Inside, a key with reader effective access reads other keys' held claims, marked,
  unless a grant there blocks.** This is the maintainer's reading. A key reads another key's held
  version on node D exactly when D has a root, reader is in `effective_roles(D)`
  ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d1), and no reader grant effective on D
  (its node D or an ancestor, its deny list masking neither) carries `unreviewed: block` (d6); one
  block wins over any inherited allow. `oml-domain` holds this as a pure function beside
  `effective_roles`, computed on every request. Held means state `held`, dead-lettered versions
  included, of any kind. Decision 2's "any state" means held or promoted, confirmed on
  2026-10-31: rejected and demoted versions inside are served as everywhere else, so what review
  turned down never returns to an agent's context.

    Every served claim version carries `reviewed`, true exactly when it is `promoted` and not
    `self_signed` (ADR-0001 d5). Every read applies d4 and then this rule, pointers included:
    `derives_from` and `replaces` name a held version inside to exactly these keys, and
    `410 lifecycle.erased` also answers them for a version erased while held; every other key gets
    `access.not_found`. This amends ADR-0025 d3 (the readability test) and d5 (held text matched
    for its author alone), ADR-0013 d1 (a serving tier of promoted rows) and ADR-0022 d2 (`/v1`
    reads own and promoted rows).

- **d6 — `unreviewed` is a nullable attribute of reader grants, null meaning allow, set to block
  at issuance.** This is the maintainer's reading. A reader grant carries
  `unreviewed: "allow" | "block"`, absent meaning allow, so a crew's keys read its held work with
  no setting; on a grant of any other role it is `request.validation`. The nullable column
  `grants.unreviewed`, null meaning allow and valid on reader rows only, needs no backfill:
  `grant.issued` carries the value when sent, an event without it folds as null, and
  `key.rotated` reproduces each grant with its value. A value never changes; another value is a
  revoked grant and a new one. The wire schema declares no default, and the descriptions state
  what absent means. `POST /v2/keys` and `POST /v2/keys/{key_id}/grants` take it on each reader
  grant and echo it; `oml keys issue` and `oml keys issue-agent` gain `--block-unreviewed`, which
  sets `block` on every reader grant they send, the one [ADR-0028](ADR-0028-agent-key-issuance.md)
  d2 adds included. This amends ADR-0008 d4 (an attribute that narrows a read), ADR-0025 d1 and d2
  (the reader grant and the issuance body) and ADR-0028 d1 and d2 (the flag and its value).

- **d7 — Today's PoC searches `memories` directly, so the boundary is exact there, and any
  future serving index must keep it exact.** This is the maintainer's reading. The PoC has no
  `memories_fts`, `held_fts` or `memory_embeddings` table: recall matches `memories` through the
  GIN index on `to_tsvector('simple', claim)` (`migrations/0001_init.sql`), scores by `ts_rank`
  at normalization 0, which reads only the row and the query, and filters `workarea_id` in the
  statement that applies `LIMIT` (`crates/adapters/oml-pg/src/lib.rs`, `recall_fts`). A held row
  inside therefore scores as it will once promoted, and nothing inside changes an outside recall.
  The use case passes the served node set where it passes `readable` today, and `recall_fts` and
  `ListingFilter` gain `unreviewed: &[Uuid]`, the served nodes where d5 admits the key, which
  match held versions of any author.

    The record that builds the designed indexes (ADR-0013 d1 and d4, ADR-0025 d5, the
    per-simulation index included) must apply the outside exclusion before an approximate scan's
    ceiling and any corpus statistic a score uses, by partition or pre-filter, never by
    post-filtering a shared scan, and must score a held row inside as it will once promoted.
    Property **`P-R3-BOUNDARY`** states the result: a read scoped outside an in-progress subtree
    returns the same rows, count (`hits_omitted` and the budget counts included), order and scores
    whatever rows inside it are added, changed or removed; for the events tail it holds over the
    served events in order, leaving out their `seq`, each page's fill, `head_seq` and
    `next_after_seq` (d12). This amends ADR-0013 d4, whose indexer read promoted text only.

- **d8 — Recall's `include_unreviewed` defaults to `true` and can only narrow; held hits rank and
  count like every hit.** This is the maintainer's reading. `RecallRequest` and `oml_recall`
  gain `include_unreviewed` (boolean, default `true`). With `true`, the candidates also include
  every held version of any author, the key's own included, of kind `memory` or `derived_fact`,
  never a retirement version, on the served nodes where d5 admits the key; `include_own_held`
  still adds own held rows elsewhere in the served set, and nothing is served twice. With `false`
  none is added. Where d5 admits nothing, `true` adds nothing, and the field never answers an
  error. The added hits join ADR-0025 d8's total order, score descending then `artifact_id`
  ascending, with `state: "held"`, `tip: false` and `reviewed: false`, counting against `k` and
  `max_tokens`; a held successor is served beside the tip it would replace. Steering stays
  promoted only. The session-start hook's recall ([ADR-0029](ADR-0029-session-start-hook.md) d2)
  takes the default like any recall, and its hit lines already mark each held claim by its state
  (ADR-0029 d6). The read log records the held ids served, and the request digest carries
  `include_unreviewed` with its default applied. This amends ADR-0025 d4 (own held rows inside
  reach scopes inside only), d8 (hits gain held versions), d9 (the digest) and d12 (`oml_recall`
  gains the field and the claim item `reviewed`; the catalog stays twelve tools), and ADR-0029's
  Context and d2 (the hook's recall serves other keys' held claims inside).

- **d9 — The listings, the graph reads, get and the attachment fetch take d4 and d5, and nothing
  else.** Inside, the scoped listing lists every held version d5 admits beside the promoted
  served tips, marked by `state` and `reviewed`, and `state=promoted` narrows it; the graph reads
  (`GET /v2/claim-edges`, `oml_graph`) serve the held claims d5 admits; get and the attachment
  fetch of a held version serve the keys d5 admits on its workarea. Scoped outside, each serves
  no claim from inside and no edge with an endpoint that fails d4 at the scope. A graph read's
  scope node is its `workarea_id` or `scope`, the key's default workarea when it is omitted, as
  [`oml_graph`](../api/mcp-plugin.md#oml_graph) declares, and `oml_graph`'s lineage leg takes
  that node, not its subject's. This amends ADR-0025 d13 (the scoped listing).

- **d10 — The events tail is a scoped read at its `workarea_id` (d4), and inside, get's test
  decides a `lifecycle.*` event.** This is the maintainer's reading. A `lifecycle.*`,
  `submission.*` or `simulation.write` event whose subject's workarea, or a listed workarea of a
  `lifecycle.reassign`, fails d4 at the tail's `workarea_id` is omitted, so a tail scoped outside
  an in-progress subtree serves none from inside until the commit. The rest follow ADR-0025 d15's
  family table unchanged, except that a `lifecycle.*` event whose subject is inside is served
  exactly when get of its subject would serve the key, `410 lifecycle.erased` included, under
  d15's self-signed condition. `workarea.in_progress_marked` maps as `workarea.create` does, and
  the test runs at the request on the current tree, marks and access. This amends ADR-0025 d15:
  inside, a `lifecycle.*` event follows its subject's current state.

- **d11 — The workarea surface gains a create flag, an opt-in verb and the root on the read.**
  `WorkareaCreate.in_progress` (boolean, default `false`); `POST
  /v2/workareas/{workarea_id}/in-progress`, admin key role, no body, journalled as
  `workarea.in_progress_marked`, answering `200` with the workarea; `Workarea.in_progress_root_id`,
  the node's root computed at the read, the node itself for a root, null outside; and the CLI's
  `oml workareas create --in-progress` and `oml workareas in-progress <workarea>`.
  `my_effective_verbs` and `permission_epoch` are unchanged: d5 turns on a mark and a grant
  attribute, not a role, and the mark is read beside the epoch. This amends ADR-0025 d14.

- **d12 — The commit is the root's `development` → `committed` transition under ADR-0009's
  rules, refused while any version inside is held.** This is the maintainer's reading. Different
  identities propose and ratify it, each a dreamer or an admin (ADR-0009 d3). Both acts are
  refused with `workarea.held_remaining`, details `{workarea_id}` and no count, while any version
  in the subtree is in state `held`, dead-lettered ones included; rejected, demoted and erased
  versions do not block, and a refused ratification journals nothing. The ratification's test
  runs in round trip two's statement under the chain lock, which every capture takes, so no held
  version arrives between test and commit: one `EXISTS` over the subtree's nodes through the
  declared index `memories_workarea (tenant_id, workarea_id, state)`
  ([Postgres schema](../data/postgres-schema.md)), one probe per node, unmeasured
  ([FR-25](../product/roadmap.md)). d1's subtree test runs the same way. This amends
  [ADR-0024](ADR-0024-performance-foundation.md) d1, whose lock admits no scan; ADR-0008 d6's
  re-parent already holds a lock proportional to its subtree.

    The commit journals the existing `workarea.status_transition` unchanged. From it no node of
    the former subtree has a root, so its promoted claims reach every scope and its nodes are
    ordinary `development` workareas. The refusal tells a proposer that some version in the
    subtree is held, possibly where it cannot read, a signal [FR-31](../product/roadmap.md) gains
    at acceptance, beside the in-progress form of its event-feed count: an outside tail's `seq`
    numbering and each page's fill count the events inside. This amends ADR-0009 d3 (the
    precondition) and d1 (a `development` workarea's only relaxation was self-signing).

- **d13 — Two codes, the wire, the version and the vocabulary.**
    - **Codes**, each `409`, in the [errors](../api/errors.md) tables and the `ErrorCode` enum:
      `workarea.held_remaining` (d12) and `workarea.in_progress_refused` (d1, d2).
      `request.validation` gains `in_progress: true` with `prefix`, and `unreviewed` on a grant
      that is not a reader grant. No other code changes.
    - **Shapes**: those d5, d6, d8 and d11 name, optional while this record is proposed, with
      `MemorySummary.reviewed` and `Workarea.in_progress_root_id` required from its acceptance; the
      payload fields `workarea.create.in_progress` and `grant.issued.unreviewed`; the columns
      `workareas.in_progress` and `grants.unreviewed`.
    - **The version** moves to `0.3.0-design`, since `/v1` shapes change; the `spec-v0.3` tag
      waits for acceptance. [mcp-tools.json](../api/mcp-tools.json)'s `_meta.pending_design`
      names what this record adds to `oml_recall` and `oml_workarea_info`.
    - **The vocabulary** of ADR-0018 d4, at acceptance: the "held" row's meaning gains "and,
      inside an in-progress workarea, the readers ADR-0030 d5 admits", its "never" column
      unchanged, so "unreviewed" names the attribute and the recall field, never a state. Two rows
      are added, banning no spelling in current use:

    | Term | Means | Never |
    |---|---|---|
    | in-progress workarea | a `development` workarea a journalled admin act marked, with its subtree, until its commit; unrelated to `idempotency.in_progress` | "draft workarea", "live workarea" |
    | commit (of an in-progress workarea) | the root's ratified `development` → `committed` transition, which ends its subtree | — |

## Consequences

- **What a crew gets.** Its agents recall each other's claims at once, marked, while review keeps
  its own pace. A committed root is never marked again, so the next iteration marks a new node,
  such as a sibling with a `continues` edge ([workareas](../architecture/workareas.md#edges)).
- **A rejection claws nothing back from the readers inside.** What agents read and derived from
  stays read, as the read log records; a declared derivation puts its author in R2's exclusion
  set, and undeclared derivation stays undetectable (ADR-0001 d6). `reviewed: false` is the
  mitigation, and the `oml-memory` skill's instruction to weigh such claims lands with its code.
- **Marking an existing workarea takes its promoted claims from every read scoped outside it**
  until the commit; for `crew/oml`, every promoted claim in it.
- **The commit waits on review**: one held claim no eligible reviewer can judge holds the whole
  subtree until it is reviewed, dead-lettered and re-enqueued, or erased.
- **Get reads at the row's node** (d4): a key with reader access on an ancestor can get a
  promoted claim inside by id before the commit, and a held one when d5 admits it; recall, the
  listings, the graph reads and the tail scoped outside serve nothing from inside. Inside, the
  tail follows current state (d10), so a range re-read after a rejection or the commit can omit
  an event it showed.
- **Two tests run under the chain lock** (d1, d12), one probe per subtree node, unmeasured
  (FR-25), and **a future serving index meets d7's requirements**, which rule out one shared
  approximate graph filtered after its ceiling. **All of it is unbuilt.**

**Process obligations**

- **With the proposal**: this file, a [register](index.md) row, a `mkdocs.yml` nav entry,
  `llms.txt`'s mention as proposed, a [changelog](../changelog.md) entry for `0.3.0-design`, and
  d13 on [openapi.yaml](../api/openapi.yaml), [errors](../api/errors.md), the
  [HTTP API](../api/http-api.md) and [mcp-tools.json](../api/mcp-tools.json), agreeing as
  `scripts/check_contract_sync.py` checks, each mention marked "proposed (ADR-0030)". No other
  page states the relaxation.
- **On acceptance, and not before**:
    - this record and its register row become `accepted design`, dated, and every record on the
      Amends line gains "· amended by ADR-0030" on its Status line and frontmatter;
    - an accepted record whose sentence would otherwise read false gains a dated note beside its
      kept text, as ADR-0012 d1's "(wording amended 2026-10-30 — …)" does: ADR-0001 d5, which per
      its last sentence takes the second relaxation with a pointer to d3, and its H1 and
      `answers`; ADR-0009 d1 and ADR-0019 d1 ("the one bounded … exception"); ADR-0018 d4's table;
      ADR-0029's Context ("no other key's held row (R3)");
    - the pages: workareas (a section linking d4, the refusal matrix), roles and keys (the
      permission matrix linking d5, d6), retrieval (d7, d8), admission (d3), the overview's R3 row,
      simulations, security (d10), the Postgres schema and event catalog (d13), the CLI reference,
      the agent plugin, the test strategy (`P-R3-BOUNDARY`, `P-R2-INPROGRESS`, `P-R3-VISIBILITY`),
      the roadmap (FR-31), concepts, commitments C4, the review guide, the README, the home page,
      `llms.txt`, `CONTRIBUTING.md`, and the D7, D8, D9, D11, D12, D13 and D14 cells;
    - the contract surfaces drop "proposed", d13's two properties become required, and the
      `spec-v0.3` tag is cut.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A mark set only at creation** | `crew/oml` already exists and holds rows, so it could never be marked. |
| **d1 — Rows promoted before the opt-in stay visible outside** | Only a stamp per row could tell them apart, which d4 rejects, and one subtree would have two release rules. |
| **d1 — Mark a subtree holding a `committed`, `validated` or vocabulary node** | R3 would hold on some nodes inside and not others, and a term's CURIE, cited across the tenant, could be cited before the term is reviewed. |
| **d1 — A verb that clears the mark, or a fourth status** | The first contributes while skipping the commit's precondition; a status selects review policy (ADR-0009 d2), while the mark changes who reads. |
| **d1 — An agent-callable mark, or a simulation whose audience is the crew** | The mark changes what other keys read, which R1 keeps off agent surfaces; a simulation's end demotes its rows (ADR-0011), so the crew's work could never contribute. |
| **d2 — Nested subtrees** | A node would have two roots, and no single commit would release it. |
| **d2 — Reassignment, re-parent, lineage or edges across the boundary** | Out contributes one claim ahead of the rest; in takes a promoted claim from its readers without a demotion; a lineage or edge carries an id, or text, out ahead of the commit. |
| **d2 — `workarea.in_progress_refused` for a submission outside naming a target inside** | It would confirm, where the target is invisible, that the target exists; d4 already answers `access.not_found`. |
| **d3 — Self-signing inside, or the review policy deciding alone, `k: 0` included** | The maintainer keeps R2; a row inside could be promoted on no verdict, read `reviewed: true`, and leave at the commit unreviewed. |
| **d4 — The boundary stamped on rows: a root column on memory and index rows behind a row-level-security fence, or an event's inside-ness judged at the event** | Every re-parent, reassignment, removal and indexer write would need a checked row update under the right setting, and the release would depend on each having run; the earlier draft took that form, and review kept finding its failure modes. Judging an event needs a stamp or a replay of the journal per event. |
| **d5 — Only the crew's own keys, or a supervisor access that reads held claims anywhere** | The maintainer chose every reader, and "the crew's keys" is not a concept the access model has; a supervisor access relaxes R3 per key across the tree. |
| **d6 — A key-level setting, or block by default** | The maintainer rejected the first as a second access mechanism beside grants; under the second, an omitted setting fails silently, as an empty recall. |
| **d6 — An allow on any covering grant wins** | A broader grant issued for another purpose would undo a deliberate block; a deny wins for the same reason. |
| **d6 — A `NOT NULL` column, or a schema default of `allow`** | The first needs a backfill and a fold rule for every earlier reader grant; a client filling the second would send it on writer and dreamer grants and be refused. |
| **d7 — One shared approximate scan, filtered after its ceiling** | Held rows inside would use up an outside scan's ceiling and change which promoted hits, and how many, an outside recall returns; a probe of query vectors would locate them. |
| **d7 — Held claims inside scored by text alone, as own held rows are** | Ratification would raise them in every reader's results, so review would reorder results (R3). |
| **d8 — Refuse `include_unreviewed: true` where a grant blocks** | An agent configured once would fail recalls over a narrowing its grants already make. |
| **d8 — A separate list of held hits, or held steering in segment 1** | A list needs its own order and budget share (ADR-0025 d4's rejected segment); steering no one reviewed, served first and never cut, is the most privileged place for the least checked text. |
| **d10 — An event read at its subject's own node, as get is, or a third tail clause for in-progress subjects** | The first serves the events of a subtree's claims to a tail scoped above it before the commit (decision 4, `P-R3-BOUNDARY`); grafted onto "(a) or (b), and no other", the second was unreachable. |
| **d11 — An `in_progress` field, or a `reads_unreviewed` field, on the workarea read** | On the create body `in_progress` asks for the mark, so one name would mean two things; each served claim carries `reviewed` and `include_unreviewed` only narrows, so an agent needs no prior answer. |
| **d12 — An admin-only commit** | ADR-0009 d3 already takes two identities, and a ceiling that depends on the node would need a new step in the refusal order. |
| **d12 — Per-claim contribution, held claims left over, or rejecting them at the commit** | The maintainer chose whole-workarea; leftovers would leave unreviewed; an automatic rejection records verdicts no reviewer gave (ADR-0025 d10). |
| **d12 — The commit moves every node to `committed`, or carries a held count** | A descendant would reach `committed` with no transition of its own (ADR-0009 d3); a count tells a proposer how many held claims sit where it may not read. |
| **d13 — Reuse `workarea.transition_refused`** | Its rule is the ladder's shape; this refusal's remedy is review. |
