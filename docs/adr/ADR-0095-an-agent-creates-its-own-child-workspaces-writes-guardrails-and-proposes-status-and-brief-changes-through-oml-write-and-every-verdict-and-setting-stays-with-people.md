<!--
id: adr/ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people
tier: engineering
status: accepted design · 2026-11-24
answers: which workspace acts an agent's MCP key reaches from v0.8.5 and which stay with people; whether they are modes of oml_write, modes of oml_workspace or a fourth tool; why kind guardrail was kept off oml_write and whether that reason still holds; which keys create a child workspace over MCP, where, under which name, with which grants, immediately or by proposal, and under which quota; how a status proposal and a brief proposal map to the existing events and who ratifies them; what bounds the number of children and proposals; what the annotations, the server instructions, the plugin skill and the LangChain tools say; whether an agent can escalate, bypass review or flood through these acts; the upgrade, the client notice and the build plan
-->

# ADR-0095 — An agent creates its own child workspaces, writes guardrails and proposes status and brief changes through `oml_write`, and every verdict and setting stays with people

**Status:** accepted design · 2026-11-24, by the operator, who took the recommended answer to
each of the eight questions (recorded as Q1 to Q8 in d11) · proposed the same day at the operator's request ("develop further oml
workspace tool to allow creating and manipulating workspace itself, guardrails, skills, etc if it's
not yet covered"), with the scope the operator chose, "Proposals + own children": an agent's MCP
key may create child workspaces where its key allows, write guardrails, skills, documents and
steering, each held for review, and propose status changes and brief edits; mode, crew, grants and
export stay with people.

**Release:** v0.8.5 ([releases](../changelog.md))

**Amends:**
- [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) (d3): `oml_write` admits kind `guardrail`
  with a `guardrail` object, and gains the modes `create_workspace`, `propose_status` and
  `propose_brief`, each chosen by its field, as `replaces` and `withdraw` choose theirs.
- [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
  (d10): a guardrail is written and superseded on `oml_write` as on the HTTP API.
- [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md)
  (d16, the "Who writes" row): the one kind-level difference it names, a guardrail written over
  HTTP only, is removed.
- [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) (d6): "No MCP tool gains it" becomes `oml_write`'s
  `create_workspace` mode, with the same check.
- [ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)
  (d3: a status proposal is reachable over MCP; d8: a contributor key proposes a brief edge).
- [ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md)
  (d2): a seventh meter, `workspaces`.
- [ADR-0083](ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md)
  (d13 as amended 2026-11-24): `oml_write`'s list of modes grows by three.

**Depends on:** [ADR-0001](ADR-0001-the-constitution.md) (R1, R2; d5's exceptions),
[ADR-0025](ADR-0025-the-agent-only-memory-surface.md) (d1, d12, d14),
[ADR-0030](ADR-0030-an-in-progress-workspace-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md),
[ADR-0082](ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md)
(d4 and its B2 amendment), [ADR-0091](ADR-0091-workareas-become-workspaces-and-the-dreamer-role-becomes-reviewer-on-every-surface-in-one-cut-and-the-journals-former-names-are-read-through-one-map.md)
and [ADR-0093](ADR-0093-contributor-and-submitter-replace-writer.md) (the names and roles this
record uses).

## Context

An agent on MCP reads a workspace and writes claims into it, but cannot shape the workspace it
works in. Over HTTP and the CLI the same key already creates children inside its person's root
(ADR-0044 d6), and a reviewer key proposes status moves and brief members (ADR-0046 d3, d8). A
connector key, the key Claude's hosted apps act as, reaches `/mcp` alone
([security](../architecture/security.md), access-token row;
[roles and keys](../architecture/roles-and-keys.md), connections), so for an agent on claude.ai
those acts do not exist. Two earlier records closed MCP to them on purpose: ADR-0044 d6 ("No MCP
tool gains it: `oml_workspace` stays read-only, and an agent asks its user") and ADR-0046 d3 ("a
transition is a governed act, and an agent that wants one asks its user"). This record reopens
both, within R1 ([ADR-0001](ADR-0001-the-constitution.md) d1): the surface an agent calls never
promotes, signs, demotes or deletes.

### Inventory, 2026-11-24 (main at b9a86bc3)

**The MCP surface** (`crates/transports/oml-transport-http/src/mcp.rs`;
[`mcp-tools.json`](../api/mcp-tools.json); [MCP reference](../api/mcp-plugin.md)):

| Item | What it serves | Annotations |
|---|---|---|
| tool `oml_write` | submission (`POST /v1/memories`), supersession or retirement with `replaces` (`…/supersede`), withdrawal with `withdraw` (`…/withdraw`, ADR-0083 d13 as amended 2026-11-24); the mode is chosen by the field present | readOnly false, destructive false, idempotent true, openWorld false |
| tool `oml_read` | get by `artifact_id`, recall by `query`, the catalog with neither | readOnly true, idempotent true, openWorld false |
| tool `oml_workspace` | one workspace as the key sees it: identity, path, status, crew, readable children, `my_effective_verbs`, policy, `pending_transitions`, the brief (`GET /v2/workspaces/{workspace}`); one argument, `workspace` | readOnly true, idempotent true, openWorld false |
| prompt `guardrails` | the guardrail frame for the key's default workspace (ADR-0037 d11) | — |
| prompts, one per offered skill | `/oml:<name>`, one optional argument `input` (ADR-0080 d5, d6) | — |
| resources | documents at `oml://workspace/<w>/doc/<path>` and skills at `oml://workspace/<w>/skill/<name>`, two URI templates (ADR-0080 d7, ADR-0081 d6) | — |
| server instructions | 2,691 characters today, under the 4,096 Claude Code keeps (`MAX_INSTRUCTIONS_CHARS`) | — |
| designed, not served | tools `oml_relate_propose` and `oml_vocab` ([`mcp-tools.designed.json`](../api/mcp-tools.designed.json)); `oml_read`'s `lineage`, `oml_write`'s `attachments` | — |

**Workspace verbs on HTTP and the CLI** ([openapi.yaml](../api/openapi.yaml); `crates/oml-app/src/app.rs`):

| Verb | Route | CLI | Who may call | Proposal or act | MCP today |
|---|---|---|---|---|---|
| create | `POST /v2/workspaces` | `oml workspaces create` | an admin key anywhere in its tenant; a contributor key naming a user, under a parent inside its own root where its effective access confers submitter (ADR-0044 d6, ADR-0093 d6; `create_workspace_as`); `crew` refused to the user key; `chat` reserved for a person's key (ADR-0063 d8); refused under a frozen parent | immediate act, `workspace.create` | no (ADR-0044 d6) |
| mode | `POST /v2/workspaces/{w}/mode` | `oml workspaces mode` | a reviewer key used by a person, with reviewer effective access (ADR-0030 d1) | immediate act, `workspace.mode_set`; the commit is refused while anything is held | no |
| federation | `POST /v2/workspaces/{w}/federation` | `oml workspaces federation` | a reviewer key used by a person, or an admin key (ADR-0072 d5) | immediate act, `workspace.federation_set` | no |
| crew set | `POST /v2/workspaces/{w}/crew`; crew records on `/v2/crews` | `oml workspaces crew` | admin key | immediate act, `workspace.crew_updated` | no |
| status propose | `POST /v2/workspaces/{w}/transitions` | `oml workspaces status propose` | a key of role reviewer with reviewer effective access, or an admin key | proposal, `workspace.status_proposed`; forward only; refused in a user root, reason `user_root` (ADR-0085 d17) | no (ADR-0046 d3) |
| status ratify | `POST …/transitions/{id}/ratify` | `oml workspaces status ratify` | a second reviewer or admin, never the proposer; a `service` key refused | act, `workspace.status_transition` | no |
| status pending | `GET /v2/workspaces/{w}` (`pending_transitions`); `GET …/offers` | `oml workspaces status pending` | reader effective access | read | read through `oml_workspace` |
| export | `POST /v2/workspaces/{w}/exports` | `oml workspaces export` | a key used by a person, not admin, no profile, reader on the whole subtree (ADR-0084) | immediate act, `workspace.exported` | no |
| import | `POST /v2/imports` | `oml workspaces import` | a contributor key used by a person (ADR-0084 d11, ADR-0093 d6) | immediate act; versions land held; may create its target in progress | no |
| brief propose | `POST /v2/edges`, `kind: brief` | `oml edges propose brief` | a key of role reviewer with reviewer effective access on the workspace, reading a promoted claim (ADR-0046 d8) | proposal, `brief_edge.proposed` | no; the brief is read through `oml_workspace` |
| brief ratify | `POST /v2/edges/{id}/ratify` | `oml edges ratify` | reviewer effective access, never the proposer, no `service` key | act, `review.ratify` | no |
| claim edge propose | `POST /v2/edges`, `kind: claim` | `oml edges propose claim` | reviewer(propose) | proposal, `claim_edge.proposed` | no; `oml_relate_propose` designed, and ADR-0046 d4 plans a `links` field on `oml_write` instead |
| claim edge ratify | `POST /v2/edges/{id}/ratify` | `oml edges ratify` | reviewer(review), never the proposer, no lineage collision | act, `review.ratify` | no |
| releases | none routed: ADR-0085 is accepted and not built; export's `release` mode answers `releases_unbuilt` | — | — | — | no |
| reparent | `POST /v2/workspaces/{w}/reparent`, designed only | — | — | — | no |

**Claim kinds `oml_write` accepts:** `memory`, `derived_fact`, `steering`, `term`, `term_mapping`,
`vocab_release`, `skill`, `document`. Two kinds are refused, each naming `/kind`: `test` until
`oml_write` takes its object (#419), and `guardrail`.

**Why `guardrail` is missing.** The refusal is deliberate. ADR-0037 d10 (2026-11-04) keeps a
guardrail on the HTTP API "as ADR-0025 d12 does for the vocabulary kinds", and `oml_write` also
refuses a `replaces` that names a guardrail. ADR-0080 d16's class table (2026-11-14) states the
reason: "`oml_write`'s inputs stay flat, so a kind with its own required fields (a guardrail's
`severity` and `rule` …) is written over HTTP; this is the one kind-level difference left". Since
v0.8.3 the inputs are no longer flat. `oml_write` carries a `skill` object (ADR-0080 d3) and a
`document` object (ADR-0081 d5), each refused on every other kind. ADR-0037 d10 also implies a
second reason: a rule binds every key below its workspace, so its review must be strong.
ADR-0080 d16 now meets that reason on every transport, since only a person's verdict promotes an
instruction-class version (`review.person_required`, `details.reason: instruction_needs_person`).
Neither reason holds now (d2).

**The records this one works within:** ADR-0039 d8 (a new agent capability is a parameter, a
resource or a hook before it is a tool); ADR-0025 d1 and d14 (the verb ceiling per role and
`used_by`; `my_effective_verbs`); ADR-0001 d1 and d2 (R1, R2); ADR-0030 (in-progress workspaces,
person-only mode); ADR-0082 (the console builder, and B2's guardrail name as claim key); ADR-0037
(guardrails); ADR-0080 (skills, the class rule); ADR-0044 (user roots, d5 names, d6 create);
ADR-0091 and ADR-0093 (workspace, reviewer, contributor and submitter); ADR-0083 d13 as amended
2026-11-24 (withdraw as a mode, which set the pattern for this record).

Decided, not built.

## Decision

- **d1 — Structure acts are three modes of `oml_write`, each chosen by its field; `oml_workspace`
  stays read-only, and no tool is added.** `create_workspace: <name>` creates a child (d3),
  `propose_status: "protected" | "frozen"` proposes a status move (d5), and `propose_brief:
  <artifact_id>` proposes a brief member (d6). In each mode, `workspace` names the node acted on:
  the parent for a create, the workspace itself for the two proposals. Omitted, it is the key's
  default workspace, as on every MCP argument named `workspace`. Every mode requires
  `idempotency_key` and takes `reason`, which is journalled with the act and scanned as a
  verdict's reason is. Any other field is refused with `request.validation` naming it in
  `details.field`, never dropped, by the rule `oml_write` already applies. Two mode fields in one
  call are refused the same way, naming the second.
  The answer is `{mode, …, idempotent_replay}` with the act's own fields.
  This follows ADR-0039 d8 (a parameter before a tool) and the withdraw fold of 2026-11-24. It
  keeps every annotation true. `oml_write` already declares readOnly false, destructive false
  (each mode adds a row or a proposal, or marks one, and removes nothing) and idempotent true (each
  mode replays its recorded answer under the same `idempotency_key`). `oml_workspace` keeps
  readOnly true. Clients use readOnlyHint to decide whether to ask before a call, so a session's
  workspace read never starts asking for permission. The cost: Claude Code permission rules match
  a tool by name, not by its arguments. A rule that allows `oml_write` without a prompt therefore
  admits a create and both proposals without a prompt too, as it has admitted withdraw since
  2026-11-24. The upgrade note says so (d10).

- **d2 — `oml_write` admits kind `guardrail` with a `guardrail` object, and a guardrail is reviewed
  as every instruction is.** `guardrail: {severity, rule?}`: `severity` is `never`, `must` or
  `should` and is required; `rule` is an `oml-gr/1` rule and is optional (ADR-0037 d2, d8). The
  object is required with kind `guardrail`, also with `replaces`, since a supersession re-sends
  both fields. It is refused on every other kind and on a retirement, as on
  `POST /v1/memories`. The body cap stays 300 UTF-8 bytes and the rule cap 160, both vetoed by
  the scan (`guardrail.too_long`, `guardrail.rule_unparsable`; ADR-0037 d7, d10). The guardrail's
  name is `oml_write`'s existing `claim_key`, the key the console derives from its Name field
  (ADR-0082 d4, B2); omitted, the server derives its `auto-` key as for any claim. `replaces`
  naming a guardrail is admitted with kind `guardrail` and refused with any other kind (ADR-0025
  d11). A retirement of a guardrail is admitted like any retirement. Approval is unchanged. A
  version promotes only on a person's verdict outside its lineage (ADR-0080 d16;
  `review.person_required`). A held version answers with the `load_warning` every reader of it is
  served (ADR-0037 d10). Until approved, it reaches its author key and, inside an in-progress
  subtree, the keys d4 of ADR-0080 names, marked, as a line that asks and never denies. The
  refusal that named `/kind` and `/replaces` for a guardrail goes. Of the kinds not admitted over
  MCP, `test` remains (#419).

- **d3 — A contributor key creates a child inside its own person's root, immediately, under the
  check HTTP already applies, and nothing else creates over MCP.** `oml_write {create_workspace:
  <name>, workspace?: <parent>, idempotency_key, reason?}` rides `POST /v2/workspaces` through
  ADR-0044 d6's branch and admits exactly the keys it admits: a key of role `contributor` that
  names a user, under a parent inside that user's root where its effective access confers
  submitter. Every other key is refused before any lookup with `access.verb_ceiling`, `details.reason:
  create_is_own_root_only`: an admin key, a tenant key that names no user, a submitter, a reviewer,
  a reader. The admin key keeps its create on HTTP, the CLI and the console. The admin role is no
  agent's role, and R1's surface carries no admin verb.
    - **Name.** The name follows the name grammar ([HTTP API](../api/http-api.md), "Names and ids")
      and is unique in the root's namespace (ADR-0044 d5): a taken name is `409 name.taken`, and
      `chat` is refused to every key not used by a person (ADR-0063 d8). Names are immutable
      (ADR-0026 d1).
    - **What it starts as.** Status `development`, mode `standard`, no crew. `crew` is not in the
      mode's schema, and `in_progress` is a person's act (ADR-0030 d1). A child created inside an
      in-progress subtree lies inside it, as any node does, and gains no audience of its own (see
      the security analysis).
    - **Grants.** None is written. A grant covers its node and everything below it, and a denial
      always wins ([workspaces](../architecture/workspaces.md)). The creating key's effective
      access on the child is therefore what it held on the parent. So is every other key's: the
      person's review key, the person's web key, and any curator granted on the root. Creating
      confers no role.
    - **Immediate, not a proposal.** A create is none of R1's four acts. It adds an empty node that
      admits nothing beyond what the parent admits, and the same key already makes it over HTTP
      and the CLI. A held create would need two new events and a review queue for empty nodes,
      while what goes into the node is reviewed anyway. The cost: no workspace can be deleted
      ([workspaces](../architecture/workspaces.md)), so a child an agent creates stays in the
      person's tree for good. d4 bounds how many there can be. Immediate, decided
      by the operator (d11 Q1).
    - **Answer.** `{mode: "create_workspace", id, name, path, parent, status, idempotent_replay}`;
      `workspace.create` names the key as actor, as on HTTP.

- **d4 — A seventh meter, `workspaces`, bounds the children every key of an account creates.**
  `workspaces` is a stock meter in ADR-0071 d2's table. It counts the live workspaces under the
  account's root, the root excluded, whoever created them: an agent over MCP, the CLI or the
  console. The create's own transaction moves the counter and refuses past the limit (ADR-0071
  d3) with `429 limit.quota`, `details: {quota: "workspaces", limit, used}`, journalling nothing.
  The limit comes from the account's quota profile. In the default profile it is unlimited, as
  `text_bytes` was until the operator set it (ADR-0071, decision of 2026-11-14). The public
  server's value is 100 per account (d11 Q4). The meter also bounds depth, since a chain of
  children is never longer than their count, so no depth limit is added. The per-key
  `requests_per_minute` (ADR-0068 d5) bounds the rate of creates and proposals alike. Doctor:
  ADR-0071's `warn quota:` and `warn usage: … drift` lines cover the new meter with no new text.
  The usage page and `GET /v1/account/usage` serve it beside the six.

- **d5 — A status proposal is reachable over MCP under its existing authority, and a workspace
  holds one live proposal per target status.** `oml_write {propose_status: "protected" |
  "frozen", workspace?, idempotency_key, reason?}` rides `POST /v2/workspaces/{w}/transitions` and
  journals `workspace.status_proposed`. A second identity ratifies it on HTTP, the CLI or the
  console, journalling `workspace.status_transition`; ratification never rides MCP (d7). The floor
  is unchanged: a key of role reviewer with reviewer effective access. Over MCP that is an agent
  reviewer key, such as a crew's, or a person's reviewer key. A contributor key is refused
  `access.verb_ceiling`, and an admin key is refused as in d3. In a user root every proposal is
  refused `workspace.transition_refused`, reason `user_root` (ADR-0085 d17), so for an agent's
  own children this mode does nothing; it serves shared workspaces. Contributors stay refused
  (d11 Q2). Today a workspace takes any number of identical live proposals. From this
  record, on every transport, a proposal whose workspace already holds a live one to the same
  status is refused `409 proposal.pending`, `details.transition_id` naming the live one. A
  proposal is not refused because another key made the live one: ratification already refuses
  the proposer, so a second proposal of the same move adds nothing.

- **d6 — A brief proposal is reachable over MCP, and a contributor key may make it.** `oml_write
  {propose_brief: <artifact_id>, workspace?, idempotency_key, reason?}` rides `POST /v2/edges` with
  `kind: brief` and journals `brief_edge.proposed`. A second identity with reviewer effective
  access on the workspace ratifies it, never its proposer and never a `service` key, journalling
  `review.ratify` (ADR-0046 d8, unchanged; ADR-0046 d2's `same_user` mark applies). The proposer
  floor falls from reviewer to a key of role contributor or reviewer, with submitter or reviewer
  effective access on the workspace. The claim must still be a promoted served tip the key
  reads. The floor belongs to the verb, so HTTP and the CLI change with it. Every brief member
  is a claim a reviewer has already approved. Its place in the brief is approved again by a
  reviewer who did not propose it. A proposal changes nothing until then. The cost: reviewers
  get a second queue that agents can fill, bounded by the rule below and by the request rate. On
  every transport, a proposal of a claim that is already a live proposal or a ratified member of
  the same brief is refused `409 proposal.pending`, `details.edge_id` naming the existing edge.
  Removing a member stays as ADR-0046 d8 has it: a superseded or retired claim leaves the brief
  by itself. No route removes one, and this record adds none.

- **d7 — Mode, crew, grants, keys, export, import, federation, claim edges, releases and every
  verdict stay off MCP.**
    - **Mode.** `in_progress` makes every author's held rows readable to the keys in the subtree
      and on its ancestor chain (ADR-0030 d3, ADR-0080 d16). An agent that could set it would
      choose who reads unreviewed work, and committing would release that work. Both stay a
      person's acts (ADR-0030 d1, d6).
    - **Crew, grants and keys.** These decide who may act, so they are admin acts (ADR-0026 d8,
      ADR-0025 d2). A key that could change them could raise its own access, the escalation the
      security analysis rules out.
    - **Export.** An export carries the subtree out of the server, encrypted to recipients its
      caller names. It is a person's act (ADR-0084), and an agent driven by injected text must
      not hold a way to send a workspace to a recipient it was handed.
    - **Import.** An import lands held, but may create its target in progress, which is a mode act
      (ADR-0084 d6, d9; ADR-0030 d1). It stays a person's act.
    - **Federation.** It decides which recalls scoped elsewhere search the workspace. It is a
      person's or an admin's act (ADR-0072 d5).
    - **Claim edges.** ADR-0046 d4 plans them as a `links` field of `oml_write`, decided by a later
      record. `oml_relate_propose` stays designed and unserved.
    - **Releases.** Unbuilt. Cutting and ratifying one are a person's acts (ADR-0085).
    - **Every verdict.** Review, ratification of a transition or an edge, and an admin fix's confirm
      or revert promote or demote, so they are what R1 keeps off the agent surface on every tier.

- **d8 — The annotations, the instructions, the plugin skill and the LangChain tools.**
    - **Annotations.** Unchanged (d1): `oml_write` readOnly false, destructive false, idempotent
      true, openWorld false; `oml_workspace` readOnly true.
    - **`my_effective_verbs`** (ADR-0025 d14) gains `create_workspace` on a node where d3 admits the
      key and `propose_brief` where d6 does. `propose` keeps its meaning, a reviewer's claim-edge
      and status proposal. `oml_workspace` thus tells an agent which modes it may use before it
      calls one.
    - **Server instructions.** One paragraph is added after the remembering paragraph, about 380
      characters, so the text stays under 4,096 at about 3,070: "oml_write also creates a child workspace inside
      the person's own root that the key works in, empty, in development and with no grant of its
      own, and proposes a workspace's next status or a reviewed claim for its brief. A proposal
      changes nothing until a different identity ratifies it. Mode, crew, grants, export and every
      verdict are people's acts, outside this server's tools." It describes and never tells the
      model to call a tool (ADR-0067 d2).
    - **Plugin skill** (`plugins/oml/skills/oml-memory/SKILL.md`). The "Three tools, chosen by
      their arguments" list gains a line: "With `create_workspace` it creates a child workspace
      inside the person's own root; with `propose_status` or `propose_brief` it proposes a status
      move or a brief member, which a different identity ratifies." A section, "Organise work into
      workspaces", says when a child helps (a separate task whose memories another task should not
      recall), that it cannot be deleted, that it counts against the account's `workspaces` quota,
      and that a guardrail is written with kind `guardrail` and a person approves it. The skill
      names no act d7 keeps off MCP as something to ask for.
    - **LangChain** (ADR-0094 d3). The tool set does not grow, and its `oml_write` keeps the claim
      modes. ADR-0094 rejected creating workspaces from inside an agent loop ("Namespaces as child
      workspaces"), and a LangChain agent runs in such a loop. A crew that needs a child creates it
      with the CLI or the console.

- **d9 — The modes are proven by tests named after their sentences.** Each red first, by breaking
  the clause it proves:
    - mcp: `oml_write_with_kind_guardrail_and_a_guardrail_object_holds_a_guardrail`,
      `a_guardrail_object_is_refused_on_any_other_kind_and_on_a_retirement`,
      `oml_write_replaces_a_guardrail_only_with_kind_guardrail_and_its_object`,
      `a_guardrail_written_over_mcp_promotes_only_on_a_persons_verdict`;
      `a_contributor_key_creates_a_child_inside_its_own_root_over_mcp`,
      `create_workspace_over_mcp_refuses_an_admin_key_a_tenant_key_and_a_submitter_key_naming_create_is_own_root_only`,
      `a_child_created_over_mcp_is_development_standard_and_carries_no_grant`,
      `a_child_created_inside_an_in_progress_subtree_widens_no_audience`,
      `create_workspace_replays_its_recorded_answer_under_the_same_idempotency_key`,
      `two_mode_fields_in_one_call_are_refused_naming_the_second`;
      `propose_status_over_mcp_journals_workspace_status_proposed_and_ratifies_nothing`,
      `propose_status_refuses_a_contributor_key_with_access_verb_ceiling`,
      `propose_brief_over_mcp_by_a_contributor_journals_brief_edge_proposed`,
      `tools_list_serves_three_tools_and_oml_workspace_stays_read_only`,
      `the_server_instructions_stay_under_4096_characters`.
    - app: `a_create_past_the_workspaces_quota_is_refused_and_journals_nothing`,
      `the_workspaces_meter_counts_every_child_under_the_root_whoever_created_it`,
      `a_second_live_proposal_to_the_same_status_is_refused_proposal_pending`,
      `a_brief_proposal_of_a_claim_already_proposed_or_a_member_is_refused_proposal_pending`,
      `a_contributor_proposes_a_brief_edge_and_cannot_ratify_it`,
      `my_effective_verbs_names_create_workspace_exactly_where_the_create_check_admits`.
    - pg and doctor: `the_workspaces_counter_folds_with_the_create_in_one_transaction`,
      `doctor_reports_workspaces_counter_drift`.

- **d10 — The upgrade is additive for MCP clients except one permission effect, which the
  upgrade note and the client notice name.** The client notice reads: "From v0.8.5 `oml_write`
  also writes guardrails (kind `guardrail` with a `guardrail` object), creates a child workspace
  inside the person's own root (`create_workspace`), and proposes a status move or a brief member
  (`propose_status`, `propose_brief`). Each proposal is ratified by a different identity. A client
  rule that allows `oml_write` without a prompt now admits these too." The HTTP API changes in
  three places: `POST /v2/edges` `kind: brief` admits contributor keys, `409 proposal.pending`
  joins the error codes on both proposal routes, and `limit.quota`'s `quota` gains `workspaces`.
  A script that proposed the same move twice now receives the 409. One forward-only migration
  adds `account_meters.workspaces` and backfills it from the tree. Its number is reserved at
  build. `oml doctor` reports any account whose backfilled count differs, as ADR-0071 d3 does for
  every meter.

- **d11 — The operator's answers of 2026-11-24.** The operator took the recommended answer to
  each question this record put.
    - **Q1 — An agent's create is immediate**, under the `workspaces` quota (d3, d4); a held create
      is rejected.
    - **Q2 — A contributor key does not propose a status move**; the reviewer floor stays (d5).
    - **Q3 — A contributor key proposes a brief member**, on every transport (d6).
    - **Q4 — The public server's `workspaces` quota is 100 per account**; the default profile stays
      unlimited, and the value is recalibrated with the other limits under #164 (d4). The number
      is unmeasured.
    - **Q5 — No key creates a child in a shared workspace in v0.8.5**; ADR-0044 d6's tenant rule
      stands (d3).
    - **Q6 — Connector keys get the modes** (security analysis).
    - **Q7 — The modes go on `oml_write`**, and `oml_workspace` stays read-only (d1).
    - **Q8 — One live proposal per target holds on every transport**, not over MCP alone (d5, d6).
    - A guardrail written over MCP needs a person's approval (d2), as every instruction does.

## Security analysis

- **Escalation: create a workspace, then gain grants there.** It cannot. A create writes no grant
  (d3). Effective access on the child is the covering grants of its ancestors, and a denial on any
  ancestor wins below it. Grants, keys and crews stay admin acts that no MCP path reaches (d7).
  The creating key gets no role on the child that it did not hold on the parent. A create outside
  the person's root is refused before any lookup (d3), so an agent never adds a node to a shared
  tree.
- **Bypassing review: create an in-progress child and read unreviewed writes there.** It cannot.
  Only a person's reviewer key sets `in_progress` (ADR-0030 d1). A child created inside an
  in-progress subtree adds no reader: its held rows reach the keys that the subtree's rows already
  reached (ADR-0030 d2, ADR-0080 d16), and the agent could already write into the subtree itself.
  Outside in-progress, the agent's held writes stay its own key's (ADR-0025 d3) in a child as in
  any node. A guardrail written over MCP binds only on a person's verdict (d2). Until then it is a
  marked line that can ask and never deny, the treatment held steering and skills already get
  over `oml_write`. Status and brief changes are proposals that a different identity ratifies
  (d5, d6). R1's four acts stay off every MCP path, and `tools/call` answers every other name as
  an unknown tool naming R1.
- **Flooding: unbounded children or proposals.** Children: the `workspaces` quota per account
  (d4), which also bounds depth, and `requests_per_minute` per key. Proposals: one live proposal
  per workspace and target status, one per claim and brief (d5, d6), and the request rate. Held
  guardrails: the `memories`, `writes` and `text_bytes` meters already count them as claims, and
  the guardrail set's cap stops an oversized set from loading (ADR-0037 d7). A tenant whose
  default profile leaves `workspaces` unlimited carries the flood risk it already carries on HTTP.
- **What changes for a connector key.** A connector key reaches `/mcp` alone, so these modes give
  it acts it lacked: a create in its person's root, guardrail writes, and the two proposals. A
  bearer key with the same role already had them over HTTP and the CLI. The modes give no key an
  authority its role and grants did not carry. The brief floor (d6) is the one widening, and it
  widens HTTP as well.

## Consequences

- An agent on claude.ai or Claude Code can organise its person's root, write the rules it
  proposes for its peers, and propose status and brief changes without leaving MCP. A person still
  takes every decision that changes who reads or follows what.
- The tool count stays at three, and `oml_write`'s description and schema grow by three modes and
  one object. A model chooses among more fields in one tool, and each refusal names the field
  outside the mode.
- A child an agent creates is permanent. The quota bounds the count, not the clutter: a person
  who wants a child gone cannot remove it, because no workspace can be deleted.
- Reviewers get agents' brief proposals in their queue. The doctor gains no line for stale
  proposals. If they pile up, a later record adds one.
- **Files the build touches:** `crates/transports/oml-transport-http/src/mcp.rs` (the modes, the
  guardrail object, the instructions; `refuse_guardrail_kind` loses its guardrail branch),
  `docs/api/mcp-tools.json` and `docs/api/mcp-plugin.md` (by `scripts/gen_mcp_reference.py`),
  `crates/oml-app/src/app.rs` (the create meter check, the brief floor, `proposal.pending`),
  `crates/oml-domain/src/access.rs` (effective verbs), `crates/adapters/oml-pg/src/lib.rs` and a
  migration (the meter), `docs/api/openapi.yaml` and `docs/api/errors.md`, the usage page in
  `crates/clients/oml-web`, `plugins/oml/skills/oml-memory/SKILL.md` and the plugin README,
  `docs/architecture/roles-and-keys.md`, `docs/architecture/workspaces.md`,
  `docs/use/guardrails.md`, `docs/use/workspaces.md` and `docs/operations/upgrades.md`.
- **Bottleneck:** the create's quota check reads and moves one counter row under the tenant's
  chain lock, as every metered write does (ADR-0071 d3). The two pending checks each add one
  indexed lookup to a proposal. Both are unmeasured and go in the register with the build.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — Modes on `oml_workspace`** (`create`, `propose_status`, `propose_brief` beside the read) | The operator named the workspace tool, and a model would look there first. MCP annotations are per tool, though, so `oml_workspace` would lose readOnlyHint, and clients that ask before a non-read-only call would ask before every workspace read. A client rule that allows the read today would also widen from reading to creating. `oml_write` is already the one tool that changes anything. |
| **d1 — A fourth tool, such as `oml_structure`** | Its own permission rule is the one thing it buys: a client could allow writes and ask before creates. ADR-0039 d8 requires a record that no parameter carries the capability, and one does. The withdraw fold of 2026-11-24 went the other way, from four tools to three. |
| **d1 — One `structure` object field on `oml_write` with an `act` inside** | Two levels of mode selection where every other mode is chosen by one field, and a refusal would have to name a nested pointer for each act. |
| **d2 — Keep guardrails off MCP** | The recorded reason was flat inputs, and `skill` and `document` objects ended it in v0.8.3. The review reason is met by the person's verdict on every instruction (ADR-0080 d16). The exclusion bound only connector keys, which reach `/mcp` alone, since a bearer agent key writes a guardrail with `oml write --severity`. What protects a rule is its review, not its transport. |
| **d2 — Flat `severity` and `rule` fields on `oml_write`** | Each kind's own fields live in that kind's object, as `skill` and `document` do, so one refusal names the object on every other kind. |
| **d2 — A `guardrail.name` field** | It would be a second spelling of `claim_key`, which `oml_write` already takes and which the console's Name field fills (B2). |
| **d3 — A held create, ratified by a second identity** | It needs two new events, a review queue for empty nodes, and a person's look before an agent can separate two tasks. What goes into the node is reviewed anyway, and the same key creates immediately over HTTP. Rejected by the operator (d11 Q1). |
| **d3 — Create wherever the key holds submitter, shared workspaces included** | ADR-0044 d6 keeps tenant keys from creating, so that shared structure stays an admin's. Agents would add permanent nodes to trees many teams read. Rejected for v0.8.5 (d11 Q5). |
| **d3 — Admin keys create over MCP** | The admin role is no agent's role, and an admin key reads no content (ADR-0026 d15), so on the agent surface it would do structure blind. |
| **d4 — A depth limit or a per-parent child limit** | A chain of children is never longer than their count, so the count meter bounds both with one setting the operator already knows how to set. |
| **d4 — Count only children created over MCP** | A meter counts rows, not transports (ADR-0071 d2). An agent would escape it through the CLI, and a person's console creates would be free while an agent's were not, for the same node. |
| **d5 — Open status proposals to contributors** | Protected and frozen stop writes for every key below. In a user root no move exists (ADR-0085 d17), so opening proposals would serve contributor agents only in shared workspaces, where the move is a production decision. Rejected by the operator (d11 Q2). |
| **d6 — Keep the brief proposer at reviewer** | A contributor agent could then never propose its child's brief, which the operator's scope names. The member is already approved, and its place is approved again by a different reviewer. |
| **d7 — Mode over MCP for the agent's own children** | An agent would choose who reads unreviewed work, and committing releases it. ADR-0030 d1 made the mode a person's act for that reason. |
| **d8 — Grow ADR-0094's LangChain tools** | ADR-0094 rejected creating workspaces from inside an agent loop. Its store and tools stay inside one workspace. |
