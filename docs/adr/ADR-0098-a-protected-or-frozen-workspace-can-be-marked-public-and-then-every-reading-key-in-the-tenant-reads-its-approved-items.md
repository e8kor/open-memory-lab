<!--
id: adr/ADR-0098-a-protected-or-frozen-workspace-can-be-marked-public-and-then-every-reading-key-in-the-tenant-reads-its-approved-items
tier: engineering
status: proposed
answers: how a workspace is shared with every key of its tenant without a grant per key; which keys read a public workspace and through which recall scopes; what a public workspace serves and whether authorship is shown; which workspaces are eligible and whether children inherit the flag; who sets and clears the flag and how it waits; how denials, ranking and federation interact with it; whether it crosses tenants or serves anonymous readers; and the surfaces, events, refusals, doctor lines, tests and release that carry it
-->

# ADR-0098 — A protected or frozen workspace can be marked public, and then every reading key in the tenant reads its approved items

**Status:** proposed · 2026-11-26

**Release:** v0.8.6, not built. It builds on [ADR-0097](ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md)'s
admin-act envelope and lands after it in the same release.

**Amends:** [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
(d3: a user key reads a public node outside its root, read only),
[ADR-0009](ADR-0009-status-ladder.md) (d5: setting and clearing the flag is an access act a frozen
workspace admits, as a grant is),
[ADR-0097](ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md)
(d3 and d5: one act joins each class list). It supersedes nothing. Each amended record gains an
amendment note when this record is accepted.

**Depends on:** ADR-0097 (d2 envelope, d5 and d3 classes, d10 `sole_admin`, d11 the counted second
admin, d23 pending acts, d26 reasons), [ADR-0072](ADR-0072-recall-searches-the-trusted-workspaces-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md)
(federated recall), [ADR-0093](ADR-0093-contributor-and-submitter-replace-writer.md) (the key roles).

## Context

On 2026-11-26 the operator asked for `universe.ai-agents` to be frozen and given "public status so
everybody can access workspace", and chose a new public flag, recorded in a new ADR, for v0.8.6.

Today a key reads a workspace only through a grant on it or on an ancestor
([effective access](../architecture/workspaces.md#effective-access)). Three ways exist to share a
workspace widely, and each falls short:

- **A grant per key.** Every new key, and every new person's first key, needs another grant, and
  every grant is a widening act that waits on a second admin person (ADR-0097 d5). A tenant with
  many people never stays complete.
- **A user key cannot hold one.** A user key's grants stay inside its own root, plus one reader
  grant on the `universe` node alone (ADR-0044 d3). No grant can give a person's own keys a shared
  workspace, so the people the operator means by "everybody" are the keys that cannot be reached.
- **Curation into `universe`.** A curator copies a claim into `universe` and a second identity
  reviews the copy (ADR-0044 d7). The copy loses its author and its source (ADR-0044 d9) and goes
  stale when the original changes. That is right for a few curated facts and wrong for a whole
  governed workspace.

Federation does not help either: it searches only what a key already reads and never widens it
(ADR-0072 d6).

The decision is held against the lifecycle's horizontal rule: siblings, other branches, federation
and wide sources read promoted rows of protected or frozen workspaces, and held work never crosses
([lifecycle](../architecture/lifecycle.md#sharing-vertical-and-horizontal)). It is also held
against the constitution: the flag changes who reads, never who reviews, and it never serves a row
that review has not passed.

Decided, not built.

## Decision

- **d1 — One flag, `public`, on a workspace node, off by default; set, it adds that node to the
  effective read access of every reading key of the tenant.** Effective access gains one input.
  It becomes the union of the key's grants, each reduced by its own deny entries, together with
  the tenant's public nodes when the key's role reads, minus the key's global deny. The public
  reach behaves as a `reader` grant with `covers: node` that the key does not hold: no grant row
  is written, nothing is issued, and the reach is computed on every request and cached nowhere, as
  effective access already is. A public node counts as read for every rule that asks whether a key
  reads a node: recall scope, listings, the catalog (ADR-0092), `derives_from` readability
  (ADR-0025 d3), and an edge endpoint (ADR-0046 d5). It counts for no rule that asks for another
  role. It never gives submit, review, curation, ratification, a status or brief proposal, or
  `create`. The cost is one more input to effective access, read on every request (Bottleneck
  register).
  *Rejected:* writing a reader grant on every key, which is the per-key gap the context describes
  and makes every issuance carry one more widening act; a tenant-wide grant object with its own
  role, which is a second grant model where one flag carries the one role it can have.

- **d2 — Who reads: every key of the tenant whose role reads, user keys and session keys
  included; a submitter key still recalls nothing.** The roles that read are `reader`,
  `contributor` and `reviewer` (ADR-0093 d2's table), on tenant keys and user keys alike. A
  session key reads by its role. A user key reads a public node outside its own root, read only.
  This amends ADR-0044 d3's "no user key can ever read, write, review or be served anything ... in a
  shared workspace below `universe`" for public nodes and for reading alone. A submitter key reads
  only its own rows, as today (ADR-0093 d2, "no recall means no recall of anything else"). An admin
  key reads no claim content (ADR-0044 d9), and the flag does not change that. A `maintenance` key
  never reads (ADR-0097 d18). A key that is pending, revoked, cut off or expired authenticates
  nothing and so reads nothing (ADR-0097 d23, d14).
  *Rejected:* tenant keys only, which leaves out the people the operator named; every key, a
  submitter included, which reverses ADR-0093's write-only shape for keys issued to write blind.

- **d3 — A public node is not in any key's default recall scope: a key reaches it by naming it
  as the scope, by a `sources` list, or by `sources: "wide"` (reading, for the operator to
  confirm).** A recall still searches its scope's subtree, and for a scope inside a user root also
  the `universe` node (ADR-0044 d10), and nothing else unless `sources` says so. The public reach
  makes the node readable, so:
  - **Scope.** `workspace: universe.ai-agents` on recall, on a listing or on `oml read` searches
    that node, with the steering and guardrails of its ancestors that the key reads (ADR-0025 d8).
  - **A list.** `sources: ["universe.ai-agents"]` searches it as a listed source, under ADR-0072
    d5's rules, federation on the tenant included.
  - **Wide.** `sources: "wide"` may select it as a candidate, and the widen hint (ADR-0072 d8) can
    point at it when it holds more of the query's words. Both need federation on the tenant and
    `accept_federated` on the node (d7).

  A key's `default_workspace` stays a node its grants cover; a node reached only through the flag
  does not qualify, because clearing the flag would leave the key with a default it cannot read.
  The session-start brief recalls at the default workspace (ADR-0029 d2), so a public node's brief
  reaches a key only through a scope or a source the key names.
  The cost: with federation off on the tenant, a user's agent finds a public node only by its
  name, so the workspace's name has to be known, from the workspace list, the catalog or a
  directory entry (d9).
  *Rejected:* searching every public node in every default recall, as ADR-0044 d10 does for
  `universe`, which makes every public node compete for rank and budget with each project's own
  hits (the crowding #145 measures for `universe` alone) and changes every key's recall at once;
  a per-key opt-in field, a setting no use has named yet (ADR-0039).

- **d4 — A public node serves its approved items only, with their authorship and provenance as
  any reader sees them.** The reach serves the promoted tips of every kind a reader is served at
  that node: knowledge, terms, skills, guardrails, documents and steering, each as data under
  ADR-0092. It never serves a held version: a held row stays its author's alone (ADR-0025 d3), and
  a public node is never in progress, because only a `development` node can be (ADR-0030 d1). The
  reach covers the node alone (d5), so ADR-0080 d16's sharing of an in-progress subtree with its
  ancestors never passes through it. Retired versions are served as to any reader; an admin fix's
  marked overlay is served marked, as ADR-0089 serves it to any reader; release bindings, once
  built, apply to the public reach as to any grant (ADR-0085). Each row keeps its `principal`, its
  `promoted_at` and its lineage fields under ADR-0025 d3's readability rule, so `derives_from`
  names only versions the key reads. A public `universe` node keeps ADR-0044 d9's serving: origin
  `universe`, no author, no lineage below it.
  The cost: every key of the tenant reads the principals that wrote in a public workspace. A
  workspace whose authors must stay unnamed is curated into `universe` instead (ADR-0044 d7).
  *Rejected:* serving public rows without author and source, as curated `universe` claims are,
  which hides the evidence that makes a governed workspace worth sharing and gives two serving
  rules for one workspace depending on the reader's route; serving held rows marked unreviewed,
  which breaks the horizontal rule.

- **d5 — Only a `protected` or `frozen` node outside every user root is eligible; the flag covers
  the node alone and is not inherited.**
  - **Status.** A `development` node is refused (`workspace.public_refused`, `details.reason:
    status`), and so every in-progress node is. The ladder only moves forward (ADR-0009 d3), so an
    eligible node never becomes ineligible, and no transition clears the flag: freezing a public
    `protected` node keeps it public.
  - **User roots.** A user root and every node under it are refused (`details.reason:
    user_root`). They are capped at `development` (ADR-0085 d17), so the status rule already
    refuses them; the named reason says why the cap holds. A person's own work never becomes
    public through this flag.
  - **`universe`** is eligible: it is permanently `protected`. Marked public, its node becomes
    readable by tenant keys that hold no grant on it, served under ADR-0044 d9.
  - **Frozen.** Setting and clearing the flag on a `frozen` node are admitted. ADR-0009 d5 freezes
    claim content and promotions; the flag changes who reads, which is an access act, unrelated to
    status, as ADR-0009's consequences already say of a grant on a frozen area. The table of what
    each status refuses gains the row "Set or clear `public`: refused, allowed, allowed". This
    amends ADR-0009 d5 by naming the flag beside its three mutations.
  - **Children.** The flag covers its node alone. A child of a public node is not public: not one
    that exists, and not one created later under a public `protected` parent. Each child that
    should be public is marked on its own, by its own act. A frozen public node has only frozen
    children (ADR-0009 d4), so marking a whole frozen subtree is one act per node.
  *Rejected:* covering the subtree, under which a new `development` child of a public `protected`
  node becomes readable to every key with no admin act and with held work nearby; inheriting the
  flag down the tree, which has the same effect when a child climbs the ladder, a two-person move
  that is not an admin act and confirms no widening; eligibility for `development` nodes, which
  the horizontal rule excludes.

- **d6 — Setting the flag is an admin act of class widening; clearing it is narrowing.** Both
  journal ADR-0097 d2's `admin.act` envelope, the class fixed in code.
  - **Set**, act `workspace_public_set`, class `widening`, joins ADR-0097 d5's list. It takes
    effect only when a second admin person who counts (ADR-0097 d11) confirms it. Under
    `sole_admin` it takes effect at once, marked `sole_admin` on the act and on the workspace read
    (ADR-0097 d10). A reason is required (ADR-0097 d26). While pending it has no effect, and when
    declined or expired it is final (ADR-0097 d23). A workspace holds one pending set at a time; a
    second is refused `workspace.public_refused`, `details.reason: pending`, naming the live
    `act_id`.
  - **Clear**, act `workspace_public_clear`, class `narrowing`, joins ADR-0097 d3's list. It takes
    effect at once, never reverts by itself, and is listed in the admin's To do as done. A clear
    while a set is pending declines that set. Its reason follows ADR-0097 d26's narrowing rule.
  - **Who may.** An admin person: a linked admin key or `<user>-admin`. Every other key, a person's
    reviewer key on the workspace included, is refused `access.verb_ceiling`, `details.reason:
    public_is_admin_only`. No MCP mode sets or clears it; settings stay with people (ADR-0095 d7).
  *Rejected:* a person-held reviewer key on the workspace sets it, as `accept_federated` is set
  (ADR-0072 d5), which lets one workspace reviewer give every key of the tenant a read it did not
  have, where `accept_federated` grants nothing; the workspace's reviewer person also confirms, as
  ADR-0097 d5 asks for an in-progress subtree, which adds a third party to a shared workspace that
  belongs to no person; setting at once with a later review, which ADR-0097 d1 rejects for every
  widening.

- **d7 — A denial still wins, ranking is unchanged, and federation's own settings still apply.**
  - **Denials.** A global deny naming the public node or a subtree above it removes the reach, as
    it removes any grant. An admin cuts one key off a public node by denying it there, narrowing,
    at once (ADR-0096 d4). A per-grant deny entry cuts only its own grant (effective access, step
    1), so it does not remove the public reach, just as it does not remove another grant that
    covers the node. A global deny is the tool for "this key never reads it".
  - **Ranking.** Recall's ordering contract is unchanged. A public node's rows score with the same
    statistics as any searched node (ADR-0072 d7), with no boost and no penalty, and the flag
    enters no score.
  - **Federation.** The flag makes a node readable; federation decides whether a recall scoped
    elsewhere searches it. With federation off on the tenant, `wide` and lists are refused, as
    today, and a key reaches the node by naming it as the scope. With federation on, the node is a
    trusted source like any `protected` or `frozen` node the key reads (ADR-0072 d1), and
    `accept_federated` off keeps it out of `wide` and out of lists from scopes outside it, while a
    key still reads it as the scope. The tenant's federation policy is untouched.
  *Rejected:* a public node that ignores global denials, which gives an admin no way to cut one
  key off it short of clearing the flag for everyone; a ranking boost for public rows, which
  breaks ADR-0072 d7's rule that a score never depends on how a source was chosen.

- **d8 — Public means every authenticated key of the one tenant: never another tenant, never an
  unauthenticated reader.** Every read of a public node authenticates a key of the tenant that
  owns it, through the routes that serve any read. No route serves a workspace without a key, the
  console's personal pages included, and no key of another tenant reaches it; the tenant boundary
  holds (ADR-0007). The flag is not an export allowance: export and import stay under ADR-0084,
  unchanged. A tenant with open sign-up (ADR-0068) gives each new person a key, and that key reads
  every public node, which is the intent and the cost an admin weighs before marking one.
  *Rejected:* an anonymous read route, which needs its own record on abuse, rate limits per caller
  without a key, and the site boundary; a read across tenants, which needs a record on the tenant
  boundary and is not what the operator asked for.

- **d9 — The surfaces.**
  - **HTTP.** `POST /v2/workspaces/{workspace}/public {public, reason}`, operation `setWorkspacePublic`.
    A clear, and a set under `sole_admin`, answer `200 {workspace, public, act_id, state:
    "effective"}`; a set waiting on a second admin answers `202 {workspace, act_id, state:
    "pending_confirmation"}`. ADR-0097's `confirmAdminAct` and `declineAdminAct` settle it. The
    workspace read serves `public: {value, since, act_id, sole_admin}` and, to an admin, the
    pending `act_id`. The tree read and the workspace list mark public nodes. The one-key read
    (ADR-0096 d8) serves `public_reach`, the public nodes the key reads through the flag, apart
    from its grants, so a key's reach stays explainable.
  - **CLI.** `oml workspaces public set <workspace> --reason <text>` prints `effective` or "waits
    on another admin" with the act id; `oml workspaces public clear <workspace> [--reason <text>]`;
    `oml workspaces show` prints the flag.
  - **Console.** The admin workspace page gains a Public toggle with a reason field, the state
    "Waits on another admin" while pending, and the act's history. The set appears in To do's
    "Admin acts to confirm" with ADR-0097 d23's confirm screen, which names the workspace, its
    status, its children that stay private, and that every reading key of the tenant, user keys
    included, will read its approved items. The personal pages, the workspace picker and the
    Memories cockpit label a public node "Public". Copy follows the neutral voice.
  - **MCP.** No mode sets the flag. `oml_workspace` and the catalog serve `public` on each node, so
    an agent can name a public node as its scope or source.
  - **Journal.** The `admin.act` envelope for each act; the effect `workspace.public_set
    {workspace_id, value, act_id, from}` in the commit where the act takes effect, which for a
    pending set is the confirmation's commit. The lake's twin tables replay it (ADR-0090 d18).
  - **Refusals.** `workspace.public_refused` with `details.reason` one of `status`, `user_root`,
    `pending`, `already_public`, `not_public`; `access.verb_ceiling` with `public_is_admin_only`;
    `access.not_found` for a workspace the caller cannot see, as for any route.
  - **Doctor.** `workspace.public` (info: each public node, since when, its act and whether it was
    `sole_admin`); `workspace.public_ineligible` (`critical`: a public node that is `development`
    or inside a user root, which only a direct store edit can produce). A pending set is counted
    by ADR-0097's `admin.act_pending_confirmation`.
  - **A directory entry.** A public node needs no directory entry to be read, but a tenant that
    wants its people to find it writes one, as [a directory workspace](../architecture/workspaces.md#a-directory-workspace)
    describes.

- **d10 — It ships in v0.8.6 with one additive migration, after ADR-0097's envelope.** The
  migration adds `public` (false by default) and `public_act_id` to `workspaces`, and a partial
  index on the tenant's public nodes, under the table's row security. Every existing workspace
  starts not public, so the upgrade changes no key's reach. For `universe.ai-agents` the operator's
  steps are: freeze each child first, then the node, each by the ladder's two persons (ADR-0097
  d17, or marked under `sole_admin`); then `oml workspaces public set universe.ai-agents --reason
  "<reason>"` for the node and for each child to be shared, confirmed by a second admin person
  where the tenant has one.

## Consequences

- **Docs on acceptance.** [Workspaces](../architecture/workspaces.md): effective access gains the
  public input, the status table its row, a new section on public workspaces, and federation's
  section the interaction of d7. [Lifecycle](../architecture/lifecycle.md): the horizontal bullet
  names public nodes. [Roles and keys](../architecture/roles-and-keys.md): the read column and
  the admin act. [Retrieval](../architecture/retrieval.md#searching-beyond-the-scope): reaching a
  public node by scope, list or `wide`. The event catalog (`workspace.public_set`), the errors
  page (`workspace.public_refused`), the HTTP and CLI references (generated), the guides
  [administering](../guides/administering.md) and [workspaces](../use/workspaces.md), and the
  design records. ADR-0009, ADR-0044 and ADR-0097 gain amendment notes.
- **SDKs and counts.** One new route: the routed-operations count and the two SDK route-count
  assertions move by one; OpenAPI, both SDKs, and the HTTP and CLI references are regenerated.
- **The cost, stated once.** Every reading key of the tenant, every future open-sign-up key
  included, reads every public node and the principals that wrote there; the read cannot be
  narrowed to a group, except by global denials one key at a time. A tenant that wants a group
  keeps using grants.

### Security analysis

- **No held row crosses.** The reach serves promoted tips only (d4) and covers its node alone
  (d5), and an eligible node is never in progress; the horizontal rule holds by construction.
- **No review authority moves.** The reach is read only (d1); R1 to R4 are untouched, and an agent
  can neither set the flag nor gain any verb from it.
- **One admin cannot widen alone,** except under `sole_admin`, where the act is marked and the
  doctor names it (ADR-0097 d10). Clearing is narrowing, so a leaked admin key that clears a flag
  hides nothing; it narrows.
- **Personal roots stay personal.** No node in a user root is eligible (d5).
- **Residual.** A principal name in a public workspace is read by every key of the tenant (d4).
  The tenant's admins decide that by marking the node.

### Doctor lines

`workspace.public` (info); `workspace.public_ineligible` (`critical`). Pending sets are counted by
ADR-0097's `admin.act_pending_confirmation`.

### Bottleneck register

Rows the build adds to [limits](../reference/limits.md):

- **Effective access** reads the tenant's public node ids once per request, from a partial index
  on `(tenant) where public`; no cache. The size of a tenant's public set is unmeasured, and a
  measure issue is filed with the build.
- **`wide` candidates** grow by the public nodes a key reads; ADR-0072 d12's cap and budget bound
  them, unchanged.

### Tests by sentence

- `a_reader_key_reads_the_approved_items_of_a_public_workspace_it_holds_no_grant_on`
- `a_user_key_reads_a_public_workspace_outside_its_root_and_writes_nothing_there`
- `a_submitter_key_recalls_nothing_from_a_public_workspace`
- `an_admin_key_reads_no_content_of_a_public_workspace`
- `a_public_workspace_never_serves_a_held_version_to_a_key_that_did_not_write_it`
- `a_public_workspace_is_not_in_a_user_keys_default_recall`
- `a_public_workspace_is_searched_when_named_as_scope_or_listed_in_sources`
- `a_child_of_a_public_workspace_stays_private`
- `a_development_workspace_and_a_user_root_cannot_be_marked_public`
- `a_frozen_workspace_can_be_marked_public_and_cleared`
- `marking_a_workspace_public_waits_on_a_second_admin_person`
- `marking_a_workspace_public_takes_effect_at_once_under_sole_admin_marked`
- `clearing_public_takes_effect_at_once_and_declines_a_pending_set`
- `a_reviewer_key_cannot_set_public`
- `a_global_deny_removes_a_public_workspace_from_a_keys_reach`
- `a_public_workspaces_rows_score_the_same_as_under_a_grant`
- `accept_federated_off_keeps_a_public_workspace_out_of_wide_recall`
- `a_key_of_another_tenant_never_reads_a_public_workspace`

Each is red first, by breaking the rule it proves.

### Questions for the operator

1. **Default recall (d3).** A public node is reached by scope, by a list, or by `wide`, and is not
   in any default recall. Recommended: keep it so, and turn federation on for the tenant so the
   widen hint points agents at it. The alternative is ADR-0044 d10's treatment of `universe` for
   every public node.
2. **Authorship (d4).** Public rows keep their principals and lineage, as any reader sees them.
   Recommended: keep it so. The alternative serves them like curated `universe` claims, with no
   author and no source.
3. **`universe` itself (d5).** It is eligible. Recommended: eligible, with ADR-0044 d9's serving
   unchanged, so no exception is written for it.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A reader grant on every key** | Needs one more widening act per key, misses every key issued later, and cannot reach user keys, whose grants stay in their root (ADR-0044 d3). |
| **Curating the workspace into `universe`** | Copies go stale, lose their author and source (ADR-0044 d9), and each copy is reviewed again; right for a few facts, wrong for a governed workspace. |
| **A new tenant-wide grant object** | A second grant model for one role; the flag carries the one role a public read can have. |
| **Public `development` workspaces** | Breaks the horizontal rule: development rules are loose, and a development node may sit in an in-progress subtree. |
| **Subtree coverage or inheritance** | A later child, or a ladder move, would widen every key's reach with no admin act. |
| **Default recall of every public node** | Every public node competes with each project's own hits, and every key's recall changes at once (#145 measures the effect for `universe` alone). |
| **Set by a workspace reviewer, like `accept_federated`** | `accept_federated` grants nothing; `public` grants a read to keys that had none, which is widening, an admin act (ADR-0097 d5). |
| **Anonymous or cross-tenant reads** | Each needs its own record on the boundary it crosses; the operator asked for every key of the tenant. |
