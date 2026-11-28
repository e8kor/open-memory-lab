<!--
id: adr/ADR-0098-a-protected-or-frozen-workspace-can-be-marked-public-and-then-every-reading-key-in-the-tenant-reads-its-approved-items
tier: engineering
status: accepted design · 2026-11-26
answers: how a workspace is shared with every key of its tenant without a grant per key; which keys read a public workspace and through which recall scopes; what a public workspace serves and whether authorship is shown; which workspaces are eligible and whether children inherit the flag; who sets and clears the flag and how it waits; how denials, ranking and federation interact with it; whether it crosses tenants or serves anonymous readers; and the surfaces, events, refusals, doctor lines, tests and release that carry it
-->

# ADR-0098 — A protected or frozen workspace can be marked public, and then every reading key in the tenant reads its approved items

**Status:** accepted design · 2026-11-26, decided by the operator, who answered the record's three
readings the same day. The answer on default recall (d3) differs from the draft's recommendation,
and every rule that rested on that recommendation is re-derived here (marked **re-derived**).
Each answer is marked **decided by the operator, 2026-11-26** where it applies.

**Release:** v0.8.6, built. It builds on [ADR-0097](ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md)'s
admin-act envelope and lands after it in the same release.

**Amends:** [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
(d3: a user key reads a public node outside its root, read only; d10: a user root's recall also
searches every public node),
[ADR-0009](ADR-0009-status-ladder.md) (d5: setting and clearing the flag is an access act a frozen
workspace admits, as a grant is),
[ADR-0097](ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md)
(d3 and d5: one act joins each class list). Each carries an amendment note dated 2026-11-26. It
supersedes nothing.

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
  reads a node: recall, listings, the catalog (ADR-0092), `derives_from` readability
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

- **d3 — Every default recall searches every public node the key reads (decided by the operator,
  2026-11-26; re-derived).** A recall with `sources: "scope"`, the default, searches the served
  tips of subtree(scope) within reader effective access, for a scope inside a user root also the
  `universe` node (ADR-0044 d10), and also the served tips of every public node the key reads and
  no global deny removes, ranked together in the one total order of
  [retrieval](../architecture/retrieval.md#order-of-the-answer).
  It holds for tenant keys and user keys alike, and amends ADR-0044 d10's hit membership for user
  roots. A recall scoped at a public node, or below an ancestor of one, counts that node once.
  What rests on it:
  - **Hits and instructions.** A public node outside the scope's ancestor chain contributes hits
    of the knowledge kinds only. Its steering list does not apply at other scopes, because
    applicable steering is read up the scope's ancestors (ADR-0025 d8), and its skills and
    guardrails rank as hits only where ADR-0092 already admits them: at a scope that is the public
    node itself. A public node's rules therefore never steer a recall elsewhere; they are read as
    data where they are recalled.
  - **The other modes.** `wide` is the scope, public nodes included, plus the candidates ADR-0072
    d4 selects. A `sources` list searches the listed nodes alone, as ADR-0072 d5 says, so a public
    node is in a list's answer only when the list names it.
  - **Listings and the tail.** The listing (`GET /v1/memories` without `own`) and the events tail
    keep their subtree scopes, as ADR-0044 d10 keeps them for `universe`: a key that wants a public
    node's items as a list names it as the workspace.
  - **The session start.** The session-start hook recalls at the default workspace (ADR-0029 d2),
    so its recall carries public hits. The public node's own brief (ADR-0046 d7) is served only at
    a scope that is that node.
  - **`default_workspace`** stays a node the key's grants cover. A node reached only through the
    flag does not qualify: clearing the flag would leave the key with a default it cannot read,
    and every default recall already searches the node.
  - **Explainability.** Each hit from a public node outside the scope carries `via: "public"`
    beside its `workspace_path`, and `search.sources` lists each public node searched, so an
    answer still says where each hit came from (ADR-0072 d8).

  The cost, accepted by the operator: every public node competes for rank and budget with each
  project's own hits in every default recall, the crowding #145 measures for `universe`, now for
  every public node, and no field turns it off for one recall. Marking or clearing a node changes
  every reading key's default answer at its next request (d7, ranking).
  *Rejected:* leaving public nodes out of every default recall, reached only by naming them as
  the scope, by a list or by `wide`, which the draft recommended and the operator did not take:
  under it a user's agent with federation off finds a public node only by knowing its name; a
  per-key opt-in field, a setting no use has named yet (ADR-0039).

- **d4 — A public node serves its approved items only, with their authorship and provenance as
  any reader sees them (decided by the operator, 2026-11-26).** The reach serves the promoted tips
  of every kind a reader is served at that node, within d3's rule for which kinds rank where. It
  never serves a held version: a held row stays its author's alone (ADR-0025 d3), and a public
  node is never in progress, because only a `development` node can be (ADR-0030 d1). The reach
  covers the node alone (d5), so ADR-0080 d16's sharing of an in-progress subtree with its
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
  - **`universe`** is eligible (decided by the operator, 2026-11-26): it is permanently
    `protected`. Marked public, its node becomes readable by tenant keys that hold no grant on it
    and joins their default recall (d3), served under ADR-0044 d9. No exception is written for it.
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

- **d7 — A denial still wins, the ordering contract is unchanged, and federation is not needed
  (re-derived from d3).**
  - **Denials.** A global deny naming the public node or a subtree above it removes the reach, from
    default recall included, as it removes any grant. An admin cuts one key off a public node by
    denying it there, narrowing, at once (ADR-0096 d4). A per-grant deny entry cuts only its own
    grant (effective access, step 1), so it does not remove the public reach, just as it does not
    remove another grant that covers the node. A global deny is the tool for "this key never
    reads it".
  - **Ranking.** Recall's ordering contract is unchanged: steering first and unranked, then one
    ranked list. A public node's rows enter the default recall's statistics as the scope's own
    nodes do (ADR-0072 d7, with R the scope and the public nodes), with no boost and no penalty,
    and the flag enters no score. The cost: R changes when a node is marked or cleared, so the
    scores of every key's default recall change with it, and a tenant's recall eval is re-run
    after a node is marked. A tenant with no public node recalls exactly as before.
  - **Federation.** A public node is searched by default recall whether or not federation is on
    for the tenant, so the flag needs no federation. `accept_federated` keeps governing recalls
    that reach a node through `wide` or a list from a scope outside it; it does not remove a
    public node from default recall, which is the admin's decision through the flag (d6), and
    clearing the flag is the way to remove it. `wide` already contains every public node as part
    of its scope. The tenant's federation policy is untouched.
  *Rejected:* a public node that ignores global denials, which gives an admin no way to cut one
  key off it short of clearing the flag for everyone; a ranking boost for public rows, which
  breaks ADR-0072 d7's rule that a score never depends on how a source was chosen;
  `accept_federated` off removing a public node from default recall, which gives a workspace
  reviewer a veto over an admin act through a setting that grants nothing.

- **d8 — Public means every authenticated key of the one tenant: never another tenant, never an
  unauthenticated reader.** Every read of a public node authenticates a key of the tenant that
  owns it, through the routes that serve any read. No route serves a workspace without a key, the
  console's personal pages included, and no key of another tenant reaches it; the tenant boundary
  holds (ADR-0007). The flag is not an export allowance: export and import stay under ADR-0084,
  unchanged. A tenant with open sign-up (ADR-0068) gives each new person a key, and that key reads
  every public node in every default recall, which is the intent and the cost an admin weighs
  before marking one.
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
    from its grants, so a key's reach stays explainable. A recall hit gains `via: "public"` (d3).
  - **CLI.** `oml workspaces public set <workspace> --reason <text>` prints `effective` or "waits
    on another admin" with the act id; `oml workspaces public clear <workspace> [--reason <text>]`;
    `oml workspaces show` prints the flag.
  - **Console.** The admin workspace page gains a Public toggle with a reason field, the state
    "Waits on another admin" while pending, and the act's history. The set appears in To do's
    "Admin acts to confirm" with ADR-0097 d23's confirm screen, which names the workspace, its
    status, its children that stay private, and that every reading key of the tenant, user keys
    included, will recall its approved items by default. The personal pages, the workspace picker
    and the Memories cockpit label a public node "Public". Copy follows the neutral voice.
  - **MCP.** No mode sets the flag. `oml_workspace` and the catalog serve `public` on each node.
  - **Journal.** The `admin.act` envelope for each act; the effect `workspace.public_set
    {workspace_id, value, act_id, from}` in the commit where the act takes effect, which for a
    pending set is the confirmation's commit. The lake's twin tables replay it (ADR-0090 d18).
  - **Refusals.** `workspace.public_refused` with `details.reason` one of `status`, `user_root`,
    `pending`, `already_public`, `not_public`; `access.verb_ceiling` with `public_is_admin_only`;
    `access.not_found` for a workspace the caller cannot see, as for any route.
  - **Doctor.** `workspace.public` (info: each public node, since when, its act, whether it was
    `sole_admin`, and its count of served rows, the load it adds to every default recall);
    `workspace.public_ineligible` (`critical`: a public node that is `development` or inside a
    user root, which only a direct store edit can produce). A pending set is counted by ADR-0097's
    `admin.act_pending_confirmation`.

- **d10 — It ships in v0.8.6 with one additive migration, after ADR-0097's envelope.** The
  migration adds `public` (false by default) and `public_act_id` to `workspaces`, and a partial
  index on the tenant's public nodes, under the table's row security. Every existing workspace
  starts not public, so the upgrade changes no key's reach and no recall. For
  `universe.ai-agents` the operator's steps are: freeze each child first, then the node, each by
  the ladder's two persons (ADR-0097 d17, or marked under `sole_admin`); then `oml workspaces
  public set universe.ai-agents --reason "<reason>"` for the node and for each child to be shared,
  confirmed by a second admin person where the tenant has one.

## Consequences

- **Docs.** [Workspaces](../architecture/workspaces.md) (effective access, the status table, a
  section on public workspaces, federation's interaction), [lifecycle](../architecture/lifecycle.md)
  (the horizontal bullet), [roles and keys](../architecture/roles-and-keys.md) (the read and the
  admin act) and [retrieval](../architecture/retrieval.md#searching-beyond-the-scope) (default
  recall over public nodes) carry the design as designed, not built. The event catalog
  (`workspace.public_set`), the errors page (`workspace.public_refused`), the HTTP and CLI
  references (generated), and the guides [administering](../guides/administering.md) and
  [workspaces](../use/workspaces.md) change with the build. ADR-0009, ADR-0044 and ADR-0097 carry
  amendment notes dated 2026-11-26.
- **SDKs and counts.** One new route: the routed-operations count and the two SDK route-count
  assertions move by one; OpenAPI, both SDKs, and the HTTP and CLI references are regenerated.
- **The cost, stated once.** Every reading key of the tenant, every future open-sign-up key
  included, recalls every public node by default and reads the principals that wrote there; the
  read cannot be narrowed to a group, except by global denials one key at a time. A tenant that
  wants a group keeps using grants. Each public node competes for rank in every default recall
  (d3), an accepted cost that #145 measures.

### Security analysis

- **No held row crosses.** The reach serves promoted tips only (d4) and covers its node alone
  (d5), and an eligible node is never in progress; the horizontal rule holds by construction.
- **No review authority moves.** The reach is read only (d1); R1 to R4 are untouched, and an agent
  can neither set the flag nor gain any verb from it.
- **No public rule steers elsewhere.** A public node's steering applies only at scopes below it on
  its own chain, and its skills and guardrails rank as hits only at its own scope (d3), so marking
  a node public puts no instruction into another workspace's recall.
- **One admin cannot widen alone,** except under `sole_admin`, where the act is marked and the
  doctor names it (ADR-0097 d10). Clearing is narrowing, so a leaked admin key that clears a flag
  hides nothing; it narrows.
- **Personal roots stay personal.** No node in a user root is eligible (d5).
- **Residuals.** A principal name in a public workspace is read by every key of the tenant (d4).
  An approved row of a public node reaches every reading key's default recall, so a wrong row
  there has the tenant as its reach until it is demoted (ADR-0089) or the flag is cleared, both
  at once; recalled text stays data, served with its findings (ADR-0074).

### Doctor lines

`workspace.public` (info, with each node's served-row count); `workspace.public_ineligible`
(`critical`). Pending sets are counted by ADR-0097's `admin.act_pending_confirmation`.

### Bottleneck register

Rows the build adds to [limits](../reference/limits.md):

- **Effective access** reads the tenant's public node ids once per request, from a partial index
  on `(tenant) where public`; no cache.
- **Default recall** searches every public node a key reads, so its candidate work and its
  statistics grow with the rows of the tenant's public nodes, for every key and every recall. No
  cap bounds the number of public nodes; `workspace.public` reports the rows they add. The cost is
  unmeasured, and a measure issue is filed with the build beside #145.
- **`wide` candidates** are unchanged beyond the scope's growth; ADR-0072 d12's cap and budget
  bound them.

### Tests by sentence

- `a_reader_key_reads_the_approved_items_of_a_public_workspace_it_holds_no_grant_on`
- `a_user_key_reads_a_public_workspace_outside_its_root_and_writes_nothing_there`
- `every_default_recall_searches_every_public_workspace_the_key_reads`
- `a_user_roots_default_recall_searches_universe_and_every_public_workspace`
- `a_hit_from_a_public_workspace_outside_the_scope_carries_via_public`
- `a_public_workspaces_steering_and_guardrails_do_not_apply_at_another_scope`
- `a_sources_list_searches_a_public_workspace_only_when_it_names_it`
- `the_listing_without_a_workspace_does_not_list_public_items`
- `a_public_workspace_cannot_be_a_keys_default_workspace`
- `a_submitter_key_recalls_nothing_from_a_public_workspace`
- `an_admin_key_reads_no_content_of_a_public_workspace`
- `a_public_workspace_never_serves_a_held_version_to_a_key_that_did_not_write_it`
- `a_public_workspace_serves_principal_and_readable_lineage`
- `a_public_universe_serves_its_claims_without_their_source`
- `a_child_of_a_public_workspace_stays_private`
- `a_development_workspace_and_a_user_root_cannot_be_marked_public`
- `a_frozen_workspace_can_be_marked_public_and_cleared`
- `marking_a_workspace_public_waits_on_a_second_admin_person`
- `marking_a_workspace_public_takes_effect_at_once_under_sole_admin_marked`
- `clearing_public_takes_effect_at_once_and_declines_a_pending_set`
- `a_reviewer_key_cannot_set_public`
- `a_global_deny_removes_a_public_workspace_from_default_recall`
- `a_public_workspace_is_recalled_by_default_with_federation_off`
- `accept_federated_off_does_not_remove_a_public_workspace_from_default_recall`
- `a_tenant_without_public_workspaces_recalls_exactly_as_before`
- `a_key_of_another_tenant_never_reads_a_public_workspace`

Each is red first, by breaking the rule it proves.

### The operator's answers of 2026-11-26

1. **Default recall (d3):** every default recall searches every public node, as `universe` is
   searched inside a user root. The draft recommended the opposite; d3, d7 and the bottleneck
   register are re-derived from the answer, and the crowding cost is accepted.
2. **Authorship (d4):** public rows keep their principals and lineage, as any reader sees them,
   as recommended.
3. **`universe` (d5):** eligible, with ADR-0044 d9's serving unchanged, as recommended.

### The operator's answers of 2026-11-27 (decided by the operator)

Decided by the operator on 2026-11-27, answering the questions the v0.8.6 public lane (PR #525)
left; each is the lane's recommendation and is recorded here before the code that takes it.

1. **A user key resolves a bare name to a shared node when that node is public (d2, d9).** A user
   key's bare workspace name resolves inside its own root first (ADR-0044 d5); where the root holds
   no such name, it resolves to the tenant's shared node of that name when that node is public, so
   `oml recall --scope <name>` and every other call that names a workspace reach a public node the
   key reads. A shared node that is not public stays unresolvable to a user key, `access.not_found`,
   as before, so the name of a private shared node is not disclosed.
2. **An admin key that counts as no person is refused (d6).** Setting or clearing the flag needs an
   admin person: a `<user>-admin` session, or an admin key linked to its person. An admin key that
   counts as no person is refused `access.verb_ceiling`, `details.reason: public_is_admin_only`,
   and runs `oml keys link-self` first (ADR-0097 d22).
3. **`public` is an object on `Workspace` everywhere, and a boolean on the counted tree (d9).** The
   workspace read, the tree read and the workspace list serve `public: {value, since, act_id,
   sole_admin}` on each node; the counted tree, a count per node, serves `public: true|false`.

### The operator's answers of 2026-11-28 (decided by the operator)

4. **The private ancestors of a public node (D28; decided by the operator, 2026-11-28).** A
   public node's path, wherever it is served, shows each ancestor that the reading key cannot read
   as the placeholder segment `…` (U+2026), never its name: recall hits' `workspace_path` and
   `search.sources`, the workspace read's `path`, the tree read's `path`, the MCP reads that serve
   them, and the console, which shows the path as served. `universe` is always named. An ancestor
   the key reads, through a grant or because it is public itself, keeps its name. An admin or
   `maintenance` key, which reads the tree's structure by its role, sees every name. The rule
   applies to a public node's path only; a node a key reaches through its own grant is served as
   before. This replaces the disclosure surface-10 found: the name of a private shared node is not
   disclosed by a public node below it, as answer 1 of 2026-11-27 already keeps it from a bare-name
   lookup.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A reader grant on every key** | Needs one more widening act per key, misses every key issued later, and cannot reach user keys, whose grants stay in their root (ADR-0044 d3). |
| **Curating the workspace into `universe`** | Copies go stale, lose their author and source (ADR-0044 d9), and each copy is reviewed again; right for a few facts, wrong for a governed workspace. |
| **A new tenant-wide grant object** | A second grant model for one role; the flag carries the one role a public read can have. |
| **Public `development` workspaces** | Breaks the horizontal rule: development rules are loose, and a development node may sit in an in-progress subtree. |
| **Subtree coverage or inheritance** | A later child, or a ladder move, would widen every key's reach with no admin act. |
| **Public nodes left out of default recall, reached by scope, list or `wide`** | The draft's recommendation, which the operator did not take (2026-11-26): with federation off a user's agent finds a public node only by knowing its name, which defeats "everybody can access". |
| **Public steering and guardrails applied at every scope** | Puts one workspace's instructions into every recall of the tenant through a read flag; instructions apply up the scope's own chain (ADR-0025 d8). |
| **Serving public rows without author and source** | Hides the evidence that makes a governed workspace worth sharing; the operator kept authorship (2026-11-26). |
| **Set by a workspace reviewer, like `accept_federated`** | `accept_federated` grants nothing; `public` grants a read to keys that had none, which is widening, an admin act (ADR-0097 d5). |
| **Anonymous or cross-tenant reads** | Each needs its own record on the boundary it crosses; the operator asked for every key of the tenant. |
