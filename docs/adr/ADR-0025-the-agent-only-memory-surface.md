<!--
id: adr/ADR-0025-the-agent-only-memory-surface
tier: engineering
status: accepted design
answers: how one agent key both writes and recalls through grants; what an agent may read back of its own held memory; how a recall is budgeted in budget tokens and truncated deterministically; how supersession and the author's retire-proposal move the tip only through admission; which MCP tools carry that surface
-->

# ADR-0025 — One key writes and recalls through grants, and an agent reads back, budgets, supersedes and retires its own memory only through admission

**Status:** accepted design · 2026-10-31 · amended by [ADR-0026](ADR-0026-names-are-keys.md)

**Amends:** ADR-0001 (d5), ADR-0008 (d4, d6), ADR-0010 (d1, d3), ADR-0012 (d7), ADR-0013 (d1, d2, d5), ADR-0014 (d2), ADR-0018 (d4), ADR-0022 (d4, d6)

## Context

**The two roadmap rows.** [FR-10 and FR-11](../product/roadmap.md) (roadmap.md:53-54) are the
critical-path rows for M2's agent-only-memory scenario: an agent that stores only to OML can read
back what it wrote, search it under a budget, update it, and propose retiring it
([critical path](../product/critical-path.md), critical-path.md:155-166). Neither row has a home
in an accepted record:

- `include_own_held`, `max_tokens`, per-row counts, an `oml_get` tool, `oml_supersede` and a
  journalled author retire-proposal are declared nowhere;
- [ADR-0021](ADR-0021-ai-human-ontologies.md) d9 and [ontologies](../architecture/ontologies.md)
  (ontologies.md:201-202) already cite `max_tokens` as if it existed.

**The one-key shape.** The Claude Code plugin needs one key that both remembers and recalls. The
design supports it: a writer-role key whose grants include reader scope
([roles and keys](../architecture/roles-and-keys.md), roles-and-keys.md:34-40;
[CLI sessions](../cli/sessions.md), sessions.md:102-115). The design never says which grant roles
each key role admits: "a role no higher than that ceiling" presupposes an order that "roles do not
imply one another" denies.

**What the PoC does instead.** It holds one role per key and checks it by equality
(crates/oml-app/src/app.rs:37-46). Several of its behaviours contradict invariants that a single
writer+reader key would expose:

- supersede flips the tip at proposal, before any review (app.rs:663-749), against
  [audit and lineage](../architecture/audit-and-lineage.md) (audit-and-lineage.md:138-141) and
  [the agent plugin](../api/mcp-plugin.md) (mcp-plugin.md:107-110);
- the events tail is tenant-wide, so every reader sees other writers' held submissions
  (app.rs:780-794), against R3;
- the transport drops deny masks and invents grant ids
  (crates/transports/oml-transport-http/src/lib.rs:481-565);
- recall falls back to the whole tenant (crates/oml-app/src/types.rs:153-155);
- a steering row is served as a ranked hit (lib.rs:178, 204-210);
- the PoC neither NFC-normalizes bodies nor journals `body_bytes`, `version` or `kind`
  (app.rs:435-446), and its `memories` table has no `version`, `kind`, tip, `promoted_at` or
  `body_bytes` column and a nullable `claim_key` (migrations/0001_init.sql:167-178,
  migrations/0002_poc_adapter.sql:25-33, crates/oml-app/src/types.rs:42), while the claim item
  of d12 needs every one of them.

**Open readings in the pages.** What "own" means, which steering applies, the rank tie-break, the
budget unit, what a recall logs, the events-tail filter, and where `my_effective_verbs` comes from
are all left open.

**What this record does.** It is one decision set: the agent-only memory surface and the access
model it rests on. It decides each open reading against R1–R4 and the honesty discipline
([ADR-0018](ADR-0018-the-documentation-set.md) d3, D17). Wherever the design already has machinery
for a behaviour, it reuses it, so it adds no endpoint, no event type, no claim state and no error
code: the [error taxonomy](../api/errors.md) and the operation set of the
[OpenAPI document](../api/openapi.yaml) are unchanged. The multi-arch release image
changes no decision; it is a page edit to [CI and toolchain](../engineering/ci-and-toolchain.md).

Line citations in this record refer to the tree at commit `cfe6d0d`, the base it was written
against; several cited pages change in the same change as this record, so their current line
numbers differ. Everything this record decides is design. The PoC behaviours named above are
sourced to the code that has them; nothing decided below is built.

## Decision

- **d1 — Grant admissibility is a table, effective roles are one pure function, and every
  workarea-scoped verb refuses in one order.** A key's role names exactly which grant roles the
  key may carry. The table is declared once, on
  [roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) (D9):

    | Key role | Admissible grant roles |
    |---|---|
    | reader | reader |
    | writer | reader, writer |
    | dreamer | reader, dreamer |
    | admin | none |
    | operator | none |

    A grant role is never admin or operator. A grant whose role falls outside its key role's set is
    refused with `access.issuer_ceiling`, details `{role, admissible}`, plus `grant_index` at
    issuance (d2). A key of role admin carrying any dreamer grant is refused with
    `access.role_exclusive`; that check runs first, at issuance and at every grant amendment, and
    it moves from the HTTP transport into `oml-domain`.

    `effective_roles(N)` is a pure `oml-domain` function computed on every request, with no cache:
    take every grant whose node is N or an ancestor of N; skip a grant whose own deny list names N
    or an ancestor of N; take the union of the remaining grants' roles; and answer the empty set if
    any global per-key denial names N or an ancestor of N. This is
    [workareas](../architecture/workareas.md) (workareas.md:105-143) as written.

    A verb that needs role R on node N resolves in this order:

    1. R is not admissible under the key's role → `access.verb_ceiling`;
    2. R is in `effective_roles(N)` → allowed;
    3. reader is in `effective_roles(N)` → `access.grant_refused`, details `{workarea_id, needed_role}`;
    4. otherwise → `access.not_found`.

    Step 1 depends only on the key's role and needs no node, so it runs before the workarea is
    resolved: `request.workarea_unresolved` is answered only to a call that has passed step 1,
    and a reader key submitting is `access.verb_ceiling` whether or not it has a default
    workarea.

    The needed role per verb: **writer** for submit and steering writes on the resolved node, which
    must exist; **reader** for recall on the scope node (search runs over subtree(scope) ∩ reader
    set), a non-own get on the row's workarea, a non-own listing, the events tail and workarea
    info; **dreamer** for status-transition propose and ratify and for dead-letter re-enqueue, on
    the node, while admin keys act on those verbs by their key role. Supersede follows the ordered
    rule of d10. Review next and verdict need dreamer on the row's workarea in addition to the
    unchanged reviewer check: a row outside dreamer effective access is never served, and a verdict
    on one resolves by the refusal order above, with no exception for review —
    `access.grant_refused` with `needed_role: dreamer` where reader is effective on the row's
    workarea, `access.not_found` otherwise. A `held_by: service` key is refused every review-class
    verb — review queue and review next, verdict, reopen, claim-edge ratify and workarea-edge
    ratify — and status-transition ratify with `access.verb_ceiling`, before any lookup, like step
    1 above: a service key's dreamer grant confers those verbs like any dreamer grant, and this
    rule, not the grant, keeps it out of review and out of every ratification. Admin and operator
    verbs keep their checks on the key's own role ([HTTP API](../api/http-api.md),
    http-api.md:113-117).

    A writer-role key holding `wa:writer` and `wa:reader` therefore remembers and recalls on `wa`.
    Recall refuses only through grants: the tenant-wide fallback goes, and a request with no scope
    and no default answers `request.workarea_unresolved`.

    The table is the smallest order consistent with every sourced example: the writer+reader
    transcript (sessions.md:102-115); an admin writes ordinary claims only through a writer-role key
    (roles-and-keys.md:62-69, 201-207, matrix footnote 1); the operator holds no grant
    (roles-and-keys.md:211-213); a dreamer that proposes edges needs reader on both endpoints
    (workareas.md:177-192). The refusal order is [errors](../api/errors.md) (errors.md:63-71) to
    the letter, since `access.grant_refused` fires only where the key can read. One pure
    function keeps [ADR-0008](ADR-0008-workarea-algebra.md) (no second access mechanism) and
    [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d1; grants never reach admin verbs
    (R1); R2 is untouched. A grant carries a role, not a verb list, so the reconciler's dreamer
    grant ([security](../architecture/security.md)) confers every dreamer verb; refusing the
    review class and every ratification by the `held_by: service` attestation keeps "never counts
    as review capacity" (roles-and-keys.md:54-55) true of ratifications too, which use no serve.
    That the grant still confers the propose verbs is a stated residual,
    [FR-32](../product/roadmap.md).

    The use-case tests issue keys with no grants and no default and rely on the issuer's default
    (crates/oml-app/tests/use_cases.rs:39-50, app.rs:304), which d2 removes: run unchanged, every
    one of them that submits stops at `request.workarea_unresolved` before it reaches the refusal
    it pins. With fixtures that grant each key its own role on one workarea, as the HTTP tests
    already do (crates/bins/oml-server-lead/tests/http_api.rs:216-234), the pinned refusals change
    code in these places only:

    - a writer key calling recall (use_cases.rs:277-292, http_api.rs:739-749) answers
      `request.workarea_unresolved` when the key has no default workarea and names no scope — the
      use-case writer as written — and `access.not_found` when its scope holds no reader grant, as
      with the HTTP tests' writer-only grant, which mcp-plugin.md:371-382 already specifies;
    - a writer key superseding another key's claim (use_cases.rs:449-461) answers
      `access.author_only` only when it can read the target, and `access.not_found` with a
      writer-only grant (d10);
    - a second supersession of a tip whose first successor is still held (use_cases.rs:463-485,
      http_api.rs:826-834) is no longer refused at submission: it is admitted as another held
      successor, and `submission.tip_conflict` moves to the verdict that loses the compare-and-set
      (d10).

    The reader and dreamer submits (use_cases.rs:251-275, http_api.rs:423-434,
    crates/bins/oml-server-lead/tests/mcp_facade.rs:463-476), the writer verdict
    (use_cases.rs:200-212), the writer review serve (http_api.rs:559-569) and the reader supersede
    (http_api.rs:785-794) keep `access.verb_ceiling` under step 1 above, whatever the fixture.

    This amends ADR-0010 d1 (a key role names its admissible grant roles; it is not a rung in an
    order) and ADR-0010 d3 (grant admissibility joins the issuer ceiling at issuance and at every
    grant amendment).

- **d2 — Issuance carries grants and denies, drops nothing, and discloses the value only after the
  last grant commits.** `POST /v2/keys` takes `{role, grants: [{workarea_node_id, role, deny:
  [{subtree_node_id}]}], default_workarea_id?, held_by?, principal?, label?, expiry?,
  tenant_id?}`; an unknown field is `request.validation`. Validation runs in this order:
  `tenant_id` is present if and only if the issuer is an operator key, else
  `request.validation`; `access.role_exclusive`; the
  issuer ceiling on the key role (`access.issuer_ceiling`, including an admin minting an operator
  key, with details `{role, admissible}`: the requested key role and the key roles the issuer
  may issue); grant admissibility (d1; `access.issuer_ceiling` with details `{role, admissible,
  grant_index}`: the first refused grant's role, the key role's admissible set and that grant's
  0-based index in `grants`); every grant node and deny node exists in the tenant, else
  `access.not_found`; each deny node lies inside its own grant's subtree, else
  `request.validation`; a `default_workarea_id`, when present, is covered by some grant of the
  key, else `request.validation`.

    When `default_workarea_id` is absent, the default is the first grant's node; a zero-grant key
    has none. A key never inherits the issuer's default, so the inheritance at app.rs:304 goes.

    `held_by` is the issuing admin's attestation (event-catalog.md, `key.issued`), so only an
    admin key sets it, and an operator key's issuance that carries it is `request.validation`. A
    tenant admin issues the `held_by: service` keys — the reconciler's dreamer-role repair key and
    a forwarding follower's reader-role hop key ([security](../architecture/security.md)) — under
    the ordinary issuer ceiling; the operator issues none, since it issues only a tenant's first
    admin key. roles-and-keys.md:52 and security.md:246, which named operators as the issuer, are
    edited to match.

    `key.issued` carries the full catalog payload `{key_id, role, principal, issued_by_key_id,
    expiry, value_disclosed: false, held_by, default_workarea_id}`; one `grant.issued` follows per
    grant, and a grant's epoch is its own journal seq. The key value is returned only after the
    last `grant.issued` commits. If issuance fails after `key.issued`, the caller gets an error and
    the server journals `key.revoked` for the key in a following commit, with `reason_digest` over
    the fixed reason `issuance_incomplete`; the key is then listed as revoked. Its value was never
    disclosed, so it authenticates no request either way, including before that commit lands. The
    `201` echoes each stored `grant_id` and epoch, and the key's `default_workarea_id` and
    `held_by` as `key.issued` journals them, as every key read does, so the issuer sees the
    default the server chose; invented grant ids and a fixed epoch of 1 go.

    `POST /v2/keys/{key_id}/denials` journals `key.denied` with epoch equal to its own seq, admin
    key role only; the CLI verb is `oml deny set`. Grant add, grant revoke, denial lift and issuance
    idempotency are not built in this iteration, and the README says so.

    A silently dropped deny widens access, which violates "refused, not coerced" (errors.md:78),
    and a closed body cannot ignore a `tenant_id` the issuer may not send, so that is refused too.
    Withholding the value until every grant commits makes a partial failure fail closed without
    changing the port's one-event commit. Under the issuer's default a new key would target
    `universe`, which refuses every claim write.

- **d3 — "Own" means the authoring key; own-work reads need no grant but obey global denials; a
  lineage target must be readable.** A row is the caller's own when its authoring key
  (`memories.author_key_id`) is the calling key; once rotation is built, own also covers a key in
  the caller's journalled `rotated_from` chain. Principal equality never confers own-row
  visibility, so the principal match at app.rs:639-661 goes; R2 still excludes reviewers by both
  key id and principal. The definition governs get of held or rejected versions,
  `include_own_held`, `GET /v1/memories?own=true` and the `access.author_only` check on supersede,
  which reads it on the target version (d10), and it is declared on
  [roles and keys](../architecture/roles-and-keys.md#own).

    Own-work reads admit any tenant role except operator and need no grant; a global per-key
    denial covering the row's workarea still masks the row (deny-wins). An own row that is a
    simulation row or sim-derived is read only while the calling key is in that simulation's
    audience; outside it the row is absent (`access.not_found`), like any fenced row
    ([simulations](../architecture/simulations.md), D10), on get, the own-work listing,
    `include_own_held` (d4) and the events tail's author clause (d15). Every `derives_from` target
    and every supersede target must be readable by the submitting key — promoted within its reader
    effective access, or its own row in any state — and anything else is `access.not_found`,
    indistinguishable from a nonexistent id. A non-own `self_signed` version is readable only by a
    key with reader effective access on its own workarea, which get, resolving no workarea, takes
    as its implicit scope ([retrieval](../architecture/retrieval.md), d8). http-api.md:287-291 is
    reworded to say writer is the only role needed and the ancestors must still be readable.
    Served, the two lineage pointers obey the same test on every read: `derives_from` on get,
    recall and every listing lists only the ancestors the caller can read, and `replaces` is null
    when the caller cannot read the replaced version — after an admin's reassignment of it, say,
    or its return to held — so one item never names in `replaces` what its `derives_from` hides. A
    promoted version derived from its author's held row therefore names that row in neither
    pointer on get, recall or listings; the lineage read names it as a redacted bare id, as it
    names every node outside the reader's scope, so a reader of the descendant learns that the
    ancestor exists and its id, never its content — a stated residual tracked as
    [FR-31](../product/roadmap.md).

    **Naming a node.** An own row can sit on a node where the key holds no role, since an admin's
    reassignment can move it there. A response names a claim's node to a key only where
    `effective_roles(node)` is not empty for that key, and serves null in its place elsewhere:
    `workarea_id` and `workarea_path` on get and the listings, the claim-subject workarea ids in an
    events-tail payload (d15), and `workarea` and `workarea_path` on the agent plugin (d12). A key
    never learns the id of a node it holds nothing on, except the ancestors of a node it can read,
    whose ids (`parent_id`) and names (path segments) the workarea read and every derived path
    already disclose, and a re-parented node's former parent, which `workarea.reparent` names as
    the workarea read did before the move. The rule is declared with own, on
    [roles and keys](../architecture/roles-and-keys.md#own).

    R3 lets exactly "its author" see a held row (ADR-0018 d4, "held"), and mcp-plugin.md:606-607
    scopes own work to "the calling key's own submissions". A principal is set at issuance and
    shared across keys: comparing principals would widen held visibility to the same person's
    admin and dreamer keys, blurring the capacities ADR-0010 d6 keeps apart, and so fail open.
    Narrower visibility beside wider reviewer exclusion fails closed on both sides. Masking by
    global denials keeps own work inside the deny-wins formula rather than beside it, and a
    readable lineage target removes an existence oracle for other authors' held rows. Holding own
    rows to the simulation audience keeps the fence one predicate on every read: a key removed
    from an audience loses its own simulation rows on its next read, as "removal takes effect on
    the next read" (event-catalog.md, `simulation.audience_changed`) says of every other read. One
    naming rule for get, the listings and the tail keeps them from disagreeing about a moved row,
    and naming every node the key holds any role on lets a writer-only key see where its own rows
    sit. Holding `replaces` to the `derives_from` test keeps a reassigned version's id out of an
    item whose lineage hides it, and stating which node ids escape the rule keeps the rule honest:
    the workarea read serves `parent_id`, so a tail that nulled it would contradict the read.

- **d4 — `include_own_held`: the calling key's held rows join the ranked hits, flagged by state,
  inside the recall's read set.** `RecallRequest` and `oml_recall` gain `include_own_held`
  (boolean, default `false`); `false` is exactly the prior behaviour. With `true`, the hit
  candidates also include the calling key's own rows that are in state held, of kind `memory` or
  `derived_fact`, not retirement versions, in a workarea within subtree(scope) ∩ reader effective
  access minus global denials, inside the simulation's audience when a simulation row or
  sim-derived (d3), matched by the query, and admitted by the request's `kinds` filter
  when one is sent, as promoted hits are. They are ranked by the same total order
  as promoted hits (d8) and interleaved in `hits`, each with `memory.state` `held` and `tip`
  `false`; they count against `k` and `max_tokens` exactly like promoted hits. A held successor
  carries `replaces`; the promoted tip it would replace stays in the hits until ratification, and
  the two are never deduplicated. Own held steering and own held retirement versions never enter
  recall, because neither is in force; `oml_my_work` and `oml_get` reach them.

    The recall ceiling is unchanged — reader on the scope — so a writer-only key reads its held
    rows through the own-work listing and get. No other key's result changes. Held rows have no
    embedding ([retrieval](../architecture/retrieval.md), retrieval.md:48-52), so a held row's
    score is the fusion function evaluated with its full-text rank and no vector term: the vector
    component contributes exactly what an absent candidate list contributes under the fusion in
    use. Held and promoted candidates are then merged under the one total order of d8 before the
    `k` cut, so, given the same scored candidates, the interleaving, the `k` cut, the budget cut
    and the read-log ids are a pure function of them: on one server, over a fixed candidate list,
    the same request always serves the same rows. Across servers they agree only where the scores
    agree, since fusion is an implementation choice. In the PoC both scores are to be `ts_rank` under the same `simple` configuration and
    `plainto_tsquery` (crates/adapters/oml-pg/src/lib.rs:673-681), so they compare directly.
    A recall with `include_own_held: true` is an own-held read and routes to the lead and
    primary in cluster mode. The rule is declared in
    [retrieval](../architecture/retrieval.md#own-held-rows-in-recall); it amends ADR-0013 d1,
    whose serving tier was promoted rows only.

    One ranked list flagged by `state` is the smallest rule an agent learns: it saves the
    my-work-then-get round trips, and it is how FR-11's "steering rows first, then by rank" already
    reads. The `/v1` state enum already has `held`, so no field or segment is added, and the flag
    defaults to off. R3 is untouched: only the authoring key gains rows, and only inside workareas
    it can already read. Because a held row has no vector term, under any fusion in which an
    absent list contributes nothing and a vector match never lowers a score, it scores no higher
    than it would once promoted, so own held rows rank at or below their promoted rank; this
    affects only the author's own result, and how far below is unmeasured
    ([FR-4](../product/roadmap.md)).

- **d5 — Held text is matched only through a read fenced to its author, never through the shared
  serving indexes.** The designed form, decided and not built: a `held_fts` table
  `(tenant_id, memory_id, author_key_id, workarea_id, tsv)` holding rows only while they are held.
  Every transition into held inserts the row in its own commit: the capture commit, with the
  `tsv` computed from the NFC plaintext before `BEGIN`; a reopen, and a reassignment that
  re-enters held, with the `tsv` computed on the serving path under the same decrypt rule as get.
  Every transition out of held removes it in the commit that makes the transition — promotion,
  rejection and erasure among them. A reassignment updates `held_fts.workarea_id` in its own
  commit, and a reassignment that re-enters held removes the version from `memories_fts` and
  `memory_embeddings` in that same commit. The table carries FORCE `tenant_isolation` RLS plus a
  restrictive policy over the caller's own key set,
  `author_key_id = ANY (string_to_array(current_setting('oml.own_key_ids'), ',')::uuid[])`;
  `oml-server` sets `oml.own_key_ids` from the authenticated key and its journalled
  `rotated_from` chain at the start of every transaction, never from the client, so a rotated
  key still matches its predecessor's held text, as own (d3) requires. A commit that moves a
  version into or out of held under another key — a promotion, rejection, erasure, reopen or
  reassignment — sets it instead, transaction-local (`set_config(…, true)`), to that version's
  author key set, inside round trip one's statement beside `pg_advisory_xact_lock` and the
  chain-head read ([ADR-0024](ADR-0024-performance-foundation.md) d1), for the whole
  transaction; no restore is needed, since that transaction reads no other held text. Its rows
  are `tsvector` entries, a copy class [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d4
  already lists, so this adds no copy class; the
  [erasure inventory](../architecture/audit-and-lineage.md#the-erasure-inventory) names the
  table on its own row because its rows also enter and leave on held transitions.
  `memories_fts` and `memory_embeddings` stay promoted-only. Under RLS a row the policy does not
  match is skipped without an error, so a wrong `oml.own_key_ids` would let a removal affect
  nothing and still commit. Every commit that moves a version out of held, or reassigns a held
  version, therefore checks that its `held_fts` DELETE or UPDATE affected exactly one row, and the
  capture insert checks that it inserted one; any other count aborts the commit with
  `system.internal`. The `lifecycle.erase` copy-class list names `held_fts`.

    The PoC stores bodies inline, so it uses one port method instead:
    `JournalStore::recall_own_held(tenant, author_key_ids, readable_workareas, query, k)`, whose SQL
    hard-codes `status = 'held' AND author_key_id = ANY($ids)`. A port-contract test run against
    both stores asserts it never returns another key's row whatever the query, and a code comment
    and the README name the gap to the designed form. This amends ADR-0013 d1.

    ADR-0013 d3 prefers containment by construction to filter discipline, so that a forgotten
    predicate fails toward invisibility; on a removal the same policy fails toward retention, and
    the row-count check is what turns that into a failed commit. Designed bodies are encrypted,
    so held rows cannot be matched at query time without decrypting rows that will never be served
    ([ADR-0024](ADR-0024-performance-foundation.md) d3-d4). A plaintext-derived index of held rows
    has to exist, and it must be fenced to the author.

- **d6 — The budget unit is the budget token, counter `oml-tc/1`: ⌈body_bytes ÷ 4⌉, a
  server-defined unit, never a model's tokenizer count.** `tokens(row) = ⌈body_bytes ÷ 4⌉`, where
  `body_bytes` is the UTF-8 octet length of the row's NFC-normalized body: the clear integer
  `submission.remember` already journals ([event catalog](../data/event-catalog.md),
  event-catalog.md:69-80), which `submission.learn` and `simulation.write`, a simulation row's
  submission event, now gain, folded to `memories.body_bytes`.
  The function is pure and lives in `oml-domain`. The counter id `oml-tc/1` rides every budgeted
  response; changing the function mints a new id, and the definition never changes silently.
  The PoC has none of this today (Context): it is to normalize the body to NFC at submit, before
  the digest, the scan and the count; to journal `body_bytes`, `version` and `kind` in every
  submission event; and to backfill `body_bytes` from the inline claim in the migration that adds
  the column, so its counts equal a conformant server's for any input.

    Every served row carries `tokens` whether or not a budget was sent: steering items, hits, get
    results and own-work listing items. Only the body text is counted — never ids, paths or
    metadata — and the pages say so. No page relates the count to any model's tokenizer; that
    difference is unmeasured and opens [FR-30](../product/roadmap.md). The field names
    `max_tokens` and `tokens` stay as FR-11 fixes them.

    This amends ADR-0018 d4 with one vocabulary row:

    | Term | Means | Never |
    |---|---|---|
    | budget token | the recall budget unit under counter `oml-tc/1`, ⌈UTF-8 bytes of a claim's NFC text ÷ 4⌉ | bare "token" where the bearer key could be meant; "model token(s)"; any tokenizer's name |

    Rows the budget cuts are never decrypted (retrieval.md:171-173), so the count must exist as
    clear metadata. `body_bytes` already does, it is reproducible from the journal by a keyless
    reader, it survives erasure as `body_bytes` does, it adds no copy class, and it needs no fourth
    plugin point (ADR-0019 d4). A model tokenizer cannot run on the hot path (ADR-0024 d2), and
    the provider serves agents on many models, so any local count is a definition; naming it as
    one is the honest form (ADR-0018 d3, D17). The divisor 4 is a convention, not a measured
    ratio; clients convert with their own ratio from the per-row counts.

- **d7 — `max_tokens`: steering is never cut and is spent first, hits are the longest rank prefix
  that fits, and a budget object reports the cut.** `max_tokens` is an integer ≥ 1 on
  `RecallRequest` and `oml_recall`. Absent, there is no budget and the response carries no
  `budget` object. `max_tokens` together with `include_bodies: false` is `request.validation`.

    The cut is a pure `oml-domain` function over the steering counts S, the `k`-capped ranked hit
    counts H (own held rows included when asked) and the budget M:

    1. s = ΣS; steering is never cut;
    2. remaining = max(M − s, 0);
    3. serve the longest prefix H[0..j) whose sum is at most remaining — the first hit that does
       not fit ends the list: no skip-ahead and no partial body;
    4. truncated = (j < |H|).

    The budget object is `{max_tokens, counter: "oml-tc/1", steering_tokens: s, used_tokens:
    s + ΣH[0..j), truncated, hits_omitted: |H| − j, next_hit_tokens: tokens(H[j]) or null}`.
    `used_tokens` exceeds `max_tokens` only when steering alone exceeds it, and the object shows
    that. `k` caps the candidates before the budget applies; rows the budget cuts are never
    hydrated, decrypted or sent; counts come from `body_bytes`, and served rows hydrate in one
    set-wise statement (ADR-0024 d4). The rule is declared in a new "Token budgets" section of
    [retrieval](../architecture/retrieval.md#token-budgets), its single declaration site.

    This is FR-11's exit criterion as written: a parameter, per-hit counts, and truncation that is
    deterministic, steering rows first, then by rank. Steering is never cut because it is returned
    "in every recall of its scope" (ADR-0018 d4) and "nothing a client sends re-ranks segment 1"
    (retrieval.md:162-164). Steering past the budget is served, not refused, so the agent keeps its
    standing instructions without a retry and the overrun is visible in numbers. A strict prefix
    keeps the served set a prefix of the rank order. A budget object present only when `max_tokens`
    was sent lets a client detect a server that ignored the field.

- **d8 — Applicable steering, a total rank order, and hits that are served tips only.** Kind is
  stored; today it is validated and discarded (lib.rs:178, 204-210). **Applicable steering** for
  scope node N is every promoted, served-tip steering row that is not part of a retired claim,
  whose workarea is N or an ancestor of N, within the key's reader effective access, ordered
  structurally and never by the query: depth from the universe root ascending, then `promoted_at`
  ascending, then `artifact_id` ascending. A `self_signed` steering row applies only when N is its
  own workarea; it never applies at a descendant scope. Steering never appears in hits, and
  `kinds` filters hits only.

    **Served tip.** For a key in a simulation's audience, a claim's served tip is its overlay tip
    for that simulation where one shadows the default tip (d10), and the default tip it shadows
    is then neither a hit nor applicable steering for that key; for every other key the served tip
    is the default tip (`memories_served_tip`), and overlay tips are never served. Steering and
    hits read "served tip" in this sense.

    **Hits** are served tips (tip and state promoted) of kind `memory` or `derived_fact` — plus,
    in a workarea whose journalled policy opts the vocabulary kinds into the shared indexes
    ([ADR-0021](ADR-0021-ai-human-ontologies.md)), served tips of kind `term`, `term_mapping` or
    `vocab_release` — never retirement versions, plus own held rows under d4. A `self_signed`
    version is a hit only when the scope node is its own workarea. `kinds` still names
    only `memory` and `derived_fact`, as ADR-0021 d9 keeps it, so a recall that sends it narrows
    the hits to those kinds and one that omits it also receives opted-in vocabulary hits. Of the
    rows they admit, the serving indexes hold every promoted version, superseded ones included:
    this served-tip predicate, not index membership, keeps superseded versions, retirement
    versions and every version of a retired claim out of the hits, so a ratified retirement, or a
    later supersession of one, changes no index row. Superseded versions are readable history
    under the same access, so the predicate hides nothing a reader may not see. The order is
    total: score descending, then `artifact_id` ascending in byte order. The score is the one
    [retrieval](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked)
    declares for each store, never the fixed 0.0 at lib.rs:296-306. Determinism is guaranteed
    over a fixed candidate list; whether approximate vector search fixes that list is part of
    FR-4, and the page says so. A superseded
    version is never a hit and stays fetchable by id as history. This amends ADR-0013 d5, whose
    "in force there" was undefined.

    Neither "steering first" nor deterministic truncation can be tested without a defined
    membership and order for segment 1. Ancestor-or-self matches the OpenAPI recall example, which
    serves `universe.procurement` steering on a child scope (openapi.yaml:181-189); a descendant's
    steering belongs to another crew. Bounding steering by read access keeps it an ordinary read,
    and serving tips only (audit-and-lineage.md:130-131) is what lets supersession and retirement
    take a claim out of recall. Reading the overlay as the audience's served tip keeps one claim
    to one hit for every key, so the `k` cut, the budget cut and the read-log ids cannot differ
    between servers on that account, and one equality test for `self_signed` rows — the same the
    events tail applies (d15) — keeps steering, hits and the tail agreeing about where such a row
    is served.

- **d9 — A recall logs exactly the rows served and a digest of the whole request.** A recall's
  read-log entry records the artifact ids actually served after the `k` and budget cuts — own held
  ids included, since an author reading its own rows is still a read — and `query_digest` =
  SHA-256 over the RFC 8785 JSON of an object whose members are always present: `query` (the NFC
  string), `k` (default 8 applied), `kinds` (sorted ascending by Unicode code point, equivalently
  UTF-8 byte order, duplicates removed; `[]` when absent), `include_bodies` (default `true`
  applied), `include_own_held` (default `false` applied), `max_tokens` (`null` when absent),
  `terms` (sorted ascending by Unicode code point, equivalently UTF-8 byte order, duplicates
  removed; `[]` when absent), `expand_terms` (default `"none"` applied) and `scope` (the resolved
  node id). A sent empty `kinds` or `terms` is `request.validation` (d16), so `[]` stands only for
  absence. This resolves http-api.md:354-355 against mcp-plugin.md:309-310: both are logged. The
  read log itself stays unbuilt (migrations/0001_init.sql:9-10); the PoC is to compute
  `query_digest` by this rule and the app to return the served id set, so a future logger has one
  input. This amends ADR-0013 d2: "ids or a digest" becomes "ids and a digest".

    "Who read what" is false if the log records rows the budget cut, or omits own held rows. A
    digest of the query text alone (lib.rs:316) cannot tell apart two different requests, and an
    omitted field that one server digests as absent and another as its default would give one
    request two digests. Every member is fixed, so two requests share a digest only when they
    differ solely in the order or repetition of `kinds` or `terms` entries, and one request gets
    one digest on every server; the served ids, logged beside it, record what each read
    returned. RFC 8785 sorts object members, not array elements, so the array order is fixed by
    code point here: a locale collation would give `proc:Zinc` and `proc:alloy` different orders
    on different servers.

- **d10 — Supersession is a held version that names what it replaces; the tip moves only at
  ratification, by compare-and-set, as one `lifecycle.supersede`.**
  `POST /v1/memories/{artifact_id}/supersede` admits a held successor of the target's claim, in
  the target's workarea, with the same `claim_key` and `derives_from` ⊇ {target}, journalled as
  `submission.remember` (`submission.learn` when the successor's kind is `derived_fact`, and
  `simulation.write`, whatever its kind, when the successor is a simulation row) with the
  catalog payload plus `replaces {artifact_id, version}` and `retirement`. The target stays the
  served tip and stays in everyone's recall; review serves the successor with the version it
  replaces ([ADR-0012](ADR-0012-admission-and-review.md) d5).

    **Ordinals.** Each version has its own `artifact_id`, and `version` is its ordinal within the
    `claim_key`. A new claim's first version is 1. A successor's version is 1 + the greatest
    `version` of any row of that `claim_key` in the workarea, whatever its state, so ordinals grow
    past rejected versions and past a tip that `lifecycle.tip_override` moved back to an earlier
    version. `replaces` names the target; the ordinal says only where the version sits in the
    claim's history. The ordinal is read before `BEGIN`, like every other deterministic input of a
    capture ([ADR-0024](ADR-0024-performance-foundation.md) d1). The per-tenant chain lock
    serializes commits, and the `memories_claim_version` index refuses a capture whose ordinal a
    concurrent capture took first; the server retries that capture, so the race never surfaces as
    a constraint error. The retry re-runs every pre-`BEGIN` read of the capture, the `claim_key`
    collision test of d18 included, so a version-1 capture that lost the race answers
    `submission.claim_key_conflict` and is never renumbered; only a successor takes a fresh
    ordinal. Several held successors of one tip may therefore coexist — an author's revision
    beside a dreamer's edit-as-proposal, say — and none blocks another. The stated residual: a
    successor's version ordinal, returned to its proposer and shown on every version, counts every
    earlier version of the `claim_key` in any state, so a key learns how many versions it cannot
    read (held, rejected or demoted, by any author) precede it — a count, never content. It is
    tracked with the `claim_key` collision of d18 as [FR-31](../product/roadmap.md).

    **Ratification.** Ratifying a successor compare-and-sets that its target is still the promoted
    tip under the successor's own workarea and `claim_key`, then journals one
    `lifecycle.supersede` (the catalog shape plus `retirement`, `replaces_artifact_id` and
    `self_signed`), which flips the tip in the same commit and carries the successor's
    `self_signed` mark exactly as `lifecycle.promote` carries a first version's (ADR-0001 d5).
    `lifecycle.promote` is journalled for first versions only, with `previous_tip_version` null.
    The compare-and-set decides between competing successors: once one is ratified, a verdict that
    would ratify another is refused with `submission.tip_conflict`, nothing is journalled, the
    serve is not consumed, and that successor stays held until a reviewer rejects it; its author
    may propose afresh against the new tip, and the fresh proposal leaves the stale one held. A
    superseded version reads state `promoted`, tip `false`; the PoC's "superseded" status is
    migrated away.

    The compare-and-set runs inside round trip two's data-modifying-CTE statement (ADR-0024 d1),
    never before `BEGIN` as the ordinal is read, and it binds the target to the successor's claim
    identity, read from the successor's row in that statement: it is the conditional `UPDATE
    memories SET is_tip = false WHERE artifact_id = $target AND workarea_id =
    $successor_workarea_id AND claim_key = $successor_claim_key AND is_tip AND state =
    'promoted'`, and every other write of that statement — the journal insert, the fold that
    promotes the successor and makes it the tip, the `held_fts` DELETE of d5, the outbox rows and
    the serve's consumption — is conditioned on the row it returns. A zero row count therefore
    writes nothing: the transaction rolls back and the verdict answers `submission.tip_conflict`.
    The `held_fts` DELETE's affected-row count is asserted in that same statement (d5), so a
    competing ratification can neither trip the one-served-tip index as a constraint error nor
    flip a stale tip. A target moved to another workarea or re-keyed after the successor was
    captured fails the compare-and-set, and so does a successor moved or re-keyed away from its
    target: the verdict answers `submission.tip_conflict`, nothing is journalled, and the
    successor stays held until a reviewer rejects it. A ratified supersession therefore always
    joins two versions of one workarea and one `claim_key`, so the one `workarea_id` of its
    `lifecycle.supersede` names both.

    **Tainted successors.** A successor that is a simulation row or sim-derived never takes the
    default tip ([simulations](../architecture/simulations.md#never-the-default-tip)). Its
    ratification compare-and-sets the same target under the same predicate, the successor's
    workarea and `claim_key` included, in the same statement, and journals
    `lifecycle.supersede`, but it sets that simulation's audience overlay tip
    (`memories_overlay_tip`) and never the default tip: a target that is the default tip is
    compared without being flipped, so it stays the default tip for every key, and only an overlay
    target is flipped. Tainted and untainted successors of one tip therefore do not compete under
    the default-tip compare-and-set — an untainted successor can still be ratified after a tainted
    one — and an overlay whose target stops being the default tip stops shadowing it. Every `tip`
    a read serves is computed for the calling key, audience-scoped: a key in the simulation's
    audience sees the overlay as the claim's tip and the default tip it shadows with `tip: false`,
    and every other key sees the default tip and never an overlay.

    **Who may supersede.** Own is evaluated against the target version's authoring key — the
    current tip's `author_key_id` (d3) — never against the claim's first version. The rule runs in
    this order:

    1. a key whose role admits neither writer nor dreamer → `access.verb_ceiling`, before any
       lookup (d1, step 1);
    2. the target must be readable by the caller — its own row in any state, or promoted within
       its reader effective access — else `access.not_found`, indistinguishable from a
       nonexistent id (d3);
    3. a writer key needs writer on the target's workarea when the target is its own, resolved by
       the refusal order of d1; a target another key authored answers `access.author_only`;
    4. a dreamer key needs dreamer and reader on the target's workarea, resolved by the refusal
       order of d1, whoever authored the target — its own ratified edits included
       (edit-as-proposal, http-api.md:215);
    5. only then the refusals that depend on the target, in this order: an `X-OML-Workarea`
       equal to the target's workarea, else `request.validation`; the target is the current
       served tip, else `submission.tip_conflict`; the kind and retirement checks of d11, and on
       `oml_supersede` the vocabulary-kind check of d12, else `request.validation`; the workarea
       is not `validated`, else `workarea.frozen`. The checks every submit runs follow.

    No target-dependent refusal is answered before step 2, so none of them tells a caller that an
    unreadable target exists, or hints at its workarea, kind or state.

    A writer whose version was superseded by another key's ratified version therefore no longer
    owns the claim: the tip is the other key's, and the writer's supersede answers
    `access.author_only`. R2 keeps every author in the lineage closure off the review.

    **Rival successors.** While a successor is held, the reviewer-exclusion set of every other
    held successor of the same target (same workarea, `claim_key` and `replaces`) also contains
    its author's key and principal; review next never serves such a row to that key, and a
    verdict from it answers `review.author_in_lineage`. The rule is declared with the
    lineage-closure exclusion, on
    [roles and keys](../architecture/roles-and-keys.md#the-dreamer-split).

    **The target.** The target must be the current served tip, else `submission.tip_conflict`
    with `details.current_tip {artifact_id, version}` when the caller can read it; superseding a
    held version is not admitted ([RS-17](../product/roadmap.md)), and
    [admission](../architecture/admission.md) (admission.md:65-66) is edited to match
    http-api.md:386-387. The scope is the target's workarea: an `X-OML-Workarea` that differs from
    it is `request.validation`. `Idempotency-Key` is stored and honoured, scoped to (key, nil
    workarea, idem_key), as lab endpoints that name no workarea already are, so no target's
    workarea ever enters a record. The record is looked up after step 1 and before step 2: a
    committed record with the same request digest replays its stored response whatever the
    target's state or the key's access now — the target erased or reassigned, a reader grant lost
    — because that response names only the caller's own successor and discloses nothing new; a
    record `in_progress` answers `idempotency.in_progress`, and a different digest
    `idempotency.digest_mismatch`. With no record, step 2 runs, and the record is claimed only
    after it passes, so a refusal at step 1 or 2 claims none. The request digest of every
    idempotency record covers the method, the route template and the path parameters as well as
    the body ([HTTP API](../api/http-api.md#idempotency)), so one key reused for two targets is
    `idempotency.digest_mismatch`, never a replay of the first. A committed supersession is
    therefore replayable exactly once by construction, as D13 promises every side-effecting
    request.

    The self-signed tip rule is d17, and the `claim_key` release and the reopen refusal are d18.

    The tip never changes except by a ratified promotion (audit-and-lineage.md:138-141), and no
    tool's success changes what other keys read except through admission (mcp-plugin.md:107-110);
    the PoC's proposal-time flip is a retirement nobody reviewed, and it goes before `oml_supersede` ships.
    One event per flip is why `lifecycle.supersede` exists (event-catalog.md:290-291). Distinct
    ordinals let competing proposals coexist, so no held row — which may wait unreviewed for as
    long as held-item expiry stays unbuilt — blocks another key's proposal, and a capture never
    refuses on a readable target because of a held row the caller cannot see. Evaluating own on
    the target version keeps own a property of one row (d3). Returning the current tip in the
    refusal saves the agent a lookup. Reassignment moves one version at a time and may re-key it,
    so a compare-and-set keyed on the target's `artifact_id` alone would let a verdict in one
    workarea retire a tip another workarea serves, through an admission that workarea's review
    never saw (R1, R3); binding it to the successor's workarea and `claim_key` keeps every tip
    flip inside the workarea whose reviewers ratified it. Competing successors are a choice
    between rows, and the compare-and-set lets the first ratified one win, so a rival's author
    judging the other successor would be deciding its own row's fate: rejecting the rival clears
    the way for its own, which another reviewer may then approve. Keeping rival proposers off
    each other's review keeps R2 true of that choice, at the cost of a smaller reviewer pool
    while rivals are held.

- **d11 — The author's retire-proposal is a supersession carrying `retirement: true`.** An author
  proposes retiring its claim with `POST /v1/memories/{artifact_id}/supersede {body, retirement:
  true}`. The body is required as for any supersession and states why the claim is retired; it is
  scanned, NFC-normalized, bounded by `submission.max_body_size` and digested like any body, and it
  is what the reviewer judges. An omitted `kind` makes the successor take the target's kind, for
  every supersession, and `kind` is the only field a successor inherits: an omitted `attachments`
  or `terms` leaves it with none. A sent `kind` changes only within its class — a `memory`,
  `derived_fact` or `steering` target admits any of those three, a vocabulary-kind target keeps
  its kind — and anything else is `request.validation`; the rule is declared on
  [audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).
  `retirement: true` is refused with `request.validation` when the target's kind is `term`,
  because terms deprecate ([ADR-0021](ADR-0021-ai-human-ontologies.md) d3), or
  when a `kind` is sent that differs from the target's; a `validated` workarea refuses it with
  `workarea.frozen`, as it refuses every write, and an admin's demote is the path there.

    Who may propose retirement is exactly who may supersede (d10): a writer key on a tip it
    authored, and a dreamer key with dreamer and reader on the target's workarea on any readable
    tip, through edit-as-proposal — so a claim whose tip a dreamer's ratified edit took is still
    retirable by a non-admin. The author's retire-proposal is the writer path of that rule.

    It inherits everything d10 decides — the own and edit-as-proposal rule, the served-tip target
    rule, held arrival, the review serve with `replaces`, non-author review
    over the lineage closure, the tip compare-and-set at ratification — and journals
    its submission event (d10) with `retirement: true`, then `lifecycle.supersede` with
    `retirement: true`. Once ratified, the claim's tip is the retirement version and the claim is a
    retired claim: recall serves nothing of it, neither hits nor steering; get still answers for
    every version as history (state `promoted`, tip `false`; the retirement version, tip `true`);
    nothing is demoted or erased (R4); and a later ratified supersession of the retirement version
    returns the claim to service.

    New surface: `SupersedeRequest.retirement`, `MemorySummary.retirement`, `memories.retirement`
    (a structural column, like `term_status`, that survives shredding), and the `retirement` field
    on `submission.remember`, `submission.learn`, `simulation.write` and `lifecycle.supersede`.
    ADR-0010 d1's writer verb list and [ADR-0022](ADR-0022-two-ring-api.md) d2 are unchanged:
    writers "propose supersessions of their own claims". This amends ADR-0018 d4 with two
    vocabulary rows:

    | Term | Means | Never |
    |---|---|---|
    | retirement version | a ratified version that takes a claim's tip so that recall stops serving the claim; bytes and history are kept | "deleted", "archived", "hidden", "withdrawn" |
    | retired claim | a claim whose served tip is a retirement version | "deleted", "archived" |

    Retirement repeats the design's only precedent for retiring content — "a deprecation IS a
    supersession" (ADR-0021 d3; openapi.yaml `SupersedeRequest.term_status`) — so it inherits
    every check supersession has, with no second implementation: R1 (it only proposes), R2, R3
    (visibility changes at ratified promotion only), R4, the compare-and-set and the serve binding.
    It is journalled as a named field on a submission and on the flip, so an auditor's tail and the
    evidence export show it, and it can be undone. An inherited attachment would bind digests
    another key uploaded, which a submit refuses (`submission.attachment_missing`), and inherited
    terms would annotate a body nobody re-cited; a kind change among the non-vocabulary kinds is
    what `submission.learn` for a `derived_fact` successor already implies (d10), while a
    vocabulary kind carries structural fields no other kind has.

- **d12 — The MCP catalog grows from ten to twelve tools with `oml_get` and `oml_supersede`; one
  claim-item shape.** The agent-safe tool list of ADR-0014 d2 adds "get by id", `oml_get` (one
  `GET /v1/memories/{artifact_id}`), and "supersede, retirement included", `oml_supersede` (one
  `POST /v1/memories/{artifact_id}/supersede`). `oml_recall` gains `include_own_held`,
  `max_tokens` and a maximum of 100 on `k`, matching the ring; `oml_my_work` gains a `claim_key`
  filter, an existing ring filter. Every state-creating tool requires `idempotency_key`, and inputs
  are flat, with no conditionally required fields, so `oml_supersede` carries no vocabulary
  fields and refuses a target of a vocabulary kind (`term`, `term_mapping`, `vocab_release`) with
  `request.validation`; such a target is superseded on the HTTP API. `oml_supersede` carries the
  optional `simulation_id` and `simulated_time` pass-throughs that `oml_learn` carries, so a target
  or lineage that touches a simulation row can be superseded on MCP, the ring refusing an omitted
  `simulation_id` with `submission.simulation_id_required` as on every submit.

    After this iteration eight tools are served: remember, learn, recall, get, supersede, my_work,
    workarea_info and events_tail. `oml_attach`, `oml_relate_propose`, `oml_graph` and `oml_vocab`
    stay unbuilt, unlisted in `tools/list`, and answer "not built".
    [The agent plugin](../api/mcp-plugin.md#the-claim-item) declares one claim-item shape: base
    fields `{artifact_id, claim_key, version, kind, state, tip, retirement, replaces, workarea,
    workarea_path, tokens, created_at, promoted_at}`, with `workarea` and `workarea_path` null
    under d3's naming rule, `replaces` null where the key cannot read the replaced version (d3),
    and `tip` computed for the calling key, audience-scoped (d10), plus `text` in recall and get,
    `score` on hits only,
    `held_expires_at` in my_work, and `derives_from` (readable ancestors only, d3) and `attachments`
    in get. Recall steering items use the same shape, carrying `text` in place of the ring's `body`.
    A test asserts that every property an advertised `inputSchema` declares is accepted by the
    tool's argument struct. The two new tools, the `oml_recall` inputs, the `created_at` rename of
    `oml_my_work` (d13) and the `oml_events_tail` input and output (d15) change the shapes that
    ADR-0022 d6 says the ten agent-safe tools keep. This amends ADR-0014 d2 and ADR-0022 d6.

    Both tools pass mcp-plugin.md:107-110: `oml_get` is a read that fails closed as absence, and
    `oml_supersede` succeeds only into admission. Both endpoints exist on `/v1` (ADR-0022 d2), so
    ADR-0014 d1 holds. One item shape makes recall, get and my_work interchangeable to an agent;
    `tools/list` is the served subset of [mcp-tools.json](../api/mcp-tools.json)
    (crates/transports/oml-transport-http/src/mcp.rs:41), so the agreement test stops the server
    advertising a field it refuses.

- **d13 — The own-work listing covers everything the key wrote; the scoped listing serves
  promoted tips only.** `GET /v1/memories?own=true` lists every row the calling key authored, in
  every state, across all workarea nodes minus globally denied subtrees. It needs no grant and
  admits any tenant role; an operator key is refused with `access.verb_ceiling`, and the adapter's
  provider-key path (crates/adapters/oml-pg/src/lib.rs:766-775) is fixed so that the refusal is
  reachable rather than an internal error. `workarea_id` is a subtree filter: with `own=true` it
  must name a node the key holds some role on — one d3's naming rule names to it — else
  `access.not_found`, indistinguishable from a nonexistent node, and it then narrows the listing
  to the key's rows in that subtree whatever the key holds below it; without `own` it must name a
  node where reader is effective, else `access.not_found`, and the listing is the intersection of
  its subtree with the scope's, the scope still resolving from the token. `claim_key`, `state`,
  `kind`, `since` and `until` also filter. The own-work listing and get resolve no workarea, so an
  `X-OML-Workarea` header sent with either is `request.validation`. The order is `created_at`
  descending, then `artifact_id` descending. The cursor is an opaque keyset position bound to the
  key id and expiring under `api.cursor_ttl`; a foreign or stale cursor answers
  `request.validation`, and every page re-applies the predicates. Without `own`, the listing returns
  promoted served tips within subtree(scope) ∩ reader access — a `self_signed` version only when
  the scope node is its own workarea (d8) — as summaries only, and never another key's held row;
  a retired claim's tip, its retirement version, is among them, flagged `retirement: true`. This
  amends ADR-0022 d4: get and the own-work listing refuse the `X-OML-Workarea` override instead
  of honouring it, supersede accepts it only when it equals the target's workarea (d10), and none
  of the three answers `request.workarea_unresolved`.

    `MemorySummary` gains `tokens`, `retirement`, `replaces` and `held_expires_at` (null while
    held-item expiry is unbuilt, and stated), and its `workarea_id` and `workarea_path` become
    nullable under d3's naming rule. `oml_my_work` uses `created_at`; `submitted_at` at
    mcp-plugin.md:633,642 is renamed to it. This resolves http-api.md:360-362 against
    mcp-plugin.md:620 in favour of the agent's promise, as a page edit recorded here. An agent
    writing into two areas would otherwise miss half its own work, and every field comes from the
    ring, so ADR-0014 d1 holds. Refusing the header where nothing resolves a workarea keeps
    "refused, not coerced" (errors.md:78), and listing the retirement tip keeps "served tips"
    literal while showing a reader what get already shows: that the claim is retired. A
    `workarea_id` that only narrows leaves ADR-0022 d4's scope resolution as it is, and refusing
    one the key could not be told about keeps the filter from confirming that a node exists.

- **d14 — The workarea read carries the caller's children, effective verbs and permission epoch in
  one ring call.** `GET /v2/workareas/{workarea_id}`, at reader effective access and otherwise
  `access.not_found`, returns the workarea plus `children` (the immediate children the key can
  read, as `[{id, path_leaf, status}]`, ordered by `path_leaf` ascending in byte order, then
  `id`), `edges` (the workarea edges touching the node whose other endpoint the key can read, in
  either direction, as `[{edge_id, from, to, label, state}]` ordered by `edge_id` ascending, so
  each matches the `edge_id` the events tail reports; `[]` while no workarea edge can exist in
  the build), `my_effective_verbs` and `permission_epoch`. `path` is derived by walking `parent_id`
  names from `universe`, joined with `.`; the same derivation fills `workarea_path` everywhere
  (null today at mcp.rs:337). `crew` is persisted at `workarea.create`, and returned.

    `my_effective_verbs` is a closed set declared once, on
    [roles and keys](../architecture/roles-and-keys.md#effective-verbs): reader confers `recall`,
    `read`, `graph_read`, `events_tail`; writer confers `remember`, `learn`, `attach`,
    `supersede`; dreamer confers `propose`, `review`, `supersede` — a dreamer's supersede is
    edit-as-proposal and also needs reader on the node (d10). The field is the union over
    `effective_roles(node)`, listed in table order (the reader, writer and dreamer rows, each
    left to right) without duplicates, and for a `held_by: service` key without `review`, which
    d1 refuses it; it states the access conferred, not what this server serves — `tools/list`
    states that.

    `permission_epoch` is the journal seq of the latest `key.issued`, `key.rotated` (for its
    `new_key_id`), `key.revoked`, `grant.issued`, `grant.revoked`, `key.denied` or
    `key.denial_lifted` event for the key; a workarea re-parent joins this list when re-parent is
    built. Grant and denial rows store their own event's seq as their epoch; a successor minted by
    `key.rotated` has no `key.issued` and no `grant.issued`, so its `api_keys.issued_seq` and the
    epoch of each grant row reproduced for it are the `key.rotated` seq. Effective access is
    recomputed on every request, so the epoch labels the state; it invalidates no cache
    ([workareas](../architecture/workareas.md) owns the reading). This amends ADR-0008 d4 and d6:
    effective access is computed on every request, no resolution is cached, and the epoch labels
    the journalled state an answer was computed against without invalidating anything — a
    re-parent joins the epoch as a label, not as a cache flush. A revocation, a grant change or a
    denial therefore applies to the key's next request served by the lead. A follower resolves
    keys, grants and denials from its local replica, so on follower-served reads a revocation, a
    grant change or a denial takes effect after replication lag — the same follower metric that
    bounds promoted reads ([ADR-0024](ADR-0024-performance-foundation.md) d9) — and
    [security](../architecture/security.md) states that window as the elevation-of-privilege
    residual. A lead-only deployment, the default, has no such window.

    `oml_workarea_info` with no workarea, and `oml_events_tail` with no scope, take the key's
    journalled `default_workarea` from the authenticated key record (ADR-0022 d4, "the token's
    default") and make one ring call; a key with no default is refused with
    `request.workarea_unresolved`.

    mcp-plugin.md:687-721 requires these fields, and ADR-0014 d1 forbids facade-only fields, so the
    ring must carry them; the only effective-access read today is admin-only and returns roles, not
    verbs (openapi.yaml:1705-1734). A tenant's journal seq only increases and is reproducible from
    the journal. The verb names already appear in the mcp-plugin.md:706 example and are now
    declared once (ADR-0018 d5).

- **d15 — The events tail shows only what the key could read, under a fixed rule per event family,
  with a bounded scan and a resume point.** `GET /v2/events` gains an optional `workarea_id`: a
  node id filtering to that subtree within reader effective access, else `access.not_found`;
  omitted, the key's default workarea, or `request.workarea_unresolved` for a key with none.
  Visibility depends only on the journal up to the event and on the key's current access, never
  on later row state, and is declared once, on the [HTTP API](../api/http-api.md#events-tail):

    | Family | Who sees it |
    |---|---|
    | `lifecycle.*` | a key for which (a) or (b) holds, and no other. **(a)** The key has reader access on the subject's workarea, and three conditions hold: *promoted* — in the journal up to and including the event, the subject version's latest transition between held and promoted is a promotion (`lifecycle.promote` or `lifecycle.supersede`), not a `lifecycle.reassign` with `re_enters_held: true` or a `review.reopen`, so a version never promoted fails it, and a `lifecycle.reassign` is judged on the journal before it; *audience* — the subject is not a simulation row or sim-derived version outside the key's audience; *self-signed* — the subject is not `self_signed` unless the tail's `workarea_id` is the subject's own workarea. `lifecycle.reassign` names two workareas: its subject's workarea is `from_workarea_id`, and `to_workarea_id` when `re_enters_held` is false; (a) holds when the key has reader on either listed workarea and that workarea lies within the tail's subtree, and the self-signed condition compares the tail's `workarea_id` with that same workarea. **(b)** The key's own key set (d3) holds the subject version's authoring key, read as the version's submission event (`submission.*` or `simulation.write`) says; the subject's workarea — for a reassign, either listed one — lies within the tail's subtree minus the key's global denials; and the subject is not a simulation row or sim-derived version outside the key's audience (d3) |
    | `workarea.*` | keys with reader access on the subject workarea: the node itself, or, for `workarea.relate_*`, both endpoints, at least one of which lies within the tail's subtree |
    | `submission.*`, `simulation.write` | only a key whose own key set (d3) holds the subject version's authoring key, `memories.author_key_id` — for `submission.*` and `simulation.write` the envelope `api_key_id`, for `lifecycle.*` the `api_key_id` of the subject's submission event, read from the journal — within the tail's subtree minus its global denials, and only while the subject is not a simulation row or sim-derived version outside the key's audience, with no reader grant needed on the event's own workarea (the own-work rule of d3) |
    | `review.*`, `claim_edge.*`, `key.*`, `grant.*`, `tenant.*`, `system.*` | no key on the content tail; they belong to the evidence export |

    The `lifecycle.*` conditions keep the tail inside what the read path serves: the erase of a
    held or never-promoted version never reaches another key (R3); a simulation row's promote or
    demote never reaches a key outside its audience
    ([simulations](../architecture/simulations.md), D10); and a `self_signed` row's events reach
    only a tail scoped where the row is served ([retrieval](../architecture/retrieval.md)). So
    that the filter is computed from the journal alone, `lifecycle.demote`, `lifecycle.erase` and
    `lifecycle.reassign` gain the subject's `workarea_id` (on reassign, the existing
    `from_workarea_id` and `to_workarea_id`), `claim_key` and `version`; `lifecycle.tip_override` gains
    `from_artifact_id` and `to_artifact_id`; and `workarea.relate_ratified` gains `src_id` and
    `dst_id`. The subject's own earlier events — its submission, and the `lifecycle.promote` or
    `lifecycle.supersede` that promoted it — supply its simulation, taint and `self_signed`
    marks. A claim-subject workarea id in a served payload — `workarea_id`, `from_workarea_id`,
    `to_workarea_id` — that d3's naming rule withholds is replaced by null; `workarea.*` payloads
    serve `parent_id`, `old_parent_id` and `new_parent_id` as the workarea read serves
    `parent_id`, within the exception d3 states. An event served to a key outside
    its actor's own key set — the actor being the envelope's `api_key_id` — carries `principal`
    and `api_key_id` as null, and every payload field naming a key (`proposer_key_id`,
    `ratifier_key_id`, each `*_by_key_id`) as null; the evidence export carries the identities.
    The stated residual: `seq`, `head_seq`, `next_after_seq` and `permission_epoch` (d14) are
    tenant-wide journal positions, so a key learns how many events it cannot see occurred
    between two it can, and when — a count, never content, tracked as
    [FR-31](../product/roadmap.md).

    An event type the table does not map is not shown. One call scans at most `limit` raw events
    from `after_seq + 1`. `next_after_seq` is `after_seq` when no raw event follows it, and
    otherwise min(`after_seq` + `limit`, `head_seq`), with `head_seq` read in the snapshot the
    scan reads — the last raw seq scanned, since a tenant's seqs have no gaps. The client passes
    it back verbatim; `after_seq` greater than `head_seq` is `request.validation`. The MCP output
    is `{events: [{seq, event_type, at, subject, workarea}], head_seq, next_after_seq, idle}`,
    where `subject` is `{artifact_id, claim_key, version}` for `lifecycle.*`, `submission.*` and
    `simulation.write` (for `lifecycle.supersede` the new version, for `lifecycle.tip_override`
    the version that becomes the tip), `{edge_id, from, to}` for `workarea.relate_*`, and
    `{workarea_id}` for every other `workarea.*`; `workarea` is always present and names the
    in-scope workarea — the subject's workarea; for `workarea.relate_*`, the edge's `from`
    endpoint when it lies within the tail's subtree, else its `to` endpoint; for
    `lifecycle.reassign`, `to_workarea_id` when `re_enters_held` is false and that workarea
    satisfies a clause that makes the event visible (the reader clause, with reader on it within
    the tail's subtree, or the author clause, with it within the tail's subtree minus global
    denials), else `from_workarea_id` — null where d3's naming rule withholds it — and
    `idle = (next_after_seq == head_seq)`.
    Hashes, signatures and payloads are dropped. On MCP an omitted `after_seq` means 0, as on the
    ring — a page edit to mcp-plugin.md:805-806.

    The PoC tail is tenant-wide (app.rs:780-794): it would show a writer+reader key every other
    writer's held submissions and every reviewer's key id, an R3 breach (mcp-plugin.md:787-789).
    Fixed per-family rules show two keys with equal access the same `lifecycle.*` and
    `workarea.*` events in the same order, each additionally seeing only its own `submission.*`
    and `simulation.write` events and, under (b), the `lifecycle.*` events of its own versions
    (mcp-plugin.md:833-836),
    and a bounded scan with a resume point bounds the work per call and cannot loop. Judging
    promotion by the latest transition, not by any earlier one, keeps the later events of a
    version an admin moved back into held out of its new readers' tails; judging a reassign on
    the journal before it lets the readers it takes a promoted version from see it leave.
    Nulling identities keeps the tail from naming the reviewer that `oml_my_work` and the
    review queue never name. Mapping `simulation.write` with the submission family gives a
    simulation row's author its own submission and gives the author clause a source for that
    row's authoring key; naming `to_workarea_id` only when the event is visible through it keeps
    `workarea` from naming a node that a per-grant deny hides while the event shows through
    `from_workarea_id`.

- **d16 — Request bodies refuse what they do not serve; refusals carry one-hop recovery details;
  no new codes.** The recall, supersede, key-issuance and grant bodies refuse unknown fields
  (`additionalProperties: false` in OpenAPI, `deny_unknown_fields` in serde), nested objects
  included; the submit body stays open. The supersede body therefore declares every submission field
  a successor can need — `simulation_id` and `simulated_time` for a target or lineage that touches a
  simulation row, and `mapping` for a `term_mapping` successor — beside its own, so closing it
  refuses nothing a supersession must carry. `terms` and `expand_terms` are parsed and refused by
  name with `request.validation` while unbuilt; `kinds` is honoured, and an empty `kinds` or `terms`
  array is `request.validation`. Refusals carry details: `access.grant_refused` → `{workarea_id,
  needed_role}`; `access.issuer_ceiling` → `{role, admissible}`, plus `grant_index` for an
  inadmissible grant at issuance (d2); `submission.tip_conflict` → `current_tip {artifact_id,
  version}`, present only when the caller can read it. The taxonomy is unchanged.

    Today a recall carrying `max_tokens` or `include_own_held` gets a silently unbudgeted,
    promoted-only answer (lib.rs:756-765) — the coercion errors.md:78 forbids. The details let an
    agent correct course without a lookup call.

- **d17 — A self-signed version never replaces a reviewed tip.** In a `development` workarea
  whose policy permits self-signing ([ADR-0001](ADR-0001-the-constitution.md) d5), a
  `self_signed` version may take the tip only as a claim's first version or from a `self_signed`
  tip. A supersession or retirement (d10, d11) whose target was promoted by non-author review
  needs full non-author ratification even there: the review policy does not admit self-sign for
  that row, which waits for a non-author verdict like a row in a `committed` workarea. The rule is
  declared on
  [audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).

    A self-signed replacement of a reviewed tip would retire or replace reviewed content with no
    second identity, and since `self_signed` rows are served only to reads whose scope node is
    their own workarea ([retrieval](../architecture/retrieval.md)), every recall and listing
    scoped at an ancestor would lose the reviewed version and see nothing in its place. ADR-0001
    d5 scopes the development exception and makes any change to it a new decision; under this one
    the exception no longer reaches a tip that non-author review promoted. This amends ADR-0001
    d5.

- **d18 — A `claim_key` is released by a never-promoted rejection, and a reopen that would collide
  is refused.** A new claim (version 1) is refused with `submission.claim_key_conflict` while a
  version of that `claim_key` in its workarea is held, promoted or demoted — held now, or
  promoted at some point and not erased. A version 1 rejected without ever being promoted
  releases the key (admission.md:327-330), and so does a claim whose every version is rejected
  or erased. A demoted claim keeps its key, because a demoted version can return to service: a
  sim-derived row that survives re-review is un-demoted
  ([simulations](../architecture/simulations.md)). So no two held, promoted or demoted versions
  of one `claim_key` ever share an ordinal: the `memories_claim_version` index covers exactly
  those three states, and (`claim_key`, `version`) — `lifecycle.tip_override`'s `to_version`
  among its uses — names one version. An erased chain keeps its `artifact_id`s in the journal
  and the evidence export beside any fresh chain under the released key. The cost: a claim
  whose every version is demoted holds its `claim_key` until it is erased or returns to
  service, so its replacement starts under another `claim_key`. Reopening a rejected first
  version is refused with `submission.claim_key_conflict` under the same test, so a reopen never
  puts a second live first version beside a fresh one. A reopened successor needs no such test,
  since the compare-and-set of d10 guards the tip; any reopen whose ordinal a held, promoted or
  demoted version of its `claim_key` already carries is refused with the same code, so a reopen
  never trips the `memories_claim_version` index. The rule is declared on
  [audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).
  The stated residuals: the collision tells any writer on the workarea that a held version, or
  a promoted or demoted one it cannot read, holds that `claim_key` — a writer can learn that a
  name is taken without reading anything under it — and the ordinal counts versions the key
  cannot read (d10, Ordinals). Both are tracked as [FR-31](../product/roadmap.md).

    Releasing the `claim_key` after a rejection delivers the resubmission the design promises
    without ever creating two promoted first versions, and refusing the colliding reopen keeps
    (`claim_key`, `version`) naming one live version. Under ADR-0012 d7 a non-author's reopen had
    no refusal of its own. This amends ADR-0012 d7.

## Consequences

**Agent-facing results**

- **One key.** A writer-role key with `wa:writer` and `wa:reader` grants and default workarea `wa`
  remembers, recalls, reads back, supersedes, proposes retirement, lists its own work, inspects
  its workarea and tails its events. The Claude Code plugin therefore needs one connection:
  `OML_RECALL_KEY` and the `oml-recall` server go away when the code lands.
- **FR-10 and FR-11** close against their exit criteria once the agent-only-memory scenario passes
  end to end in CI; until then they stay open on the [roadmap](../product/roadmap.md)
  (ADR-0018 d3).
- **Retired claims** need no new state: a retired claim leaves recall, and every version stays
  fetchable as history.

**Opened and recorded**

- **Opened**, each with its exit criterion on the [roadmap](../product/roadmap.md):
  [FR-30](../product/roadmap.md) — the budget-token counter `oml-tc/1` is not any model's count,
  and how far the two differ is unmeasured; [RS-17](../product/roadmap.md) — an author cannot
  revise its own claim before it is reviewed; [FR-31](../product/roadmap.md) — the `claim_key`
  collision of d18 tells a writer that a held or unreadable version holds a name, a successor's
  version ordinal counts the versions its key cannot read, the tenant-wide journal positions
  of the events tail and the permission epoch (d14, d15) count the events a key cannot see, and
  the lineage read names a readable version's unreadable ancestors, its author's held rows
  included, as redacted bare ids (d3); [FR-32](../product/roadmap.md) — a `held_by: service`
  key's dreamer grant confers every dreamer verb, so a stolen reconciler key can propose within
  its grants, though never review or ratify (d1).
- **Resolved by page edit and recorded here:** `derives_from` readability (d3); the `own=true`
  default scope (d13); the issuer of `held_by: service` keys, a tenant admin rather than the
  operator (d2); the MCP `after_seq` default (d15); what a recall logs (d9);
  admission.md:65-66 brought into line with http-api.md:386-387 (d10); the `claim_key` release
  after a rejection and the reopen that would collide with it (d18); the revocation latency that
  roles-and-keys, security, the event catalog, compliance and commitments described as a cache
  window, restated as the key's next request on the lead and replication lag on follower-served
  reads (d14); `auth.epoch_cache_bound` removed with the cache it bounded (d14); FR-10's exit
  criterion reworded to the retire-proposal d11 decides.

**Surface and versioning**

- `/v1` request and response shapes change, so `info.version` moves from `0.1.0-design` to
  `0.2.0-design`. No operation, error code, event type or claim state is added;
  `scripts/check_contract_sync.py` holds the three contract surfaces in agreement. The
  `lifecycle.demote`, `lifecycle.erase`, `lifecycle.reassign`, `lifecycle.tip_override` and
  `workarea.relate_ratified` payloads gain fields (d15), `lifecycle.supersede` gains
  `retirement` and `self_signed` (d10, d11), `simulation.write` gains `body_bytes`, `replaces`
  and `retirement` (d6, d10, d11), and `SubmitAccepted` gains `workarea_id`, `retirement` and
  `replaces`.
- The MCP catalog goes from ten tools to twelve, eight of them served once the code lands. A served
  tool's entry in `mcp-tools.json` changes in the same change as the code that serves it, because
  `tools/list` is read from that file; until then `_meta.pending_design` in that file names what
  this record decides for the served `oml_recall` and `oml_events_tail` that their entries do not
  carry yet.

**PoC behaviour that changes**

- A writer key calling recall answers `request.workarea_unresolved` when it has no default
  workarea and names no scope, and `access.not_found` when its scope holds no reader grant,
  instead of `access.verb_ceiling` (d1 lists the pinned tests this touches).
- Keys issued without a default workarea and with no grants answer `request.workarea_unresolved`
  on a `/v1` submit, recall or non-own listing once their call passes the verb ceiling, and
  recall's tenant-wide fallback is gone; get, the own-work listing and supersede resolve no
  default workarea and never answer it.
- A second supersession of a tip whose first successor is still held is admitted, not refused;
  the competing verdicts meet the compare-and-set (d10).
- Unknown request fields that were tolerated are refused, and an `X-OML-Workarea` header on get
  or the own-work listing is refused (d13).
- The PoC's "superseded" status is migrated away; rows whose successor never reached promotion
  become served tips again.
- "Own" means the authoring key, so held-row visibility across keys that share a principal ends;
  continuity across key rotation waits on `rotated_from` support.

**Costs that are not measured**

- Effective access is recomputed on every request (one grants read plus one ancestry walk); the
  tail's bounded scan and the steering ancestry query add to it. None of these costs is measured;
  they are tracked against [FR-7](../product/roadmap.md) and [FR-25](../product/roadmap.md), and no
  number is stated.
- Strict-prefix truncation can serve zero hits when the top hit exceeds what remains of the budget,
  since bodies reach 1 MiB; steering larger than `max_tokens` is served over budget. Both show in
  the budget object.

**What stays unbuilt after this iteration** (stated divergences in the README and code comments):
the designed `held_fts` (the PoC uses an author-predicated port method); the read log; held-item
expiry (`held_expires_at` is null); workarea edges (`edges` is `[]`); grant add and revoke, and
denial lift; issuance idempotency; key rotation and expiry enforcement; the operator tier.

**Process obligations**

- With the proposal: the record registers at its three points (this file, a
  [register](index.md) row and a `mkdocs.yml` nav entry), `llms.txt` moves to
  ADR-0001–ADR-0025, the D4, D6, D7, D9, D11, D12, D13 and D14 cells of the D-register name
  ADR-0025, and one
  [changelog](../changelog.md) entry records the 0.2.0-design bump.
- On acceptance, and not before: this record's Status line and frontmatter `status` become
  `accepted design` with the acceptance date, its register row's Status cell follows, the Status
  lines of ADR-0001, ADR-0008, ADR-0010, ADR-0012, ADR-0013, ADR-0014, ADR-0018 and ADR-0022
  gain "· amended by ADR-0025", and a `spec-v0.2` tag is cut.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A total order reader < writer < dreamer < admin** | It lets a dreamer key carry writer grants, and an admin key remember and recall. That contradicts matrix footnote 1 and roles-and-keys.md:201-207, and weakens the two-key rule. |
| **d1 — A key as a set of roles, or writer implying read** | Contradicts "issued with exactly one role" (roles-and-keys.md:34-36) and "not a writer key with implied read" (roles-and-keys.md:38). |
| **d1 — Answer `access.not_found` for any node outside reader access, writes included** | Read literally, it refuses a write-only crew key on a node it holds writer on. errors.md ties `access.grant_refused` to readability, not to the order of checks. |
| **d1 — Keep `access.verb_ceiling` for a writer without a reader grant calling recall** | The writer role admits reader grants, so the ceiling is not what refuses the call; mcp-plugin.md:371-382 specifies `access.not_found`. |
| **d1 — Cache effective access, invalidated by the epoch** | No measurement shows it is needed, and a cache opens a staleness window, beyond the follower's replication lag, that [security](../architecture/security.md) would have to state. Recomputing on every request is one grants read plus one ancestry walk. |
| **d1 — `access.not_found` for a verdict whenever dreamer is not effective** | A second refusal order for one verb; the reviewer already holds the serve, so `access.grant_refused` tells it nothing the serve did not. |
| **d1 — Keep a service key out of review by serving it nothing** | Claim-edge, workarea-edge and status-transition ratification and reopen use no serve, so a stolen reconciler key could still ratify connections, which change other keys' graph reads and every later lineage-closure exclusion, and ratify a workarea's move up the status ladder. |
| **d1 — Compute effective access in SQL in the adapter** | It puts policy in an adapter (ADR-0019 d1) and leaves no pure function to property-test against workareas.md:130-143. |
| **d2 — One transaction with N+1 events** | It changes the commit request for every caller, which is FR-12's batch-commit work. |
| **d2 — Disclose the value after `key.issued` and add the grants afterwards** | A grant failure would leave a disclosed key with fewer rights than its caller believes it holds. |
| **d2 — Inherit the issuer's default workarea** | A PoC invention that hands the new key the issuer's target. |
| **d3 — Principal equality (the PoC's rule)** | Widens R3's audience to every key a principal holds. |
| **d3 — Require a reader grant for own-work reads** | A writer-only key could never see its own held rows; http-api.md:213 says `own=true` admits any tenant role. |
| **d3 — Own-work ignores global denials** | A second access mechanism. |
| **d3 — `derives_from` needs only writer and may name unreadable ids** | Refusals that differ between an unreadable id and a nonexistent one leak existence and state. |
| **d3 — Serve unreadable ancestors as `{artifact_id, redacted: true}` on get** | It turns `derives_from` from ids into objects on every `/v1` read, for a disclosure the lineage read already serves. |
| **d3 — Elide an unreadable held ancestor from the lineage read to `{redacted: true}` with no id** | A second redaction shape beside the bare id every other unreadable node takes, and the id-less node still tells the reader that an unreadable ancestor exists; the disclosure is stated as a residual instead (FR-31). |
| **d3 — Serve `replaces` whatever the caller can read** | After a reassignment of the replaced version, one item would name in `replaces` the id its `derives_from` hides. |
| **d3 — Name an own row's node whatever the key holds** | The tail would null the node the listing names, and a key would learn the id and path of a node it holds no role on. |
| **d4 — A separate `own_held` segment** | A second ranked list for the budget to order, which the agent must merge with the hits. The flag is opt-in, so the meaning of `hits` for existing clients is unchanged anyway. |
| **d4 — Include rejected rows** | FR-10 names held rows; rejected rows stay reachable through get and the own-work listing. |
| **d4 — Own held rows wherever the key ever wrote, regardless of current read access** | Recall would route around denies and revocation. |
| **d4 — Admit `include_own_held` for writer-only keys** | Recall is the reader verb (roles-and-keys.md:64), and the own-work listing serves that case. |
| **d5 — Widen `memories_fts` to every state and filter on state** | Filter discipline on a shared index, which ADR-0013 d3 rejects. |
| **d5 — Decrypt and match the author's held rows on every query** | One KMS unwrap per held row, decrypting rows that are never served. |
| **d5 — Trust the author policy on removals** | Under RLS a removal the policy does not match affects nothing and commits, so held text would outlive its held state. |
| **d5 — Return own held rows by recency, unmatched** | Fails "search it" (critical-path.md:163-166). |
| **d5 — An author policy keyed to the one authenticated key** | A rotated key could never match its predecessor's held text, so `include_own_held` would stop returning held rows across a rotation, against own (d3). |
| **d6 — An embedded BPE vocabulary** | Still one model family's proxy. It needs plaintext or a new journalled field, its version must ride every count (ADR-0019 d4), and an upgrade forces a re-count that needs decryption. |
| **d6 — A remote count-tokens call** | A non-Postgres network call on the hot path (ADR-0024 d2); model-specific, and it sends plaintext out of the deployment. |
| **d6 — UAX #29 word counts** | Needs plaintext, is not a model's count either, and diverges on code and on scripts written without spaces. |
| **d6 — Client-declared counts** | Other readers would rely on a number the author chose. |
| **d6 — A deployment-tunable divisor** | Counts would differ between deployments, so a fixed counter id would no longer mean one function. |
| **d7 — Refuse with a new `request.budget_below_steering` code** | A new code in a closed taxonomy, and the agent loses its standing instructions and pays a retry; serving with visible counts is equally honest. |
| **d7 — Cut steering in a fixed order** | Violates ADR-0018 d4 and ADR-0013 d5. |
| **d7 — Skip-and-continue packing** | Row size would reorder the results, against "then by rank". |
| **d7 — A partial body** | It no longer matches `plaintext_digest`, and the agent cannot tell it was cut. |
| **d7 — A top-level `truncated` boolean only** | It cannot say how far over or short the answer is, and it is indistinguishable from a server that ignored `max_tokens`. |
| **d7 — Budget counting with `include_bodies: false`** | Counting bodies that are not sent gives the caller a meaningless budget, so the combination is refused. |
| **d7 — A cursor to continue past the cut** | A pagination surface FR-11 does not ask for; the caller raises `max_tokens` or lowers `k`. |
| **d8 — Steering from the whole subtree** | A recall at a parent would serve every crew's instructions. |
| **d8 — Ancestor steering outside read access** | A read outside effective access: a second access mechanism. |
| **d8 — Recall over every promoted version** | Retirement and supersession could never take a claim off recall. |
| **d8 — Leave steering unordered** | The wire array has an order anyway, and the budget and the read log need a stable one. |
| **d9 — A digest only for searches** | Cannot answer which rows a key actually saw. |
| **d9 — Log the candidate list before the cuts** | Records reads that never happened. |
| **d10 — Keep the proposal-time tip flip** | Breaches R3: other keys lose the old tip before review. |
| **d10 — Journal both `lifecycle.promote` and `lifecycle.supersede`** | Two facts to correlate for one flip — the pair `lifecycle.supersede` exists to replace. |
| **d10 — Auto-reject the loser of the compare-and-set** | Records a verdict no reviewer gave. |
| **d10 — Allow superseding one's own held version** | Needs a held→held chain and a stale-promotion rule that no page specifies (RS-17). |
| **d10 — Leave dreamer edit-as-proposal unbuilt** | http-api.md:215 specifies it, and it is one branch of the same access check. |
| **d10 — One held successor per tip, a second supersession refused at submission** | A held successor nobody reviews — held-item expiry is unbuilt — would block every other supersession of the claim, so a dreamer's edit-as-proposal could stall the author's own revision or retirement; and a refusal on a readable target that depends on another key's held row is an existence oracle for that row (d3). The chosen design replaces that refusal oracle with a count: a successor's ordinal still shows how many versions precede it, and that count is a stated residual (FR-31). |
| **d10 — Competing successors share version n+1, unique only once promoted** | Two live rows with one (`claim_key`, `version`) make the ordinal ambiguous in the review serve, the journal and the evidence export, and the version-1 rule would need an index of its own. |
| **d10 — Version assigned at ratification** | The signed submission event carries the version (event-catalog.md, `submission.remember`), so a later ordinal is a second fact about one capture, and a held successor would have no version to show its author or its reviewer. |
| **d10 — Lock the target row inside the capture commit to allocate the ordinal** | A statement inside the chain lock beyond the one insert (ADR-0024 d1); the unique index plus a server-side retry yields the same distinct ordinals. |
| **d10 — Own as authorship of the claim's first version** | Own would stop being a property of one row (d3), and an original writer could keep revising a claim whose tip another key's ratified edit now carries. |
| **d10 — Answer target-dependent refusals before readability** | A `request.validation`, `submission.tip_conflict` or `workarea.frozen` on an unreadable target confirms it exists and hints at its workarea, kind or state. |
| **d10 — Scope supersede's record to the target's workarea and look it up after readability** | A retry after the target was erased or reassigned, or after the key lost its reader grant, would answer `access.not_found` or `410` for a write that committed, and the client would be told it failed. |
| **d10 — Let a ratified tainted successor take the default tip, or refuse it outright** | The first breaks "taint never captures the default tip" (D10); the second means no overlay tip can ever arise, and a simulation could never alter a real claim for its audience. |
| **d10 — Digest the body alone** | Two supersessions of different targets with one body under one key would replay the first's stored response, and the second would silently not happen. |
| **d10 — Compare-and-set on `artifact_id` alone** | A reassigned target lets one workarea's reviewers retire another workarea's tip, under a `lifecycle.supersede` whose one `workarea_id` would name two workareas. |
| **d10 — Leave rival proposers eligible to review each other** | A key whose held successor competes for a tip could reject the rival and then win the tip once another reviewer approves its own: a vote in its own row's case through a rival's review, which R2 exists to prevent. |
| **d11 — A dedicated retire endpoint, a `submission.retire` event and an `oml_retire` tool** | A second path to the same tip flip that must re-implement idempotency, the serve, the compare-and-set and `access.author_only`. It adds an operation, an event type and a thirteenth tool, and amends ADR-0010 d1 and ADR-0022 d2. |
| **d11 — Inherit the target's attachments and terms when omitted** | An inherited digest binds bytes another key uploaded, which a submit refuses, and inherited terms annotate a body nobody re-cited. |
| **d11 — A proposal ratified into `lifecycle.demote`** | Makes demote an effect of dreamer ratification, amending the admin-only demote row (ADR-0010 d6); it leaves the claim's tip undefined and cannot be undone. |
| **d11 — A new claim state for retirement** | Widening a `/v1` response enum is breaking (http-api.md:83-87). |
| **d11 — Reuse `term_status: deprecated` for every kind** | A deprecated term stays served so citations self-heal; a retired claim must leave recall. |
| **d11 — An author taking back its own held row** | Does not reach the promoted stale claim FR-10 names. |
| **d11 — Retirement on the writer own-claim path only** | A claim whose tip a dreamer's ratified edit took could then be retired by no one but an admin's demote, which cannot be undone. |
| **d12 — A separate `oml_retire` tool** | Two tools for one ring call (d11). |
| **d12 — A version argument on `oml_get`** | The ring has none, and each version has its own `artifact_id` (ADR-0014 d1). |
| **d12 — Keep supersede HTTP-only** | Fails FR-10 (mcp-plugin.md:285-286 is the sentence this record removes). |
| **d12 — A batch get** | No ring endpoint for it exists, and recall already returns text. |
| **d13 — Default `own=true` to the token workarea's subtree** | Silently leaves out the key's rows in other areas. |
| **d13 — Facade-only `submitted_at` and `held_expires_at`** | ADR-0014 d1. |
| **d13 — `X-OML-Workarea` with `own=true` as a second subtree filter** | Two filters on one listing need an intersection rule of their own; refusing the header is the smaller contract. |
| **d13 — Omit retirement tips from the listing** | The listing would hide a promoted served tip that get serves, and no reader could see in it that a claim is retired. |
| **d14 — Compose `/tree`, `/edges` and `/status` in the facade** | Several ring calls, against mcp-plugin.md:14-16. |
| **d14 — Open `GET /v2/keys/{id}/effective-access` to agents** | It is an admin read, and it returns roles. |
| **d14 — MCP tool names as verbs, computed in the facade** | The facade would say more than the ring, and it would replace a vocabulary the page already uses. |
| **d14 — A per-key epoch counter table** | A second source of truth beside the journal. |
| **d15 — Scan until `limit` visible events** | Unbounded work per call for a key that sees little. |
| **d15 — Reveal submission events after promotion** | Visibility that changes over time, so two reads of one range disagree. |
| **d15 — Let the recording key see its own review events** | A second rule per family for little value; `oml_my_work` shows the outcome. |
| **d15 — Start from the newest event on MCP only** | Facade-only semantics; an agent starts from now by passing `head_seq`. |
| **d15 — Every `lifecycle.*` event to every reader of the subject's workarea** | Shows the erase of another author's held version (R3), a simulation row's promote to keys outside its audience (D10), and a `self_signed` row's promote to tails scoped above the only workarea that serves it. |
| **d15 — Show a version's later events once it was ever promoted** | A version an admin moved back into held would show its later events, its erase included, to every reader of its new workarea (R3). |
| **d15 — Serve the actor's identity on the tail** | Every reader would see which reviewer ratified a row and which admin moved or erased it, which the review queue and `oml_my_work` withhold. |
| **d15 — Fill the subject's workarea from the current `memories` row** | Visibility would then depend on later row state, so two reads of one range could disagree. |
| **d16 — Lenient parsing** | A wrong answer that looks right. |
| **d16 — Read an empty `kinds` or `terms` as no filter** | `[]` would mean "all" by convention while narrowing to nothing by the letter, and the read-log digest could not tell it from absence. |
| **d16 — New codes such as a retired-claim refusal** | The existing codes already name the rules; a retired claim is simply not served. |
| **d17 — Let a self-signed version replace a reviewed tip** | Replaces or retires reviewed content with no second identity, and hides the claim from every read scoped above the workarea (ADR-0001 d5). |
| **d18 — `claim_key` unique over every non-erased row** | Blocks resubmission after a rejection. |
| **d18 — Scope the version-1 collision to what the caller can read** | Two keys' held first versions of one `claim_key` would coexist and the refusal would move to a verdict that the author never sees; the residual is tracked as FR-31 instead. |
