<!--
id: adr/ADR-0030-in-progress-workareas
tier: engineering
status: proposed
answers: what an in-progress workarea is and who makes one; which keys read its held claims, marked, and how a reader grant blocks that; how recall, get, the listings and the events tail treat it inside and outside; why R2 still holds there and how it coexists with self-signing; how its whole subtree contributes at once at a commit that nothing held may precede
-->

# ADR-0030 — An in-progress workarea serves its held claims to its readers at once, marked, and contributes everything at once when a commit finds nothing held

**Status:** proposed · 2026-10-31

**Amends:** ADR-0001 (d3, d5), ADR-0008 (d4), ADR-0009 (d1, d3, d4), ADR-0013 (d1, d4), ADR-0018 (d4), ADR-0025 (d1, d2, d3, d4, d5, d8, d9, d12, d13, d14, d15), ADR-0028 (d1, d2)

## Context

**What a crew working on one project needs.** A crew of agents on one project writes memory
that the next agent in the same crew needs within minutes. Under R3 every write is held and
readable by its author alone until a non-author ratifies it
([ADR-0001](ADR-0001-the-constitution.md) d3), so a crew's own agents cannot use each other's
work before review catches up. Review throughput is unmeasured
([FR-2](../product/roadmap.md)), and a crew that waits on it either stalls or writes outside the
gate.

**What the design already rejected, and why it is reopened bounded.**
[ADR-0012](ADR-0012-admission-and-review.md) rejected *optimistic review — visible until
rejected*: serving a held claim to others breaks R3, and a rejection would have to claw back
what other agents read and derived from. This record reopens that alternative for one bounded
case and answers both objections. Visibility before review is confined to one subtree an admin
created for the purpose, every held claim it serves is marked `reviewed: false`, and nothing leaves the
subtree until every claim in it has been reviewed. A rejection still claws nothing back from
the readers inside; that cost is stated in Consequences.

**What the constitution allows today.** [ADR-0001](ADR-0001-the-constitution.md) d5 scopes one
bounded exception: a `development` workarea may, by journalled tenant policy, relax R2 and R3
for rows marked `self_signed`. That relaxation lets an author approve its own row. The
maintainer wants the opposite trade: R2 kept, review deferred, visibility widened inside and
held back outside. d5 says no other page may widen its scope and that a change to it is a new
decision on that record. This is that decision.

**The maintainer's decisions.** The maintainer fixed these points on 2026-10-31, and this record
designs around them without reopening them:

1. an in-progress workarea is a `development` workarea that a journalled admin act opts in;
2. inside, held claims are readable at once by every reader, ranked with the promoted hits and
   marked `state: "held"` and `reviewed: false`, and recall gains `include_unreviewed`, on by
   default when the scope is in-progress;
3. R2 is kept: review is deferred, never self-signed;
4. contribution is all at once, for the whole workarea, at its move to `committed`, which is
   refused while anything in it is held;
5. errors and wire land in the contract surfaces, and `info.version` moves to `0.3.0-design`;
6. a key's access to the held claims of other keys reaches only inside in-progress
   workareas;
7. a reader grant allows it unless issuance blocks it;
8. that access is an attribute on reader grants, `unreviewed: allow | block`.

How the workarea read reports that access, and how recall answers a blocked key, the
maintainer left to this record (d8, d11).

**The pages this record is held against.** The ladder and its refusals are declared on
[workareas](../architecture/workareas.md) (D8); who may read is the
[permission matrix](../architecture/roles-and-keys.md#permission-matrix) (D9); recall and its
indexes are [retrieval](../architecture/retrieval.md) (D12); the wire is the
[HTTP API](../api/http-api.md) (D13), [openapi.yaml](../api/openapi.yaml),
[errors](../api/errors.md), the [agent plugin](../api/mcp-plugin.md) and
[mcp-tools.json](../api/mcp-tools.json). Each rule below is declared on one of those pages and
linked from the rest ([ADR-0018](ADR-0018-the-documentation-set.md) d5).

Decided, not built.

## Decision

- **d1 — An in-progress workarea is created as one, by an admin, and stays one until its
  commit.** `POST /v2/workareas` takes `in_progress` (boolean, default `false`), and
  `workarea.create` journals it. The verb keeps its ceiling, the admin key role: no MCP tool and
  no agent role reaches it (R1), because the mark changes what other keys can read. The mark is
  set only at creation. No later verb sets it, and none clears it: the workarea stops being
  in-progress only when its own `development` → `committed` transition is ratified (d12). It is
  not a fourth status: `development`, `committed` and `validated` stay the only statuses
  ([ADR-0009](ADR-0009-status-ladder.md) d1), and an in-progress workarea is a `development`
  workarea carrying the mark.

    Creation with `in_progress: true` is refused under a `validated` parent
    (`workarea.frozen`, as today), inside another in-progress workarea's subtree
    (`workarea.in_progress_refused`: subtrees never nest), and together with `prefix`
    (`request.validation`). A vocabulary's terms are cited across the tenant by permanent CURIE,
    and a CURIE readable before its term is reviewed could be cited, and its citations scanned,
    against a term that is then rejected.

    The workarea read carries `in_progress` (true for the in-progress workarea and every node in
    its subtree while it is `development`) and `in_progress_root_id` (the in-progress workarea
    whose subtree holds the node, else null); each entry of `children` carries `in_progress`
    (d11).

    A mark set only at creation cannot reach a row written before it. A later opt-in would show
    every reader the held rows their authors wrote when only they could read them, and would
    take promoted rows away from the readers of the parents with no per-row journalled act, the
    read-path visibility switch [ADR-0009](ADR-0009-status-ladder.md) rejected as a fourth
    status. An off switch would be a second way to contribute that skips the two-identity commit
    and its precondition.

- **d2 — The in-progress workarea and its descendants are one subtree: every node in it stays
  `development` until the commit, and no claim or node crosses its boundary before.** A child
  created under an in-progress workarea, or under any node in its subtree, is a `development`
  workarea inside that subtree. While the subtree is in progress:

    - a status transition proposed on any node in it other than the in-progress workarea itself
      is refused with `workarea.in_progress_refused`: the subtree moves up the ladder only at
      its root. This sits beside ADR-0009 d4's child-status constraint, which still governs the
      later move to `validated`;
    - a reassignment of a claim into or out of the subtree is refused with
      `workarea.in_progress_refused`; a reassignment between two nodes inside it is allowed
      under the existing rules;
    - a re-parent that would move a node into or out of the subtree is refused with
      `workarea.in_progress_refused`; re-parenting the in-progress workarea itself moves the
      subtree whole and is allowed under the existing rules;
    - a `prefix` on a child created inside it is refused with `workarea.in_progress_refused`
      (d1).

    The refusals are declared in the
    [refusal matrix](../architecture/workareas.md#refusal-matrix) of workareas. A crew
    structures a project in sub-areas, so the rule covers the subtree, not one node. A descendant marked `committed` or `validated` before the root would
    claim a stage the subtree has not reached, since nothing in it has left the subtree. A
    reassignment out would contribute one claim ahead of the rest; one in would take a promoted
    claim away from the readers of its old workarea without a demotion. This amends ADR-0009 d1,
    which lists what a status refuses, and d4, beside which the new constraint sits.

- **d3 — The constitution gains a second bounded relaxation, of R3's first sentence only, and R2
  holds inside it unconditionally.** ADR-0001 d3 reads: "Nothing is readable by others until it
  is ratified. Review never reorders results, and steering rows, which precede hits, are never
  ranked." Under this record the first sentence holds everywhere outside an in-progress
  subtree. Inside one, a held claim is readable by the keys
  that the [permission matrix](../architecture/roles-and-keys.md#permission-matrix) allows (d5,
  d6, d7), marked `state: "held"` and `reviewed: false`, and nothing in the subtree is readable
  at any scope outside it until its commit (d10, d12). The second sentence holds inside without
  exception (d9).

    ADR-0001 d5 reads: "R2 and R3 hold unconditionally in `committed` and `validated`
    workareas. A `development` workarea MAY relax them by journalled tenant policy." Under this
    record it scopes two relaxations, both on that record and nowhere else:

    1. **self-signing**, unchanged: R2 and R3 relaxed in one `development` workarea for rows
       marked `self_signed`, served only at their own workarea, never promoted beyond it, never
       in silver or gold;
    2. **in-progress**: R3's first sentence relaxed inside an in-progress subtree, as above. R2
       is not relaxed, and every claim inside promotes only by ratification on non-author
       verdicts under its workarea's review policy.

    R2 holds unconditionally everywhere, and R3 holds unconditionally in `committed` and
    `validated` workareas, as before. No other page may widen either scope.

    The constitution states its exceptions on the day they are made (ADR-0001 d5). A relaxation
    that keeps R2 is narrower in authority than self-signing and wider in audience, so it is
    named separately rather than folded into the first. This amends ADR-0001 d3 and d5, and
    ADR-0018 d4's "held" row (d13).

- **d4 — Self-signing and in-progress exclude each other: no workarea inside an in-progress
  subtree admits self-signing.** A journalled policy act that would let a workarea inside an
  in-progress subtree self-sign is refused with `workarea.in_progress_refused`, and a workarea
  created in-progress, or created inside one, never admits it. A row is therefore never both
  `self_signed` and inside an in-progress subtree. Creating an in-progress workarea under a
  parent that self-signs is allowed, because review policy is per workarea
  ([admission](../architecture/admission.md#review-policy-objects)). After the commit (d12) the
  former subtree's `development` descendants are ordinary `development` workareas, and their
  policy may admit self-signing from then on.

    The maintainer keeps R2 inside. A self-signed row inside would also reach readers outside at
    the commit with no second identity behind it, which ADR-0001 d5 forbids ("never promote
    beyond it"). Refusing the combination is one check at the policy act. This amends ADR-0001
    d5.

- **d5 — A key reads other keys' held claims only inside in-progress workareas.** Access to
  the held claims that other keys wrote, **unreviewed access**, reaches only nodes inside an
  in-progress subtree. A reader grant on `universe` that allows it reads held claims in every
  in-progress workarea of the tenant and in no other workarea. `committed` and `validated` workareas, and `development`
  workareas outside any in-progress subtree, keep R3 unconditionally. The rule is declared once,
  on the [permission matrix](../architecture/roles-and-keys.md#permission-matrix); every other
  page links there.

    The claims readable this way are the held versions of every kind, dead-lettered ones
    included, under every other read predicate: global denials, the simulation audience
    ([simulations](../architecture/simulations.md)) and the naming rule of
    [own](../architecture/roles-and-keys.md#own). Each served claim carries `reviewed`, which is
    true exactly when its state is `promoted` and it is not `self_signed`, on the ring's
    `MemorySummary` and on the agent plugin's claim item, everywhere a claim version is served.
    A `self_signed` promoted row therefore reads `reviewed: false` too, since a self-signature is
    never called a review (ADR-0001 d5). Rejected, demoted and erased versions are served
    inside as everywhere else: to their author only.

    "Inside" and "outside" describe a read's scope, never the key: a key whose reader grant on
    an ancestor covers an in-progress workarea is one of its readers under
    [ADR-0008](ADR-0008-workarea-algebra.md) d4, and it reads the subtree by scoping inside it
    or by id (d10).

    A supervisor-style access that read held claims anywhere the key can read would relax R3
    per key across the whole tree and expose every crew's held work. This amends ADR-0025 d3:
    a held claim inside an in-progress subtree that the key may read joins what "readable"
    means for a `derives_from` target. A supersede target must still be the served tip.

- **d6 — A reader grant confers unreviewed access by default, and issuance can block it per
  grant.** A reader grant that says nothing allows its holder to read the held claims of the in-progress
  workareas it covers, so every key in a crew reads its crew's held work with no setting at
  issuance. A key holding no reader grant reads none. A key whose reader grants block it still
  reads its own held rows, in any state, under the own-work rule
  ([own](../architecture/roles-and-keys.md#own)): unreviewed access concerns other keys' claims.

    A default of block would make every agent key in an in-progress crew need the setting at
    issuance, and an omitted setting would fail silently, as an empty recall. The block exists
    for a key that must read a crew's reviewed claims only: an outside monitor, or an agent whose
    instructions say to trust nothing a non-author has not reviewed.

- **d7 — Unreviewed access is the `unreviewed` attribute of a reader grant, `allow` or `block`,
  and a block on any reader grant effective on a node wins there.** Each reader grant carries
  `unreviewed: "allow" | "block"`, default `allow`. A grant of another role that carries the
  attribute is refused with `request.validation`. The key may read held claims of other keys on
  node N exactly when:

    1. N lies inside an in-progress subtree (d5);
    2. reader is in `effective_roles(N)` ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md)
       d1);
    3. no reader grant effective on N carries `unreviewed: block`. A grant is effective on N when
       its node is N or an ancestor of N and its own deny list names neither N nor an ancestor of
       N, so a grant whose deny masks N neither allows nor blocks there.

    A key that needs it everywhere holds one reader grant on `universe` with `allow`. A key that
    needs it per workarea holds one reader grant per node, resolved the usual way: the union of
    grants, attenuated by their deny lists, minus global denials. The result is a pure
    `oml-domain` function beside `effective_roles`, computed on every request with no cache, like
    effective access ([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)).

    The attribute is journalled like every grant attribute, in the style of the proposed
    ADR-0027's `edge_labels` narrowing on dreamer grants: `grant.issued` carries it, the grant
    row stores it, the `201` of issuance and the key detail echo it, and the permission epoch
    labels it. A grant's value never changes; another value is a revoked grant and a new
    `grant.issued`.

    - On the wire, `POST /v2/keys` and `POST /v2/keys/{key_id}/grants` take `unreviewed` on each
      reader grant.
    - In the [CLI](../cli/reference.md), `oml keys issue`, `oml keys issue-agent` and
      `oml grants add` gain `--unreviewed <allow|block>`, default `allow`. It sets the value on
      every reader grant the command sends, the reader grant that
      [ADR-0028](ADR-0028-agent-key-issuance.md) d2 adds beside a writer grant included.

    A block wins for the reason a deny wins: a grant that withholds one read must not be undone
    by a broader grant the issuer added for another purpose. The rule is declared on the
    [permission matrix](../architecture/roles-and-keys.md#permission-matrix).

    This amends three records:

    - ADR-0008 d4: a grant attribute narrows one read, resolved block-wins over the same grants;
    - ADR-0025 d1 and d2: a reader grant carries an attribute as well as a role, and the issuance
      body carries it;
    - ADR-0028 d1 and d2: `issue-agent` gains the flag, and the added reader grant carries its
      value.

- **d8 — Recall's `include_unreviewed` defaults to the key's unreviewed access on the scope node,
  and the hits it adds are ranked and marked like every other hit.** `RecallRequest` and
  `oml_recall` gain `include_unreviewed` (boolean).

    - **The default** is `true` when the key may read held claims of other keys on the scope node
      (d7), and `false` otherwise: outside every in-progress subtree, and wherever a reader grant
      effective on the scope blocks it.
    - **With `true` at a scope inside an in-progress subtree**, the hit candidates also include
      every held version, of any author, of kind `memory` or `derived_fact`, never a retirement
      version, that lies in a workarea within subtree(scope) where the key may read held claims of
      other keys, and that the query and the `kinds` filter admit. The key's own held rows in
      those workareas are among them; `include_own_held` adds its own held rows elsewhere in the
      subtree, and nothing is served twice.
      Workareas in the subtree where a grant blocks contribute their promoted hits only; that is
      absence, as a per-grant deny is absence in any subtree read.
    - **An explicit `true` where the scope node itself blocks** is refused with
      `access.grant_refused`, details `{workarea_id, needed_role: "reader", unreviewed: "block"}`:
      the request names rows that exist under the rule and that the key's grants withhold. A
      default recall never asks for them, so it never refuses.
    - **With `true` at a scope outside every in-progress subtree**, nothing is added and nothing
      is refused. No held claim of another key is readable at such a scope (R3), so the answer
      is exact. The key's own held rows still join only under `include_own_held`.

    The added hits merge with the promoted hits under the one total order of ADR-0025 d8, score
    descending then `artifact_id` ascending. They carry `state: "held"`, `tip: false` and
    `reviewed: false`, a held successor carries `replaces` beside the promoted tip it would
    replace, and they count against `k` and `max_tokens` like any hit. Applicable steering stays
    promoted steering only, so a held steering row never enters segment 1, and a held retirement
    version never takes a claim out of recall. Both are reachable by get and the listings.

    The read log records the held ids served like any others. The request digest gains the
    member `include_unreviewed`, with the scope's default applied, so one request digests alike
    on every server. Absent, the field gives exactly the prior answer at every scope that could
    exist before this record. The rule is declared on
    [retrieval](../architecture/retrieval.md#held-claims-inside-an-in-progress-workarea).

    One ranked list marked by state is the shape ADR-0025 d4 chose for the author's own held rows,
    for the same reason: a second list would need its own order and its own share of the budget.
    Steering stays promoted because the vocabulary defines it as a promoted standing
    instruction (ADR-0018 d4), and an instruction no one has reviewed, served ahead of every hit and never
    cut is the most privileged place for the least checked text. This amends ADR-0025 d4, whose
    own held rows inside an in-progress subtree are reached at scopes inside it only (d10), d8,
    whose hits gain these rows, and d9, whose digest gains a member.

- **d9 — Inside an in-progress subtree, held and promoted claims are indexed alike, behind a
  fence keyed by the subtree, so ratification changes no score and the commit releases every
  row at once.** Every held and promoted version in a workarea inside an in-progress subtree
  has the `memories_fts` row and the `memory_embeddings` row it would have once promoted outside
  one, a held one from its capture on; a simulation row keeps its per-simulation index. Each such
  row carries
  `in_progress_root_id`, the in-progress workarea whose subtree holds it, and `held_fts` holds
  none of them. A restrictive row-level security policy on both tables serves a row whose
  `in_progress_root_id` names a workarea still in `development` only to a transaction whose
  `oml.in_progress_root` names that workarea:

    - **reads**: `oml-server` sets `oml.in_progress_root` at the start of every transaction from
      the resolved scope node, never from the client, and leaves it empty at a scope outside;
    - **removals**: a commit that removes a version's index rows sets it, transaction-local, to
      that version's root, read from the version's row in round trip one;
    - **the check**: the FTS removal is checked to affect exactly one row, as `held_fts` removals
      are (ADR-0025 d5). Both rows sit behind the same fence under the same setting, so a
      passing check also shows the embedding removal ran where its row is visible.

    The policy reads the root's status, so the commit, which changes one `workareas` row, makes
    every one of those index rows matchable at scopes outside, in its own commit.

    `oml-indexer` embeds a held version inside the subtree as it embeds a promoted one. It is
    served the text on the serving path it reads promoted text from, and gains no credential that
    reaches quarantine ([admission](../architecture/admission.md#quarantine-placement)).
    Ratification inside changes no index row, and a reassignment between two nodes of one
    subtree only updates its rows' workarea; rejection, demotion and erasure remove the rows in
    their own commit. Held rows still never enter silver or gold: silver materialization stays
    promotion-only ([ADR-0012](ADR-0012-admission-and-review.md) d1).

    The PoC form is one port method whose SQL takes the in-progress root and the readable
    workareas, plus a predicate that keeps every recall scoped outside off the subtree. A
    port-contract test run against both stores asserts that a scope outside never returns a row
    inside, whatever the query. The code and the README state the gap to the designed form.

    Held claims without a vector term score below their promoted rank (ADR-0025 d4), so
    ratification would raise them in every reader's results, and review would reorder results
    against R3. A fence keyed by the root and released by its status makes the commit one row
    update, with nothing proportional to the subtree inside the chain lock
    ([ADR-0024](ADR-0024-performance-foundation.md) d1). It is containment by construction
    ([ADR-0013](ADR-0013-reads-served-from-postgres.md) d3): a query that forgets the boundary
    still sees nothing behind it. This amends ADR-0013 d1, whose serving tier held promoted rows,
    ADR-0013 d4, whose indexer read promoted text, and ADR-0025 d5, under which the shared
    indexes held promoted rows only.

- **d10 — By-id reads see inside under access, scoped reads see inside only from inside, and no
  read scoped outside leaks that the subtree holds anything.**
    - **Get** of a held version inside, and the fetch of its attachments, are served to a key
      that may read held claims of other keys on its workarea (d7); any other key gets
      `access.not_found`. Get takes the row's own workarea as its implicit scope, so it reads
      inside under access. The lineage read follows the same test.
    - **The scoped listing** (`GET /v1/memories` without `own`) at a scope inside an in-progress
      subtree lists promoted served tips and every held version, of any kind, in the workareas
      where the key may read held claims of other keys, each marked by `state` and `reviewed`;
      `state=promoted` narrows it to reviewed rows. **The own-work listing** (`own=true`) is
      unchanged.
    - **The events tail** at a `workarea_id` inside an in-progress subtree shows a third clause,
      the in-progress clause: a `submission.*`, `simulation.write` or `lifecycle.*` event whose
      subject lay inside that subtree when the event was journalled is shown to a key that may
      read held claims of other keys on the subject's workarea, under the audience condition,
      with identities nulled as on every event served outside its actor's key set.
    - **The boundary**: every read scoped outside the subtree, at an ancestor up to `universe`,
      skips it whole, the reading key's own rows included:
        - recall serves nothing from it, and the budget's counts and `hits_omitted` count
          nothing from it;
        - the scoped listing lists nothing from it;
        - the events tail shows none of the claim events journalled while the subtree was in
          progress, before the commit or after, under any clause.
    - **The node** stays visible to its readers as structure: the workarea read lists it among
      `children` with `in_progress: true`, and its `workarea.*` events show at the tails of keys
      that can read it.

    The tail's rule is declared on the [HTTP API](../api/http-api.md#events-tail); recall's on
    [retrieval](../architecture/retrieval.md#held-claims-inside-an-in-progress-workarea).

    The maintainer chose the same treatment inside for get, the listings and the tail. Submission
    events are how a tailing agent learns of a crew mate's new claim. A rule judged on the
    journal up to the event shows two reads of one range the same events, which is why the
    events of the in-progress period never reach an outside tail, even after the commit
    (ADR-0025 d15). The outside tail learns of the commit from its `workarea.status_transition`,
    and of the rows from recall. Skipping own rows at outside scopes keeps one boundary rule for
    every scoped read. This amends ADR-0025 d13, whose scoped listing never served another key's
    held row, and d15.

- **d11 — The workarea read reports the mark and the caller's unreviewed access there.** `GET
  /v2/workareas/{workarea_id}` and `oml_workarea_info` add three fields:

    - `in_progress` and `in_progress_root_id` (d1);
    - `reads_unreviewed`, true exactly when the calling key may read held claims of other keys on
      the node (d7), and therefore what `include_unreviewed` defaults to at that scope (d8);
    - `in_progress` on each `children` entry.

    `my_effective_verbs` is unchanged. Its set is the union over the grant roles effective on the
    node (ADR-0025 d14), and unreviewed access turns on the node's mark and a grant attribute, not
    on a role. An agent reads one boolean before it trusts what recall will mix in. This amends
    ADR-0025 d14.

- **d12 — The whole subtree contributes at once: the commit is the in-progress workarea's
  `development` → `committed` transition, refused while any version in the subtree is held.**
  The transition is proposed and ratified by different identities exactly as
  [ADR-0009](ADR-0009-status-ladder.md) d3 requires. A proposal is refused with
  `workarea.held_remaining`, details `{workarea_id}`, while any version in the subtree is in state
  `held`, dead-lettered ones included. A ratification is refused with the same code under the
  same test. The ratification's test is a condition inside round trip two's one statement, as the
  supersession compare-and-set is ([ADR-0024](ADR-0024-performance-foundation.md) d1): every
  capture takes the chain lock, so no held version can arrive unseen between the test and the
  commit. A refused ratification journals nothing and leaves the transition pending. Rejected,
  demoted and erased versions do not block.

    At the commit, in one journal event, `workarea.status_transition` with
    `in_progress_released: true`:

    - the in-progress workarea becomes `committed`, and its subtree stops being in progress;
    - every promoted served tip in the subtree becomes readable at scopes outside under the
      ordinary rules, in the same commit (d9);
    - the descendants keep their status, `development`, and become ordinary `development`
      workareas: what is written in them afterwards is held, readable by its author alone, and
      readable to others once promoted, claim by claim;
    - nothing held exists in the subtree (the precondition), so no held claim changes audience,
      and rejected rows stay unserved;
    - no index row and no lake row is rewritten. A promoted version inside entered silver at its
      own promotion, which serves no client read ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md)).

    The test covers the whole subtree, and a dreamer proposer may lack access on some node in
    it. The refusal therefore carries no count, and it tells the proposer only that some version
    in the subtree is still held, possibly in a node the proposer cannot read. That signal is
    tracked as [FR-31](../product/roadmap.md).

    The maintainer chose whole-workarea contribution. Committing with held rows left over would
    take from the readers inside rows they relied on and would contribute a subtree not fully
    reviewed. Rejecting them automatically would record verdicts no reviewer gave (ADR-0025 d10).
    This amends ADR-0009 d3: the transition gains a precondition and an effect.

- **d13 — Two new codes, the wire changes, and the version moves to `0.3.0-design`.**
    - **Codes**, each `409`, landing in the [errors](../api/errors.md) family table and the status
      index, the `ErrorCode` enum and the [HTTP API](../api/http-api.md) in one change:
      `workarea.held_remaining` (d12) and `workarea.in_progress_refused` (d1, d2, d4).
    - **Reused codes**: `access.grant_refused` gains the case of d8, and `request.validation`
      gains `in_progress` with `prefix` and `unreviewed` on a grant that is not a reader grant.
    - **Shapes**:
        - `WorkareaCreate.in_progress`;
        - `Workarea.in_progress` and `in_progress_root_id`;
        - `WorkareaView.reads_unreviewed` and `children[].in_progress`;
        - `RecallRequest.include_unreviewed`;
        - `MemorySummary.reviewed`, required;
        - `GrantRequest.unreviewed` and `Grant.unreviewed`;
        - `workarea.create.in_progress`, `workarea.status_transition.in_progress_released`
          and `grant.issued.unreviewed`;
        - the columns `workareas.in_progress`, `grants.unreviewed`, and
          `memories_fts.in_progress_root_id` and `memory_embeddings.in_progress_root_id`.
    - **The agent plugin**: `oml_recall` gains `include_unreviewed`, the claim item gains
      `reviewed`, and `oml_workarea_info` gains the d11 fields. [mcp-tools.json](../api/mcp-tools.json)
      stays the exact `tools/list` payload with closed schemas: a served tool's entry changes with
      the code that serves it, so until then `_meta.pending_design` names what this record adds
      to `oml_recall`.
    - **The version**: this is a `/v1` change, so `info.version` moves from `0.2.0-design` to
      `0.3.0-design` under the [changelog](../changelog.md)'s rule. The `spec-v0.3` tag waits for
      this record's acceptance.

    The vocabulary of ADR-0018 d4 changes. The "held" row reads "invisible to
    every reader except its author, the reviewer it is served to, and, inside an in-progress
    workarea, the keys its readers' grants allow", and keeps its "never" column, so "unreviewed"
    names the access, the attribute and the parameter, never a state. Two rows are added:

    | Term | Means | Never |
    |---|---|---|
    | in-progress workarea | a `development` workarea created with `in_progress: true`; inside its subtree its readers read held claims, marked, and nothing in it is served at a scope outside until its commit; unrelated to an idempotency record's `in_progress` state | "draft workarea", "staging", "live workarea", a status name |
    | unreviewed access | a key's access, on a node inside an in-progress subtree, to the held claims other keys wrote, resolved from the `unreviewed` attribute of its reader grants, a block winning | "unreviewed" as a state; "permission" for the access itself |

## Consequences

**What a crew gets, and the usage pattern**

- **One crew, one subtree.** A crew on one project works in one in-progress workarea:
    - its agents recall each other's claims at once, ranked with the reviewed ones and marked
      `reviewed: false`;
    - its reviewers work through the held claims at review's pace;
    - an admin and a second identity commit the workarea when nothing is held.
- **A fresh in-progress workarea per project or iteration.** Transitions are forward only and
  the mark is set only at creation, so a committed workarea never becomes in-progress again.
  The next project or iteration gets a new in-progress workarea, typically a sibling with a
  `continues` edge to the last ([workareas](../architecture/workareas.md#edges)).
- **What outside readers see.** Before the commit they see nothing of the subtree but the node
  itself. At the commit they see every promoted claim in it at once, and their tails see one
  `workarea.status_transition`.

**Costs, stated in the same breath**

- **A rejection claws nothing back from the readers inside.** A held claim that agents read,
  acted on or derived from and that is then rejected stays read: the read log records who read
  it. A derived claim declares it in `derives_from`, so review excludes its author from judging
  the derived claim and sees where it came from (R2 over the lineage closure). Undeclared
  derivation stays undetectable (ADR-0001 d6). The `reviewed: false` mark and the plugin skill's
  instruction to weigh such claims rather than trust them are the mitigation. Nothing beyond
  the subtree is exposed.
- **The commit waits on review.** A subtree with one held claim that no eligible reviewer can
  judge cannot commit until that claim is reviewed, dead-lettered and re-enqueued, or erased.
  That is R2's liveness cost (ADR-0001), now paid once per subtree.
- **The commit costs two identities**, as every transition does (ADR-0009 d3).
- **The fence's cost is unmeasured.** Its policy reads the root's status per candidate row,
  the commit's held test runs over the whole subtree, and the indexer embeds held text inside.
  None of these is measured; they are tracked against [FR-4](../product/roadmap.md) and
  [FR-25](../product/roadmap.md), and no number is stated.
- **The indexer reads held text inside the subtree**, so its input grows from promoted text to
  promoted text plus the held text of in-progress subtrees. Embedding rows are already in the
  erasure inventory ([audit and lineage](../architecture/audit-and-lineage.md#the-erasure-inventory)).
- **FR-31 grows by one signal**: the commit refusal of d12.

**Pages this record changes** (decided, not built): [workareas](../architecture/workareas.md),
[roles and keys](../architecture/roles-and-keys.md), [retrieval](../architecture/retrieval.md),
[admission](../architecture/admission.md), [overview](../architecture/overview.md), the
[HTTP API](../api/http-api.md), [openapi.yaml](../api/openapi.yaml), [errors](../api/errors.md),
the [agent plugin](../api/mcp-plugin.md), [mcp-tools.json](../api/mcp-tools.json), the
[Postgres schema](../data/postgres-schema.md), the [event catalog](../data/event-catalog.md), the
[CLI reference](../cli/reference.md), [concepts](../concepts.md),
[commitments](../product/commitments.md), the [roadmap](../product/roadmap.md) (FR-31), the
plugin skill, `llms.txt` and the [changelog](../changelog.md).

**What stays unbuilt** after this record: all of it. The PoC has no in-progress mark, no
`unreviewed` attribute and no `reviewed` field, and its served `oml_recall` entry carries none
of this until the code that serves it lands.

**Process obligations**

- **With the proposal**:
    - the record registers at its three points: this file, a [register](index.md) row and a
      `mkdocs.yml` nav entry;
    - the D8, D9, D11, D12, D13 and D14 cells of the D-register name ADR-0030;
    - `llms.txt` names it;
    - one [changelog](../changelog.md) entry records the `0.3.0-design` bump.
- **On acceptance, and not before**:
    - this record's Status line and frontmatter `status` become `accepted design` with the date,
      and its register row follows;
    - the Status lines of ADR-0001, ADR-0008, ADR-0009, ADR-0013, ADR-0018, ADR-0025 and ADR-0028
      gain "· amended by ADR-0030";
    - a `spec-v0.3` tag is cut.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A later opt-in verb on an existing workarea** | Readers would see held rows their authors wrote when only they could read them, and readers of the parents would lose promoted rows with no per-row journalled act: the visibility switch ADR-0009 rejected. |
| **d1 — A verb that turns the mark off** | A second way to contribute that skips the two-identity commit and its precondition; or, with held rows left, it would take from readers the rows they relied on with no act of review. |
| **d1 — A fourth status, `in_progress`** | The maintainer's decision is a `development` workarea opted in. ADR-0009 d1 keeps three statuses, and its d2 makes a status select review policy, while this mark changes who reads. |
| **d1 — An agent-callable opt-in, as an MCP tool or a writer verb** | Its success would change what other keys can read, which R1 keeps off every agent surface. |
| **d1 — An in-progress vocabulary workarea** | A CURIE could be cited across the tenant before its term is reviewed, and the citation scanned against a term later rejected. |
| **d1 — An in-progress workarea as a simulation whose audience is the crew** | Simulation provenance and taint are indelible and the simulation's end demotes its rows (ADR-0011), so the crew's real work could never contribute. |
| **d2 — Nested in-progress workareas** | Two boundaries on one row, and no single commit that releases it. |
| **d2 — Children outside the boundary** | A crew could contribute claim by claim through a child, against the maintainer's whole-workarea decision. |
| **d2 — No children under an in-progress workarea** | A project could not structure its work in sub-areas, and the subtree rule costs one predicate. |
| **d2 — Descendants move up the ladder before the root** | A `committed` or `validated` descendant would claim a stage while nothing in it has left the subtree. |
| **d2 — Reassignment and re-parenting across the boundary** | Out contributes one claim before the rest; in takes a promoted claim away from its readers without a demotion. |
| **d4 — Self-signing inside an in-progress workarea** | The maintainer keeps R2: review is deferred, never self-signed. |
| **d4 — Both relaxations on one workarea, self-signed rows staying at their own workarea** | A self-signed row would sit inside a subtree whose commit releases everything at once, and readers outside would face two release rules for one subtree. |
| **d5 — Only the crew's own keys read held claims** | The maintainer chose every reader. "The crew's keys" is not a concept the access model has, and building one would be a second access mechanism beside grants (ADR-0008). |
| **d5 — A supervisor view: a permitted key reads held claims anywhere it can read** | It relaxes R3 per key across the whole tree and exposes every crew's held work. |
| **d5 — Ancestor grants stop at the boundary, so only grants on the in-progress workarea or inside it read inside** | The mark would change grant resolution, against ADR-0009 d2 and its rejected per-status grant models, and the maintainer's decision names reader effective access. A crew that needs it uses a per-grant deny or a global denial, or `unreviewed: block` (d7). |
| **d5 — A held successor or retirement takes effect inside at once** | The tip moves only at ratification (ADR-0025 d10), and a rejection would have to restore what readers were served. |
| **d6 — Blocked unless granted** | Every agent key in an in-progress crew would need the attribute set at issuance, and an omitted setting fails silently, as an empty recall. |
| **d7 — A key-level `unreviewed_scope` setting** | A second access mechanism beside grants that every check would have to combine with them. |
| **d7 — An allow on any covering grant wins** | A broader grant added for another purpose would undo a block an issuer set deliberately; deny already wins for the same reason. |
| **d7 — A mutable attribute on a stored grant** | A grant's journalled meaning would change in place; revoking and re-issuing keeps one `grant.issued` per meaning. |
| **d8 — A separate list of held hits in the recall response** | The maintainer chose them mixed in and marked. A second list needs its own order and its own share of the budget (ADR-0025 d4's rejected `own_held` segment). |
| **d8 — Serve only the promoted rows when a blocked key asks for held ones** | The request names rows that exist and that its grants withhold, so a silent narrowing is the coercion errors.md forbids. |
| **d8 — Refuse `include_unreviewed: true` at a scope outside every in-progress subtree** | An agent configured once for its crew would fail every recall it scopes outside, and the answer there is exact: no held claim of another key is readable. |
| **d8 — Held steering in segment 1** | An instruction no one has reviewed, never cut and served ahead of every hit, where steering is by definition promoted (ADR-0018 d4). |
| **d9 — Held claims inside score by full text alone, as own held rows do** | Ratification would raise them in every reader's results, so review would reorder results (R3). |
| **d9 — Separate in-progress index tables, moved into the shared indexes at the commit** | The move is proportional to the subtree inside the chain lock (ADR-0024 d1), or asynchronous, leaving a window where the rows are served nowhere. |
| **d9 — A query predicate on the shared indexes** | Held text would sit in a shared index behind a WHERE clause a future query can forget: the filter discipline ADR-0013 d3 rejects. |
| **d10 — Submission events inside stay author-only** | A tailing agent would learn of a crew mate's claim only by polling recall. |
| **d10 — Show the in-progress period's events at outside tails after the commit** | Two reads of one range would disagree, the visibility that changes over time ADR-0025 d15 rejected. |
| **d10 — Outside reads skip the subtree but keep the reading key's own rows** | Two boundary rules for one subtree; the own-work listing and get already reach every own row. |
| **d10 — A listing flag of its own for held rows** | The listing already filters by `state`. |
| **d11 — A `read_unreviewed` member of `my_effective_verbs`** | That set is the union over effective grant roles (ADR-0025 d14), and unreviewed access turns on the node's mark and a grant attribute. |
| **d12 — Per-claim contribution** | The maintainer chose whole-workarea contribution. |
| **d12 — Commit with held claims left over, as ordinary held claims** | Readers inside would lose rows they relied on, and the contributed subtree would not be fully reviewed. |
| **d12 — Reject the remaining held claims at the commit** | It records verdicts no reviewer gave (ADR-0025 d10). |
| **d12 — The commit moves every node in the subtree to `committed`** | One ratification would move nodes the ratifier may hold no access on, where ADR-0009 d3 binds a transition to effective access on its node. |
| **d12 — A held count in the refusal's details** | It would tell a proposer how many held claims sit in nodes it may not read. |
| **d13 — Reuse `workarea.transition_refused` for the commit precondition** | Its rule is the shape of the ladder. This refusal's remedy is review, so it names its own rule. |
| **d13 — Name the boundary code `workarea.in_progress`** | It reads as the `in_progress` state of `idempotency.in_progress`; `_refused` follows `workarea.transition_refused`. |
| **d13 — Add `include_unreviewed` to the served `oml_recall` entry now** | `tools/list` is read from that file, so the server would advertise a field it refuses. |
