<!--
id: adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source
tier: engineering
status: accepted design · 2026-11-04 · an amendment of 2026-11-12 (d16) is proposed
answers: what a user is and where their root workarea sits; how a key is tied to its person and when the person's root is created; how a user key addresses, reads and creates workareas; which grants a user key may carry and how a universe read stops short of other users' roots; what a node grant is; who places claims in universe and how curation counts toward review under R1 and R2; whether the reviewer exclusion covers the person behind a key; what a reader of a universe claim is served and what stays in the journal; how recall, steering and guardrails reach universe from a user's scope; how names stay unique when two users both want the same one; what crews and existing tenants do; which fields, flags and doctor lines change; who may start a memory in a user root (the proposed amendment of 2026-11-12); what it amends in ADR-0001, ADR-0008, ADR-0010, ADR-0018, ADR-0025, ADR-0026, ADR-0028 and ADR-0039
-->

# ADR-0044 — Every user works under a root of their own beneath `universe`, and `universe` serves only curated claims, without their source

**Status:** accepted design · 2026-11-04 · amended by [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md), [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md) · an amendment of 2026-11-12 is **proposed** and awaits the operator's acceptance ([the amendment](#the-amendment-of-2026-11-12-a-memory-is-started-in-a-user-root-by-its-own-users-keys-only), d16; #329)

**Release:** introduced in v0.5.0 ([releases](../changelog.md))

**Amends:** ADR-0008 (d1, d3, d4), ADR-0010 (d1), ADR-0018 (d4), ADR-0025 (d2, d3, d8), ADR-0026 (d1, d4, d5, d7), ADR-0028 (d1), ADR-0039 (d15)

## Context

**Today a tenant has one tree and one flat namespace, and nothing in it belongs to a person.**
Every workarea of a tenant is a node under `universe`, created by an admin
([ADR-0008](ADR-0008-workarea-algebra.md) d1; `create_workarea` in `crates/oml-app/src/app.rs`
requires the admin role). Its name is unique across the whole tree
([ADR-0026](ADR-0026-names-are-keys.md) d1; the `workareas_tenant_id_name` constraint in
`migrations/0006_names.sql`), so two people who both want a workarea called `project` cannot
both have one. A key carries a `principal`, a `held_by` attestation (`agent`, `human` or
`service`, `migrations/0001_init.sql`) and optionally a crew, and nothing that says which
person it acts for: `oml keys issue-agent` takes `--principal crew/claude-mobile` and a
`--workarea` the admin names ([CLI](../cli/reference.md#oml-keys-issuance-revocation-rotation)).

**A grant always covers a whole subtree.** `effective_roles` admits a grant on N or on any
ancestor of N (`crates/oml-domain/src/access.rs`; [workareas](../architecture/workareas.md#effective-access-union-of-attenuated-grants-then-deny-wins)).
A reader grant on `universe` therefore reads every workarea of the tenant. A key that should read
what the tenant shares, and nothing of anyone else's, has no grant shape that says so: denying
each other person's subtree inside the grant would have to be amended every time a person is
added.

**`universe` holds nothing.** The design says the root refuses direct claim writes
(ADR-0008 d1; `submission.universe_refused` in [errors](../api/errors.md)). As built, no code
path answers that code: no grep of `crates/` finds it, and a key with a writer grant on
`universe` could write there. The refusal is decided, not built.

**The live tenant.** The maintainer's deployment holds one tenant, `oml`, whose agent keys work
in workareas such as `universe.crew.oml` under principals such as `crew/claude-mobile`
([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md); the issue-agent example in
[the MCP plugin page](../api/mcp-plugin.md)). Nothing there names a user.

**The maintainer's decisions of 2026-11-04,** which this record designs around and does not
reopen:

1. Every user has a root workarea of their own, a child of the tenant's `universe`, named after
   the user: `universe.<user>`. There is still exactly one tree per tenant. A user's keys address
   workareas relative to that root, so a key of alice naming `project` means
   `universe.alice.project`, and a workarea created with such a key is created under
   `universe.alice`.
2. A user is a human principal. Their root is created when the first key naming that person is
   issued, by admin issuance (`oml keys issue`, `oml keys issue-agent`, connect-code keys), and
   agent keys issued for a person live under that person's root.
3. The shared `universe` is read-only for users and curated by admins only: users neither write
   nor propose to it. Every user key reads universe's own claims and never another user's root.
4. A user who reads a claim that lives in `universe` sees its origin as `universe` and nothing
   else: no path below it, no author, no lineage into another user's root, no curator. The
   journal keeps full attribution, and an admin's audit still sees everything.

The constitution holds over all of it: no agent-callable verb promotes or signs (R1), the
reviewer is never the author over the lineage closure (R2), the gate changes visibility and not
ranking (R3), and nothing is deleted silently (R4) ([ADR-0001](ADR-0001-the-constitution.md)).
ADR-0037, proposed on [#20](https://github.com/e8kor/open-memory-lab/pull/20) and not on this
branch, serves a key's guardrails from its scope node and, under `guardrail_scope: ancestors`,
from every ancestor its reader access covers; d10 below states how that reaches `universe`.

Decided; introduced in v0.5.0.

## Decision

- **d1 — A user root is a child of `universe` named after its user, marked as a user root when
  it is created, and the tree stays one tree.** A user root is an ordinary workarea with one
  extra fact, `user_root: true`, fixed at creation and journalled in its `workarea.create`
  payload. It is always a direct child of `universe`, starts in `development` like every new
  workarea, and climbs the [status ladder](../architecture/workareas.md#the-status-ladder-d8)
  like any other. It is never re-parented and nothing is re-parented into or out of it across
  its boundary (d5 needs that). The server creates it, and nothing else does: no verb creates a
  user root directly (d2). Everything beneath a user root belongs to that user's subtree; every
  workarea outside every user root, `universe` included, is **shared**. One tree per tenant
  stays exactly as ADR-0008 d1 and the [concepts](../concepts.md#workarea) page state it, so
  paths, subtree grants and the child-status rule keep one answer each. The cost is one more
  column on `workareas` and one more payload field on `workarea.create`.

- **d2 — A key names its person in a new issuance field, `user`, and the first key naming a user
  creates that user's root.** `POST /v2/keys` takes `user`, a name under the
  [creation grammar](../api/http-api.md#names-and-ids). It is given explicitly and never derived
  from `principal`, `name` or `label`, which stay independent as ADR-0026 d9 has them: a
  principal may carry `/` and need not be a name, and a person's two keys under the two-key rule
  share a principal but serve different capacities. A key carrying `user` is a **user key**; a
  key without it is a **tenant key**, as every key is today.
    - `user` is admitted on `reader`, `writer` and `dreamer` keys, with `held_by` `agent` or
      `human`, connector keys included. It is refused, `request.validation` with
      `details.field: user`, on `admin` and `operator` keys, whose authority is tenant-wide or
      provider-wide and sits in no one's root, and on `held_by: service` keys, which act for a
      process and not a person.
    - When no workarea of the shared namespace (d5) holds the name `user` names, issuance
      creates the user root in its own commit, before `key.issued`, journalled as
      `workarea.create` with `user_root: true` and the issuing admin as actor. When the name is held by a user root, the key joins that
      user. When it is held by a shared workarea, issuance is refused with `name.taken`, kind
      `workarea`, before anything is journalled. A user's name is therefore its root's name,
      and no separate user record exists: the root is the user.
    - An issuance that fails after the root is created leaves the root, empty, and journals
      nothing more; the next issuance naming that user reuses it. Nothing is deleted (R4).
    - `user` is journalled in `key.issued` as the root's id, fixed for the key's life and
      carried to a rotation's successor, as `crew` is (ADR-0026 d9).
    - "A user is a human principal" is held the way `held_by: human` is held: the issuing
      admin attests it and the server verifies the attestation, never the person. An agent key
      issued for alice is a user key with `user: alice` and `held_by: agent`.

    The cost is one field the admin must state on every key meant for a person; a key issued
    without it lands outside any user root, which `oml doctor` names (d14).

- **d3 — A user key's grants stay inside its own root, and issuance adds one reader grant on the
  `universe` node alone.** Every grant a user key carries names a node inside its user's root,
  the root included, under the ordinary [admissibility](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)
  table; a grant naming any other node is refused at
  issuance and at every later grant act with `access.issuer_ceiling`, details
  `{role, admissible, grant_index, user}`, where `admissible` lists the roles the key's role
  admits and `user` names the root the grant must lie in. Issuance then adds exactly one grant
  the request did not send: `reader` on `universe` with `covers: node` (d4), journalled as its
  own `grant.issued` and echoed in the `201` like every grant. This amends ADR-0025 d2's "the
  server takes exactly the grants it is sent" for user keys alone. Because a node grant on
  `universe` covers `universe` and nothing beneath it, and every other grant lies inside the
  user's root, no user key can ever read, write, review or be served anything in another
  user's root or in a shared workarea below `universe`: crews included (d13). A tenant key is
  unchanged: an admin may grant it anything, a user's root included, which is how reviewers and
  curators reach users' work (d7). The cost is that a person who also works in a crew holds a
  tenant key for that crew beside their user keys, and the journal shows which capacity acted,
  as the two-key rule already does for admin work
  ([ADR-0010](ADR-0010-roles-and-keys.md) d6).

- **d4 — A grant covers its node's subtree by default, or its node alone with `covers: node`.**
  A grant gains one attribute, `covers`, `subtree` (the default, every grant today) or `node`.
  The coverage test of `effective_roles` becomes: a `subtree` grant covers N when its node is N
  or an ancestor of N, as now; a `node` grant covers N only when its node is N. A `node` grant
  carries no deny entries, since nothing beneath it is covered to mask; one sent with any is
  `request.validation`, `details.field: /grants/<i>/deny`. Global denials apply to it as to every
  grant, and the permission epoch, the union and deny-wins are untouched. This amends ADR-0008 d3
  ("covers that node's subtree") and d4 (the formula's first term). The CLI spells it
  `--grant <workarea>:<role>:node`. A node grant is open to every key, not only user keys, so an
  admin can grant a tenant key `universe`'s own claims without the tenant. The cost is one more
  attribute that every coverage test, the effective-access listing and the evidence export carry;
  it keeps one formula for every access check, which ADR-0008's consequences rely on.

- **d5 — Workarea names are unique in the shared namespace or within one user's root, a
  qualified name `<user>.<name>` is unique in the tenant, and a user key's names resolve in its
  own root.** This amends ADR-0026 d1's "unique in its tenant, the whole tree":
    - **Two kinds of namespace.** The **shared namespace** holds `universe`, every user root's
      name and every shared workarea's name. Each **user namespace** holds the names of the
      workareas strictly beneath that user's root. A name is unique within its namespace, and
      within a user namespace `universe` and the user's own name are reserved, `name.taken`, so
      a bare name never means two things to one key. Crew, key and tenant namespaces are
      unchanged.
    - **The qualified name.** A workarea inside a user root has the qualified name
      `<user>.<name>` — `alice.project` — which is unique in the tenant and, since d1 never moves
      a node across a root's boundary and no name changes, never changes. A shared workarea's
      qualified name is its name. This is not the derived path: the path of
      `universe.alice.experiments.run-grid` changes if `run-grid` is re-parented within alice's
      root, and its qualified name `alice.run-grid` does not. The tenant-unique key is therefore
      the qualified name, not the full path, so that d3's grant on `alice.run-grid` stays the
      same reference after a move.
    - **Where it is accepted.** Every field that takes a workarea id or name also takes a
      qualified name: exactly two labels joined by one `.`, each under the reference grammar,
      the first naming a user root. A value whose first label is `universe`, or that has more
      than two labels, is still a path and still `request.validation`, as ADR-0026 d7 refuses
      every path; `GET /v2/workareas?path=` stays the one lookup by path. This amends ADR-0026 d7
      by that one form.
    - **Resolution.** A user key resolves a bare name in its own user namespace, plus `universe`
      and its own root's name; a tenant key, admin keys included, resolves a bare name in the
      shared namespace. Every tenant key resolves a qualified name. A user key that names
      `bob.project` resolves it like any name and is answered `access.not_found`, as for an id it
      cannot see, so resolution still confirms nothing (ADR-0026 d4, amended for the namespace
      only).
    - **`name.taken` reaches user keys, inside their root only.** A user key creating a
      workarea (d6) is told `name.taken` for a name in its own user namespace. That discloses
      names in a subtree the key's own user owns, and nothing of the shared namespace or of other
      users. This amends ADR-0026 d5's "No agent key therefore ever learns that a name is
      taken".

    The cost: an admin addresses a user's workarea by its qualified name or id, not by its bare
    name, and the schema's one unique constraint becomes two, a shared one and one keyed by the
    user root.

- **d6 — A user key creates workareas inside its own root, through the existing create verb.**
  `POST /v2/workareas` admits a writer user key whose effective access confers `writer` on the
  parent; the body's `parent` defaults to the user's root when omitted, and under alice's key
  `oml workareas create alice project` creates `universe.alice.project`. The
  refusal order is the ordinary one on the parent: `access.grant_refused` with
  `needed_role: writer` where the key reads the parent, `access.not_found` otherwise, so a user
  key can never create outside its root, where d3 gives it no writer grant. The create is
  journalled as `workarea.create` with the user key as actor. A tenant writer key keeps no
  create verb, and admins keep theirs. No MCP tool gains it: `oml_workarea` stays read-only,
  and an agent asks its user, or runs the CLI under its key, to create a workarea
  ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8). This amends
  [ADR-0010](ADR-0010-roles-and-keys.md) d1 (the writer role's verbs) and ADR-0008's
  "created by a journalled admin act". The cost is that a writer can now create structure, in
  its own root and nowhere else; the governed acts on that structure — status transitions,
  edges, re-parenting, crew metadata — stay where ADR-0008 and ADR-0009 put them.

- **d7 — `universe` takes claims only from curators an admin designates, through an ordinary
  held write that a second identity reviews; curation adds no verb and no promotion path.**
  ADR-0008 d1's "refuses direct claim writes" becomes: `universe` admits a claim write from a key
  whose effective access confers `writer` on it, and d3 guarantees no user key ever has that.
    - **A curator is a tenant writer key with a writer node grant on `universe`.** An admin
      designates curators by issuing them, and only by issuing them: the grant is the
      designation, journalled and in the evidence export. A curator reads what it curates
      through reader grants an admin gives it, on the user roots or shared workareas concerned;
      a user's promoted work becomes readable to a curator only by such a journalled grant.
      An admin key never curates itself: it carries no grant, and an admin who curates holds a
      curator key, as ADR-0010 d6's two-key rule has it for every content act.
    - **Curation is `POST /v1/memories` (`oml_write`) with `workarea: universe` and
      `derives_from` naming the source.** The new version is the curator's own claim, written in
      the curator's words, held on arrival and scanned like every write. `derives_from` is not
      required: a curator may also place an original claim, a tenant-wide steering row for
      instance. A source a curator cannot read fails closed, as `derives_from` always does.
    - **A second identity must ratify it, by the ordinary pipeline.** The held universe version
      is promoted only by a non-author verdict from a dreamer key with dreamer effective access
      on `universe` (a tenant key; d3 rules out user keys), under `universe`'s review policy.
      R2 excludes every key and principal in its lineage closure: the curator, the source's
      author and everyone in the source's own closure. So the
      reviewer is never the curator, never the source's author and never in its lineage, and
      the curator is never the reviewer of its own placement.
    - **R1 and R3 hold unchanged.** No verb is added: the curator writes, the reviewer judges,
      and promotion is the effect of ratification, as it always was. The source's promotion in
      alice's root made it visible to alice's root's readers; only the universe version's own
      ratification makes the curated claim visible to the tenant, so a wider audience always
      passes its own review. Removing a curated claim is the ordinary journalled demote or a
      retirement; nothing is deleted (R4).

    The cost is review capacity on `universe`: a tenant with curators and no dreamer granted on
    `universe` holds every curated claim forever, failing closed, which the review-availability
    signal reports (d14).

- **d8 — The reviewer exclusion stays per key and principal; it does not extend to the user.**
  A user key's verdict is excluded exactly as any key's is: by the keys and principals in the
  row's lineage closure and, for a held successor, its rivals' authors
  ([the dreamer split](../architecture/roles-and-keys.md#the-dreamer-split)). `user` is not added
  to that set, so a person may review their own agents' writes through a separate dreamer key of
  their own, granted in their root, and a user working alone can promote their own agents' work.
  The maintainer chose this on 2026-11-04 over widening R2 to the person. The cost is said
  plainly: R2 holds per credential, not per person, inside a user root, and the journal shows
  that both keys name the same user, so an audit can see a person approving their own agents'
  work. ADR-0001 d2 and ADR-0010 d5 are unchanged.

- **d9 — A universe claim is served without its source: origin `universe`, no author, no
  lineage below it.** To every key outside the claim's own authoring key set
  ([own](../architecture/roles-and-keys.md#own)), a version whose workarea is `universe` is
  served with:
    - `workarea_id` the universe id and `workarea_path` `universe`, which is the claim's own
      workarea, so no field says where its source lived;
    - `principal` null;
    - `derives_from` empty, whatever the caller can read, and `replaces` only where it names a
      version in `universe`, which a supersession always does, since a successor keeps its
      target's workarea;
    - on the lineage read, when it is built, a closure that stops at `universe`: nodes outside
      `universe` are omitted, not elided to redacted ids, so no id that points into a user's
      root is served (this narrows, for universe claims, the redacted-id residual of
      #129);
    - on the events tail, what it already serves: `lifecycle.*` events of universe versions
      carry no key ids and no principal for a key outside the actor's key set, and
      `submission.*`, the only events whose payload names `derives_from`, reach the curator's
      key set alone ([events tail](../api/http-api.md#events-tail)).

    The rule is one serving rule, so it holds on every read: `GET /v1/memories/{artifact_id}`,
    recall and every listing on HTTP; every mode of `oml_read` on MCP, the `oml:librarian`
    subagent included; `oml read` and `oml events` on the CLI; the session-start brief and the
    failure recall of `oml hook`; and any MCP resource ADR-0036 adds, if it is accepted. What
    stays: the claim's own `artifact_id`, which `oml_read` takes by id, its `claim_key`,
    `version`, `kind`, `text`, `tokens`, `created_at`, `promoted_at` and `plaintext_digest`. The
    source's author is served the same view as everyone: its own source in its own root is
    unchanged, and neither row names the other to it. The curator's own key set is served its
    own rows as every author is. The journal keeps every attribution field in the clear — the
    curator's key and principal in the submission envelope, `derives_from` in its payload, the
    reviewer in the review events — and the [evidence export](../api/http-api.md#evidence-export)
    carries them; an admin key reads no claim content, so the admin's audit of a curation is the
    export and the journal, unchanged. This amends ADR-0025 d3, whose readability rule for
    `derives_from` and `replaces` would otherwise show alice her own source's id.

    Three residuals, stated plainly: a curated claim whose text equals its source's carries the
    same `plaintext_digest`, so anyone who can read both rows — the source's own readers — can
    link them; a curator who reuses a source's wording or chooses a telling `claim_key` discloses
    what it wrote; and the tail's seq gaps still count events a key cannot see (#129). The first
    two are the curator's to avoid; #144 tracks giving the curator a check for them.

- **d10 — Recall at a scope inside a user root also searches `universe`'s own claims, and
  steering and guardrails reach `universe` as an ancestor.** The hits of a recall whose scope
  node lies inside a user root are the served tips of subtree(scope) within reader effective
  access, as now, together with the served tips of the `universe` node itself where the key's
  reader access covers it (d3's node grant does), ranked together in the one total order of
  [retrieval](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked);
  universe hits are served under d9 and own held rows never include them. Without this, a
  user's default scope — their root — would never meet a curated claim, and the session-start
  brief, which recalls at the default workarea
  ([ADR-0029](ADR-0029-session-start-hook.md) d2), would hold none. Applicable steering already
  includes rows on every ancestor within reader access (ADR-0025 d8), so `universe`'s steering
  applies at every scope in every user root with no change. ADR-0037's `guardrail_scope:
  ancestors`, if accepted, walks the same ancestry under the same reader test and so serves
  `universe`'s guardrails to every user key; `workarea` serves the scope node's alone, as
  ADR-0037 d4 says. The listing (`GET /v1/memories` without `own`) and the events tail keep
  their subtree scopes: a user who wants universe's claims as a list, or its events, names
  `workarea: universe`, where subtree(`universe`) within its access is `universe` and its own
  root. This amends ADR-0025 d8's hit membership. The cost is that universe hits compete for
  rank and budget with a project's own hits and no field turns them off; #145 tracks measuring
  whether they crowd out the hits a project needs.

- **d11 — The surface grows by three fields, one grant attribute and two CLI flags, and a client
  still starts from a URL and a key.**
    - `POST /v2/keys` takes `user`; `GET /v2/keys/{key}` and `GET /v2/whoami` serve `user` as
      `{id, name}`, null for a tenant key. A client learns its root from `whoami` alone, so
      ADR-0039 d15's URL-and-key start is unchanged; this amends d15's answer shape.
    - A grant takes `covers` (d4), in issuance and in `POST /v2/keys/{key}/grants`, and every
      grant listing serves it.
    - The `Workarea` object serves `user_root`, so the tree and the workarea read show which
      nodes are user roots.
    - Every workarea reference takes a qualified name (d5), and `POST /v2/workareas` admits a
      user key (d6).
    - `oml keys issue` and `oml keys issue-agent` take `--user <user>`. With it, `issue-agent`'s
      `--workarea` becomes optional, defaults to the user's root, and is resolved by the server
      in that user's namespace, so `oml keys issue-agent claude-laptop --principal alice/laptop
      --user alice --out key` issues a writer key granted on `universe.alice` with its reader
      grant beside it and `universe`'s node read added by the server. `--grant` takes the
      `:node` suffix (d4). This amends [ADR-0028](ADR-0028-agent-key-issuance.md) d1. The
      `oml-admin` plugin's skill passes `--user` when the operator names a person.
    - `oml login` and `oml doctor` print the key's user root beside its role, tenant and default
      workarea.
    - No MCP tool, tool argument or HTTP route is added; `oml_write` with `workarea: universe`
      from a user key is refused (d14).

- **d12 — Existing keys and workareas are grandfathered: every key issued before the build is a
  tenant key and every workarea is shared, and nothing moves.** The upgrade journals nothing and
  rewrites nothing: `api_keys` folds `user` as null and `workareas` folds `user_root` as false for
  every row, which is what their journal events, which carry neither field, say. On the live
  tenant `oml`, `universe.crew.oml` stays a shared crew subtree, the keys working in it keep
  their grants, principals and names, and their claims stay where they are, readable as today.
  The maintainer's own user root appears with the first key issued with `--user`; a name a
  shared workarea already holds, such as `crew` or `oml`, cannot be a user's name there
  (`name.taken`). Moving existing work into a user root needs re-parenting across a root
  boundary, which d1 forbids and which is not scheduled in any form; it is tracked as #143.
  Until then a curator can place what should be shared into `universe` (d7), and the old tenant
  keys stay valid until an admin revokes them, which is not built either. A migration that moved
  the live subtree under a user root was weighed and rejected (below): it would change the paths
  every existing key and log line uses, for a tenant with one person.

- **d13 — Crews stay shared: a crew's workareas live outside every user root, and its keys are
  tenant keys.** A workarea that mirrors an externally managed crew
  ([crews](../architecture/workareas.md#crews-recorded-not-operated)) is shared, created by an
  admin as today, never inside a user root, since a crew is not a person and its work outlives
  any member. Keys working in it are tenant keys. A user key may still name a crew record at
  issuance (`crew`), which feeds the independence axis as for any key, but d3 gives it no grant
  in the crew's workareas. The cost is the second key a person needs to work in a crew, the
  same cost d3 states.

- **d14 — Every new failure answers a named refusal and has a doctor line.**

    | Failure | Refusal | Doctor line |
    |---|---|---|
    | `user` on an admin, operator or service key | `request.validation`, `details.field: user` | — (refused at issuance) |
    | `user` names a shared workarea's name, or `universe` | `name.taken` / `request.validation` | — (refused at issuance) |
    | a user key's grant outside its root | `access.issuer_ceiling`, details `user` | — (refused at issuance and at grant add) |
    | a deny entry on a `covers: node` grant | `request.validation`, `details.field: /grants/<i>/deny` | — |
    | a user key writes into `universe` | `submission.universe_refused`, answered for a user key in place of step 3's `access.grant_refused`, so the agent is told universe is curated | — |
    | a user key creates a workarea outside its root | `access.not_found` / `access.grant_refused` on the parent | — |
    | a user key holds no reader on `universe` (an admin removed the node grant, when grant changes are built) | — | `oml doctor` under the key: `warn universe: this key does not read universe's claims` |
    | a key used by a person names no user (a grandfathered or mis-issued key) | — | `oml doctor` under the key: `user: none (tenant key: names resolve in the shared namespace)` |
    | curated claims wait with no eligible reviewer on `universe` | — (held, failing closed) | `oml doctor` review availability: `warn universe has no dreamer outside its writers' lineage` |
    | a key row naming a root that is not a user root | the lead refuses to serve that key, `auth.invalid` with `details.reason: user_root` | `oml doctor` under the key names the reason |

- **d15 — The wire and the vocabulary are held here until built, and the build is one change set
  in four steps.** No field above enters [openapi.yaml](../api/openapi.yaml) or the designed
  annex until its step is built; the pages mark each as planned for v0.5.0. ADR-0018 d4 gains
  four rows:

    | Term | Means | Never |
    |---|---|---|
    | user | a person an admin attests a key acts for, named by the key's `user` field; the name of that person's user root | "account", "owner" (own means the authoring key) |
    | user root | the workarea `universe.<user>` created for a user, holding everything in that user's namespace | "home", "personal workarea" |
    | user key / tenant key | a key that names a user / a key that names none | "personal key" |
    | curator | a tenant writer key an admin granted writer on the `universe` node | "publisher", "moderator" |

    ADR-0018 d4's `universe` row, "refuses direct claim writes", becomes "holds only curated
    claims (ADR-0044 d7)". The steps, each closed by its tests:

    | Step | Builds | Tests, named after the sentence each proves |
    |---|---|---|
    | S1 | d4, d1, d2, d3, d12: `covers`, `user_root`, `user`, the issuance rules and the fold of old rows | `a_node_grant_covers_its_node_and_no_descendant`, `a_node_grant_with_a_deny_entry_is_refused`, `the_first_key_naming_a_user_creates_its_root_under_universe`, `a_second_key_naming_the_user_joins_its_root`, `a_user_named_like_a_shared_workarea_is_name_taken`, `user_on_an_admin_key_is_refused`, `a_user_keys_grant_outside_its_root_is_refused_as_issuer_ceiling`, `issuance_adds_a_reader_node_grant_on_universe_to_every_user_key`, `a_key_issued_before_users_folds_as_a_tenant_key` |
    | S2 | d5, d6: namespaces, qualified names, user-key create | `two_users_each_create_a_workarea_named_project`, `a_user_keys_bare_name_resolves_in_its_own_root`, `a_qualified_name_resolves_for_an_admin`, `a_three_label_value_is_still_a_path_and_refused`, `a_user_key_creates_a_workarea_under_its_root_by_default`, `a_user_key_cannot_create_outside_its_root` |
    | S3 | d7, d8: curation, and the exclusion per key | `a_user_keys_write_into_universe_is_universe_refused`, `a_curators_write_into_universe_is_held`, `a_curated_claim_is_never_promoted_by_the_sources_author`, `a_users_own_dreamer_key_may_review_its_agents_writes` |
    | S4 | d9, d10, d14: the served view, the recall join, the doctor lines | `a_universe_claim_serves_no_principal_and_no_derives_from_to_a_reader`, `the_sources_author_is_served_the_same_universe_view`, `the_curator_reads_its_own_universe_row_in_full`, `recall_inside_a_user_root_ranks_universe_hits_with_its_own`, `universe_steering_applies_in_every_user_root`, `a_user_key_never_sees_another_users_root_in_the_tree`, `doctor_warns_when_a_user_key_does_not_read_universe` |

## Consequences

- [Concepts](../concepts.md), [workareas](../architecture/workareas.md),
  [roles and keys](../architecture/roles-and-keys.md), [retrieval](../architecture/retrieval.md),
  the [HTTP API](../api/http-api.md), the [CLI reference](../cli/reference.md),
  [errors](../api/errors.md), [releases](../changelog.md) and [llms.txt](../llms.txt) mark
  each part planned for v0.5.0 and not built. The event catalog, the Postgres schema and the
  OpenAPI documents change only with the build (d15).
- ADR-0008 d1, d3 and d4, ADR-0010 d1, ADR-0018 d4, ADR-0025 d2, d3 and d8,
  ADR-0026 d1, d4, d5 and d7, ADR-0028 d1 and ADR-0039 d15 keep their text; each Status line
  points here. The decision register's D7, D9, D13 and D14 rows cite this record.
- Three issues are open: **#143**, moving existing shared workareas into a user root
  (d12); **#144**, a curator's check that a curated claim's text, digest and `claim_key` do not
  identify its source (d9); **#145**, measuring whether universe hits in a user's recall crowd
  out the hits of the user's own scope (d10). Review capacity on `universe` and on each user root
  is part of the unmeasured review throughput of #98.
- The quick start (`oml quickstart`, [ADR-0041](ADR-0041-a-quick-start-in-one-command-per-role.md))
  is unchanged in v0.5.0: it creates the shared workarea `crew` and issues tenant keys. Moving it
  onto a user root is a later decision.
- Admin structure reads (ADR-0026 d15) show every user root by name, which is the user's name;
  an admin already sees every key's principal, so this adds no new disclosure to admins.
- Every check that reads `effective_roles` reads the new coverage test; the hot path gains no
  query, since `covers` rides the grant rows already read per request.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A forest: one tree per user beside `universe`** | The maintainer kept one tree per tenant; a forest gives subtree grants, the child-status rule and the path no single top. |
| **Derive the user from the principal** (`alice/laptop` → `alice`) | ADR-0026 d9 keeps principal, name and label independent; a principal may hold `/` and need not be a name, and a parsing rule would bind a free-text field to authority. |
| **A user record of its own, beside the root** | Two rows for one fact. The root's name is the user's name and its id is what `key.issued` records; a separate record would need its own namespace, lifecycle and journal events. |
| **An implicit universe read for every user key, outside the grants** | It would make one access path that the effective-access formula does not compute, against ADR-0008's "every access check computes d4's formula and nothing else", and the evidence export could no longer reproduce a user key's access from its grants. |
| **A reader grant on `universe` with a deny entry per other user** | Every new user would require amending every other user's keys, and a missed amendment silently reads another person's root. |
| **The full dotted path as the tenant-unique key** | A path changes on a re-parent within a root, so a reference would go stale; the qualified name `<user>.<name>` is unique for the same reason and never changes. |
| **Names unique across the whole tree, as now** | Two users who both want `project` collide, which the maintainer ruled out. |
| **User keys resolving shared names too** | A user later creating a name a shared workarea holds would silently retarget its own reads, and refusing that creation would tell user keys the names of shared workareas they cannot read. |
| **Admins place claims in `universe` with their admin key** | An admin key carries no grant and is never a reviewer or author of content (ADR-0010 d6); an admin write would be the unreviewed admin write the evidence export never has to explain. The admin designates curators instead, and curates with a curator key. |
| **Curation as a new verb that publishes a promoted source directly** | A verb whose effect is visibility to the whole tenant is a promotion by fiat, against R1, and skips the second identity R2 asks for at the wider audience. An ordinary held write reviewed on `universe` needs no new verb. |
| **Curation that moves (reassigns) the source into `universe`** | The source would leave its author's root and carry its attribution with it; d9's anonymous view would then have to rewrite the row it serves. A new version deriving from the source keeps both intact. |
| **Users proposing their own claims for `universe`** | The maintainer ruled it out: users neither write nor propose to `universe`. |
| **The user behind a key in the reviewer exclusion** | It would make R2 hold per person inside a user root, but a user's own dreamer key could then never promote their own agents' writes, so every user root would need a reviewer from outside it and a user could never work alone. The maintainer kept the exclusion per key and principal on 2026-11-04 (d8). |
| **Showing the source's author a link to the curated copy** | It tells alice which of her claims a curator chose, through a field no other reader has, and adds a field for a fact the journal already holds. |
| **Recall at a user scope without universe hits, the client naming `workarea: universe`** | The session-start brief and every agent that recalls at its default would never meet a curated claim; asking every client to recall twice contradicts the URL-and-key start. |
| **A second default workarea on a user key, for reads** | Two defaults for one key, which ADR-0022 d4 and ADR-0039 d4 keep single; d10's join gives the same reach with one rule on the owning page. |
| **Migrating the live tenant: re-parent `universe.crew.oml` under the maintainer's user root at upgrade** | Re-parenting is not built, every existing path, log line and key default would change at once, and d13 keeps crew work shared; grandfathering moves nothing and loses nothing (R4). |
| **User keys reading crew workareas by grant** | A user key would then reach shared subtrees where other people's crews work, by name or id, and "never another user's root" would rest on which shared workareas hold user content; one boundary, the user's own root plus `universe`, is checkable by construction. |

## The amendment of 2026-11-12: a memory is started in a user root by its own user's keys only

**Status of this amendment:** proposed · 2026-11-12 · built in v0.7.3, as #329 asked, and not
accepted: the operator accepts d16 or declines it. Until then d1–d15 above are the accepted
record.

Issue #329 came from the metering review of v0.7.3. d3 leaves a tenant key unchanged, "an admin
may grant it anything, a user's root included", so a tenant writer key holding a subtree grant on
`universe`, or any writer grant inside a user's root, wrote claims into `universe.<user>`.
[ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md)
meters an account by its person's keys (d1) and a chain by the author of its earliest version 1
(d3), so such a key could raise a person's `memories` with no check against their ceiling, and a
person could be charged nothing for a claim in their own root. On 2026-11-12 the operator
answered the question "Who may write in a person's root?" with "Only that person's keys". The
build of that answer refused every claim write there, and the web console's tests showed what it
took away: an admin grants another person review on someone's root, and that person's review key
edits by proposal there. Asked "Which rule do you want?" the same day, the operator answered
"Refuse new memories only". This amendment records that answer and the readings the build took.

- **d16 — A memory is started in a user root, or in a workarea beneath one, by that user's keys
  only; any other key is refused `submission.user_root_refused`. A successor or a retirement of a
  served memory there stays under the grants.** This narrows d3's "a tenant key is unchanged"
  for starting a memory and for nothing else.
    - **What starting a memory is.** A new memory: `POST /v1/memories` and `oml_write` without
      `replaces`, with a `claim_key` or without one, under a `claim_key` a rejected write
      released included (ADR-0025 d18). It is the use case `submit_memory`, the one that writes a
      version 1, and a version 1 is what begins a chain and names its account (ADR-0071 d2, d3).
    - **The test.** The workarea lies in a user root when that root is the workarea or one of
      its ancestors. The write is admitted when the key acts for that root's user. The person a
      key acts for is read the one way `same_user` reads it
      ([ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)
      d2): a session key's `session_user`, else the name of the root a user key's `user` names;
      a key with neither acts for no one. That name is compared with the root's. So a person's
      keys are their user keys, `<user>-web` among them, and the session keys that serve
      them: `<user>-review` and `<user>-curate` carry no `user` and name their person by
      `session_user` (ADR-0045 d6, d10). A tenant key that serves no session is refused.
      Another person's key is refused the same way, their session keys included: an admin's
      review designation gives a person's `<user>-review` dreamer and reader on another
      person's root (ADR-0045 d10), where it is still its own person's key, so the rule reads
      the key's person and does not rest on d3, which keeps a user key's grants inside its own
      root at issuance. The rule admits a key and confers nothing; the role and the grants
      still decide. `<user>-review` and `<user>-admin` are refused a new memory by their role
      (`access.verb_ceiling`) wherever it is addressed, so the review key meets the rule on a
      successor of a ratified retirement only. `<user>-curate` holds writer on the `universe`
      node alone, so its new memory in its own person's root is `access.grant_refused` where it
      reads that root and `access.not_found` where it does not, and in another person's root it
      reads, `submission.user_root_refused`. The first build of this
      amendment compared the key's `user` with the root: it refused a person's own
      `<user>-review` the successor of a ratified retirement in their root, so an admin who
      retired a memory of their own on the admin page could not put it back. The review of
      the build found it on 2026-11-12.
    - **A successor or a retirement of a served memory is unchanged.**
      `POST /v1/memories/{artifact_id}/supersede` and `oml_write` with `replaces` are admitted as
      ADR-0025 d10 and d11 have them: from a writer on a version it wrote, and from a dreamer
      holding dreamer and reader there, by edit as proposal, whoever the key's user is. A
      reviewer granted in a user's root, a tenant key or another person's, still proposes edits
      and retirements there, as the admin page does. Neither starts a chain or changes its
      account: a successor moves no `memories`, and a ratified retirement takes the claim off
      its account's.
    - **The guard: a successor of a ratified retirement is the user's keys' alone.** A successor
      whose target is the claim's served retirement puts the claim back in service when it is
      ratified, and the chain is live again on its account's `memories` (ADR-0071 d2). In a user
      root that is a memory started, so any key but the user's is refused
      `submission.user_root_refused` for it, with `retirement` or without. The operator's words
      of 2026-11-12 do not name this case. It is the build's finding and the reading taken of
      "Refuse new memories only": without it a reviewer granted there could raise the user's
      `memories` by a write that is not a new memory, the effect that answer keeps closed. A
      successor proposed while a retirement of the same memory is still held is admitted, since
      its target is the served memory. The two are rivals for one tip: once the retirement is
      ratified, approving the other is `submission.tip_conflict`, so it never puts the claim
      back.
    - **The refusal confirms nothing to a key that reads nothing.** `submission.user_root_refused`
      is `409` and journals nothing. For a new memory it is answered where the
      [refusal order](../architecture/roles-and-keys.md) would allow the write or answer
      `access.grant_refused`, both of which mean the key reads the workarea; a key that reads
      nothing of it keeps `access.not_found`, the answer for a workarea that does not exist, and
      a reader-role key keeps `access.verb_ceiling`. For a successor of a ratified retirement it
      is answered once the target is readable to the key, ahead of `access.author_only` and
      `access.grant_refused`; an unreadable target keeps `access.not_found`. This is the pattern
      d14 set for `submission.universe_refused`.
    - **Where the check sits.** In `submit_memory`, after the grant computation, and in
      `supersede`, once the target is known readable and only where it is a served retirement.
      It is not in the grant model: `effective_roles` stays the one formula of ADR-0008 d4, an
      admin may still grant a tenant key anything, and no grant is revoked.
    - **What else is unchanged.** Reads, recall, the listings and the events tail under reader
      access. Review by a key granted dreamer there: verdicts, the bulk verdict, transitions,
      claim-edge and brief-edge proposals and ratifications, as d3 and d7 have them. A source
      check, which journals a check on an existing version and adds none. Curation, which writes
      on `universe` and not in a root (d7). Workarea creation.
    - **Nothing already written is touched (R4).** The versions tenant keys wrote in user roots
      before the build stay as journalled: served, reviewable, attributed to their authors,
      and under the accounting ADR-0071 d3 states for them. No migration, fold or backfill
      reads them. Their author still proposes a successor or a retirement of one, as a writer
      does of a version it wrote. A held successor of a ratified retirement that a key not the
      user's proposed there before the build is one of them: the rule is checked at the
      proposal, so it can still be ratified, and its ratification puts the claim back on the
      user's `memories` under no ceiling, as the user's own does (#340).
    - **Replays.** A retried new memory answers this refusal, since the access step precedes
      the idempotency lookup there, as it does for a revoked grant. A retried successor answers
      its record, since that verb's lookup precedes every target check.
    - **Costs.** `my_effective_verbs` lists what a key's grants confer, so for a tenant key on a
      node inside a user root it still lists `remember` and `learn`, and the new memory answers
      the refusal. An in-progress workarea inside a user's root
      ([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md))
      collects new memories from that user's keys only; another author's row there is a
      successor it proposed. The user's own keys can still end one over their `memories` limit by
      retiring a claim, writing another and putting the first back, since a ratification's commit
      carries no ceiling; that is as it was before this amendment and is tracked as #340.
    - **Doctor.** `oml doctor` under a tenant writer key whose default workarea lies in a user's
      root prints `user root   : warn: the default workarea <path> lies in the root of user
      <user>, where only <user>'s keys start memories; this tenant key's new memories there are
      refused (submission.user_root_refused); issue the agent a key with --user <user>`, and its
      verdict is `warn`.

    d14 gains one row:

    | Failure | Refusal | Doctor line |
    |---|---|---|
    | a key that is not the user's starts a memory in a user root or beneath it, or proposes a successor of a ratified retirement there | `submission.user_root_refused`, answered where the key reads the workarea or the target; `access.not_found` otherwise | `oml doctor` under a tenant writer key whose default workarea lies in a user's root: `user root   : warn: …` |

    The tests, named after the sentence each proves, run over the in-memory double and
    PostgreSQL (`testing/oml-port-contracts/src/user_roots.rs`):
    `a_key_that_is_not_the_users_starts_no_memory_in_a_users_root_or_below_it`,
    `a_key_that_reads_nothing_of_a_users_root_is_told_not_found_as_for_no_workarea`,
    `a_users_own_keys_still_write_supersede_retire_and_put_a_retired_claim_back`,
    `a_reviewer_granted_in_a_users_root_proposes_a_successor_and_a_retirement_there`,
    `a_successor_of_a_ratified_retirement_in_a_users_root_is_the_users_keys_alone`,
    `a_successor_proposed_while_a_retirement_is_held_is_never_ratified_after_it`,
    `a_tenant_key_reads_recalls_and_reviews_in_a_users_root_as_before`,
    `a_memory_a_tenant_key_wrote_in_a_users_root_before_the_rule_stays_served_and_its_author_still_revises_it`,
    `a_held_put_back_another_key_proposed_before_the_rule_is_still_ratified_after_it`
    and, with the session keys a first sign-in issues,
    `a_persons_session_review_key_puts_a_retired_claim_back_in_its_own_root_and_in_no_other_persons`;
    over the application,
    `each_session_key_of_a_person_is_read_in_a_user_root_as_that_persons_key_and_no_one_elses`;
    over HTTP and `/mcp`,
    `a_tenant_keys_write_into_a_users_root_is_user_root_refused_and_her_own_key_still_writes`
    and
    `oml_write_by_a_tenant_key_starts_no_memory_in_a_users_root_and_its_reviewer_still_edits_by_proposal`;
    on the admin memory page,
    `an_admin_retires_his_own_memory_and_puts_it_back_on_the_admin_memory_page` and
    `the_admin_memory_page_refuses_putting_another_persons_retired_memory_back`.

### Rejected alternatives of the amendment

| Alternative | Why not |
|---|---|
| **Keep it and document it: a tenant key granted writer above or inside a user root starts memories there** | The operator ruled it out on 2026-11-12. A person's `memories` would keep moving by keys that are not theirs, with no check against their ceiling, and ADR-0071 d1's "what their keys write" and "what lies in their root" would name different chains. |
| **The strict rule: every claim write there, a successor and a retirement included, from the user's keys only** | Built first, and rejected by the operator on 2026-11-12: it removes edit by proposal and the retirement proposal for a reviewer an admin granted review there, which the admin page relies on. Neither act starts a chain or changes its account. |
| **No guard: a reviewer granted there also proposes the successor of a ratified retirement** | Once ratified it raises the user's `memories` by a key that is not theirs, with no check against their ceiling: the first effect of #329 by another write. |
| **Hold the ratification that puts a claim back to the account's ceiling, in place of the guard** | It changes the verdict path for every key, a reviewer's approval answering `limit.quota` for the author's account, and is a decision of its own (#340). The guard is one condition and only restricts. |
| **Refuse it in the grant model: no writer grant reaches a user root for a tenant key** | A subtree grant on `universe` reaches every root without naming one, so the refusal would have to live in `effective_roles`, a second formula beside ADR-0008 d4's, and every existing grant would have to be revoked, which is not built. The write check is two call sites. |
| **Answer `access.grant_refused`** | It names a missing role, and granting that role would change nothing. |
| **Refuse it in the store's commit** | The store does not know what the calling key reads, so it could not keep `access.not_found` for a key that reads nothing of the root. |
| **Drop `remember` and `learn` from `my_effective_verbs` for such a key** | The field lists what grants confer (ADR-0025 d14) and stays silent on a `validated` workarea too; changing what it means is a decision of its own. |
| **Demote, reassign or delete what tenant keys already wrote in user roots** | R4: nothing is deleted silently, no verb reassigns a version, and each row is attributed truthfully to the key that wrote it. |
