<!--
id: adr/ADR-0039-the-api-rewritten-for-v0-3-2
tier: engineering
status: proposed
answers: why the agent and HTTP surfaces are rewritten for v0.3.2 while OML is pre-release instead of layered; the four MCP tools that replace the eight served ones, their hints and their argument rules; the server-derived claim_key beside a required idempotency_key; one workarea argument and one id-or-name form; which HTTP routes collapse and where the 49 declared-but-unrouted operations go; the old-to-new mapping that shows nothing is lost; the breaking changes, their migration and the alias window; one error shape on HTTP and MCP; the build plan and its tests
-->

# ADR-0039 — The API is rewritten for v0.3.2 while OML is pre-release: four task-shaped MCP tools, a contract that declares only what is served, and nothing lost

**Status:** proposed · 2026-11-03

**Amends:** ADR-0014 (d2), ADR-0022 (d2, d6), ADR-0025 (d12, d13), ADR-0029 (d2, d6, d8)

## Context

The maintainer asked on 2026-11-03 for a plan to simplify the API without losing functionality.
The delivery plan recommended layering: keep every route, tool and field and add markers,
wrappers and aliases on top. The maintainer chose to **rewrite while pre-release** instead, and
to ship the rewrite as v0.3.2. This record decides that rewrite. Four facts drive it, each
checked against `feat/v0.3.1` (`beba0fc`), from which this branch is cut:

- **The contract declares what is not served.** [openapi.yaml](../api/openapi.yaml) declares 71
  operations. The router serves 22 of them, plus `/mcp`
  (`crates/transports/oml-transport-http/src/lib.rs:192-225`). Nothing in the document tells the
  22 from the 49, and the SDKs are generated from all 71, so their generated layer offers calls
  the server answers with 404.
- **One read has four names, and one argument has three spellings.** The own-work listing is
  `searchMemories` on HTTP, `oml_my_work` on MCP, `list_memories` in Python and `listMemories` in
  TypeScript. A workarea is `workarea` on `oml_remember`, `oml_learn`, `oml_my_work` and
  `oml_workarea_info`, `scope` on `oml_recall` and `oml_events_tail`, and `workarea_id` on
  `oml_vocab` ([mcp-tools.json](../api/mcp-tools.json)). On HTTP it is also `workarea_id`,
  `workarea_node_id`, `parent_id` and `default_workarea_id`. The plugin's skill spends a paragraph
  on the split.
- **The writes are one endpoint behind three tools.** `oml_remember` and `oml_learn` both call one
  handler with a different verb string (`mcp.rs:287-288`), and retiring is `oml_supersede` with
  `retirement: true`. Every MCP write requires a `claim_key` the agent must invent, while the CLI
  already derives one from the text (`crates/bins/oml-cli/src/main.rs:785-787`).
- **Four built CLI verbs are not in the reference.** `oml submit --verb`, `oml recall`,
  `oml supersede` and `oml review verdict --memory` are built (`main.rs:80-89`, `341-360`) and
  appear in no row of [the CLI reference](../cli/reference.md), which designs
  `oml review approve|reject <serve_id>` instead.

A rewrite is open now and closes later. The v1 covenant binds only from FR-21's v1-stable
declaration, which has not been made ([HTTP API](../api/http-api.md#versioning-and-deprecation);
[roadmap](../product/roadmap.md) FR-21). Against it: v0.3.0 is live on `oml.doublegate.io`, the
marketplace serves plugin 0.3.0, and Claude Code addresses plugin tools as
`mcp__plugin_oml_oml__<tool>`, so a renamed tool breaks the permission rules users wrote
([ADR-0029](ADR-0029-session-start-hook.md) d8). This record pays those costs once, before the
freeze, rather than carrying three spellings past it. Decided, not built.

## Decision

- **d1 — While FR-21 is open, the contract may be rewritten, and every rewrite carries its
  mapping, its breaking-change list and its migration in the same release.** A rename or removal
  of a route, field, tool or verb is allowed before the v1-stable declaration when the release
  that makes it (a) maps every removed capability to the call that now provides it, as d10 does,
  (b) lists every break in the changelog entry with the step that migrates it, and (c) states its
  alias window (d12). A removal without a successor is not a simplification and needs its own
  record. FR-21 stays open: the v0.3.2 surface is the candidate the freeze would pin, not the
  freeze.

    The rule makes "without loss" checkable: d10's table is the test a reviewer runs. The cost is
    that every early adopter migrates once, and that this record must name each break rather
    than leave it to a reader of the diff.

- **d2 — The served MCP catalog is four tools: `oml_write`, `oml_read`, `oml_workarea` and
  `oml_events`.** They replace the eight served tools of ADR-0025 d12. Each is a task an agent
  performs, not an endpoint it must know:

    | Tool | Task | Rides |
    |---|---|---|
    | `oml_write` | store a claim, a derived claim, a new version of a claim, or a proposal to retire one | `POST /v1/memories`, or `POST /v1/memories/{artifact_id}/supersede` when `replaces` is sent |
    | `oml_read` | find, fetch or list claims | `POST /v1/memories/recall`, `GET /v1/memories/{artifact_id}` or `GET /v1/memories` |
    | `oml_workarea` | see one workarea as this key may see it | `GET /v2/workareas/{workarea}` |
    | `oml_events` | poll what changed since a resume point | `GET /v2/events` |

    `my_work` folds into `oml_read`, because it is the same ring read as the listing, and four
    names for one read is the defect this record exists to remove. `oml_workarea` and
    `oml_events` stay separate tools. The workarea read returns a node, its children and the
    caller's effective verbs, not claim items, and the events tail is a cursor-polled stream with
    its own resume point and gap semantics. Folding either into `oml_read` would make one tool
    return three unrelated shapes, which costs the agent more than a fourth name does.

    The four unserved tools leave the catalog: `oml_attach`, `oml_relate_propose`, `oml_graph`
    and `oml_vocab` move to a designed catalog, `docs/api/mcp-tools.designed.json`, which no
    `tools/list` serves. Each returns to the served catalog in the change that builds its ring
    endpoints, renamed to this record's conventions. The record that builds it decides whether it
    folds: `oml_graph` reads like a lineage mode of `oml_read`, and that decision is left to the
    record that builds lineage.

- **d3 — `oml_write` takes one flat argument set, and the server chooses the endpoint and the
  verb from which fields are present.** Its arguments are `text` (required), `idempotency_key`
  (required), and the optional `claim_key`, `kind`, `derives_from`, `workarea`, `replaces`,
  `retirement` and `reason`.

    - Without `replaces`, it is a submission. The journal event is `submission.learn` when
      `derives_from` is non-empty and `submission.remember` otherwise, so declared lineage is what
      makes a claim learned, which is how [the HTTP API](../api/http-api.md#submit-post-v1memories)
      already defines `learn`. `kind` takes every kind `oml_remember` takes today. `retirement`
      and `reason` are refused here.
    - With `replaces`, it is a supersession of that version, and `retirement: true` makes it the
      retire-proposal of ADR-0025 d11, with `text` stating why. `claim_key` is refused, because
      the successor takes the target's. `workarea`, when sent, is the `X-OML-Workarea` scope, and
      one that is not the target's workarea is refused as the ring refuses it today
      (`app.rs:2744-2750`). `kind` must be `memory`, `derived_fact` or `steering`, and a target of
      a vocabulary kind is refused with `request.validation` and `details.field: replaces`, as
      `oml_supersede` refuses it under ADR-0025 d12: the facade calls the supersede path with
      `refuse_vocabulary_target` set, so a term is never superseded or deprecated over MCP outside
      the atomic deprecation of [ADR-0021](ADR-0021-ai-human-ontologies.md).
    - `retirement` without `replaces` is refused.

    Every such refusal is `request.validation` with `details.field` naming the argument to drop
    or add, which is ADR-0025 d16's one-hop recovery. This amends ADR-0025 d12's "inputs are
    flat, with no conditionally required fields": inputs stay flat, with no nested `oneOf`, and
    one rule per field says when it applies. The cost is that a model can send a combination the
    rule refuses; the refusal names the field, so the next call corrects it. The benefit is one
    write tool for one act of writing, in place of three tools whose schemas differ in one
    required field.

    The output is one shape in both modes: `{mode, artifact_id, claim_key, version, state,
    workarea, retirement, replaces, idempotent_replay}`, with `mode` `submit` or `supersede`, and
    `replaces` null and `retirement` false on a submission. `workarea` is the node id, as in
    today's outputs (d7). `idempotent_replay` stays: it is true when the ring answered from the
    stored response of an earlier call with the same `idempotency_key` (ADR-0014 d3), and it is
    the agent's only signal that a retry replayed. `attachments_bound` leaves with `attachments`
    for the designed annex (d9).

- **d4 — `oml_read` chooses its mode the same way: `artifact_id` fetches, `query` recalls, and
  neither lists.** Each mode takes its own arguments, and an argument outside them is
  `request.validation` with `details.field` naming it, in every mode:

    | Mode | Arguments | `workarea` means | Rides |
    |---|---|---|---|
    | get | `artifact_id` | not taken | `GET /v1/memories/{artifact_id}` |
    | recall | `query`, `workarea`, `k`, `kinds`, `include_own_held`, `max_tokens` | the `X-OML-Workarea` scope, as `oml_recall`'s `scope` is today | `POST /v1/memories/recall` |
    | listing | `own` (default `true`), `workarea`, `claim_key`, `state`, `kind`, `since`, `until`, `limit`, `cursor` | the subtree filter, as `oml_my_work`'s `workarea` is today | `GET /v1/memories` |

    `own: true` is the own-work listing (`own=true` on the ring), and `own: false` is the scoped
    listing of promoted tips that MCP did not offer before; its scope is the key's default
    workarea, and `workarea` narrows it as ADR-0025 d13's filter does. `kinds`, `since` and
    `until` pass to the ring's fields of the same names, so the tool carries every filter the
    served HTTP reads take except `include_text`: the tool always asks for the text, as
    `oml_recall` does today. Sending both `artifact_id` and `query` is `request.validation`
    naming both. Every mode returns ADR-0025 d12's one claim-item shape, unchanged (d7), inside
    `{mode, items, next_cursor}` for the listing, `{mode, item}` for get and `{mode, steering,
    hits, budget}` for recall, so a caller branches on `mode` once.

- **d5 — Each tool declares its MCP annotations.** `oml_read`, `oml_workarea` and `oml_events`
  carry `readOnlyHint: true` and `idempotentHint: true`. `oml_write` carries `readOnlyHint:
  false`, `destructiveHint: false` and `idempotentHint: true`. All four carry
  `openWorldHint: false`. A write is never destructive, since it is held until a different
  identity reviews it and a retirement is a proposal; it is idempotent because `idempotency_key`
  is required and a repeat replays the stored response (ADR-0014 d3). Claude Code runs the calls
  of a read-only tool in parallel and may repeat an idempotent call that was pending when a
  connection closed ([MCP in Claude Code](https://code.claude.com/docs/en/mcp), "Mark tools that
  are safe to call twice", fetched by the planning lane on 2026-11-03). A read still writes a
  read-log row ([retrieval](../architecture/retrieval.md#the-read-log)); that row is the server's
  own record of the read, not a change the caller asked for, so `readOnlyHint` describes the
  caller's view honestly.

- **d6 — The server derives `claim_key` from the author and the text when it is omitted, and
  `idempotency_key` stays required.** The derived key is `auto-` followed by the first 16
  lowercase hex digits of the SHA-256 of the authoring key's id, a zero byte, and the
  NFC-normalized text; once rotation is built, the id is the root of the key's `rotated_from`
  chain, so a rotated key derives the names it derived before
  ([roles and keys](../architecture/roles-and-keys.md#own)). An explicit `claim_key` that starts
  with `auto-` is refused with `request.validation` and `details.field: claim_key`, so no caller
  can send a name derived for another key. The response always names the `claim_key`, so the
  agent learns it from the first write. The rule applies on `POST /v1/memories`, so every surface
  gets one rule: the CLI stops deriving `cli-` keys and sends none. A supersession never derives
  one.

    A derived name is the author's own. The same key writing the same text twice into one
    workarea meets `submission.claim_key_conflict` while the first version is held, promoted or
    demoted, which is the duplicate it should meet. A second key writing the same text derives a
    different name and is stored as its own claim. A digest of the text alone would have made
    that conflict an exact-text oracle: any writer could submit a guessed sentence and learn
    whether another key's held row, or a promoted or demoted row it cannot read, holds it, which
    breaks ADR-0025 d5's fence on held text and FR-31's "a name, never content". With the key in
    the digest, the conflict tells a writer only about its own rows, so FR-31's residual is
    unchanged. The cost is that two agents that record the same fact into one workarea leave two
    claims, and the reviewer, not the server, sees the duplicate.

    `idempotency_key` stays required on `oml_write`. Making it optional would collapse two
    deliberate identical writes into one, and the maintainer chose to keep it required on
    2026-11-03. The SDKs keep generating one per call (sdks.md, "What the wrapper covers").

- **d7 — One spelling per concept in requests on every surface; responses keep their field
  names, except `body`.** A request field that references a workarea, crew, key or tenant is
  named after the kind and takes an id or a name ([ADR-0026](ADR-0026-names-are-keys.md) d4).
  The served request fields this renames are all of them, from the request structs
  (`lib.rs:1349-1535`) and the query parameters:

    | Where | v0.3.0 | v0.3.2 |
    |---|---|---|
    | `POST /v2/keys` | `default_workarea_id`; `tenant_id`; each grant's `workarea_node_id`; each grant's `deny[].subtree_node_id` | `default_workarea`; `tenant`; `workarea`; `deny[].workarea` |
    | `POST /v2/keys/{key}/denials` | `workarea_node_id` | `workarea` |
    | `POST /v2/workareas` | `parent_id` | `parent` |
    | `GET /v1/memories`, `GET /v2/events` | query `workarea_id` | query `workarea` |
    | MCP `oml_recall`, `oml_events_tail` | `scope` | `workarea` |
    | MCP `oml_vocab` (designed) | `workarea_id` | `workarea` |
    | path parameters | `{workarea_id}`, `{crew_id}`, `{key_id}` | `{workarea}`, `{crew}`, `{key}` |

    `crew` on key issuance and the workarea writes, and `workarea` on `POST /v2/review/next`,
    already follow the rule. `tenant_name` on `POST /v2/bootstrap/claim` names a tenant the
    ceremony creates, not one it references, and keeps its name. The `X-OML-Workarea` header of
    ADR-0022 d4 is unchanged.

    One concept keeps one spelling: the retirement flag is `retirement` on `oml_write`, as on
    HTTP and in every response; the own-work switch is `own` on MCP, the ring and the CLI
    (`--own`). `replaces` is one name with two shapes on purpose: a request names the replaced
    version by its artifact id, and a response returns `{artifact_id, version}`, as it does in
    v0.3.0, so the reader also learns the ordinal. `derives_from` is a list of artifact ids in
    both.

    Responses keep their v0.3.0 field names, so the claim item's `workarea`, a grant's
    `workarea_node_id`, a denial's `workarea_node_id`, a key's `default_workarea_id` and
    `tenant_id`, a workarea's `parent_id` and `oml_workarea_info`'s `parent` and `crew` do not
    change. A response names a stored record by its id, where a request may use the name, so the
    two are different fields by design; renaming the responses too would break every reader of a
    v0.3.0 response for no capability. The one response rename is the claim's text: the ring's
    `body` becomes `text` on every surface, as MCP already spells it, and `include_bodies`
    becomes `include_text`. "Body" stays the storage word of
    [ADR-0004](ADR-0004-bodies-and-attachments.md) for the stored bytes. `artifact_id` stays the
    version's id on every surface.

- **d8 — A new agent capability is a parameter, a resource or a hook before it is a tool.** The
  proposed records already follow this rule: ADR-0035 rejects `oml_deliver` for a parameter of the
  write, ADR-0027 rejects `oml_classify`, ADR-0037 rejects `oml_guardrails`, and ADR-0036 adds
  parameters and MCP resources and no tool. This record writes the rule down. A fifth served tool
  needs a record that says why no parameter of the four, no resource and no hook can carry it.

- **d9 — The HTTP contract declares only what the server routes; the rest is a designed annex,
  and one duplicate is removed.**

    - **Served.** [openapi.yaml](../api/openapi.yaml) holds exactly the 22 routed operations, and
      on each of them only the fields the build accepts and returns. Its `ErrorCode` enum stays
      complete, every code of [errors.md](../api/errors.md) included, because the SDKs' error
      classes are generated from it and `check_contract_sync.py` compares it with errors.md. A
      check fails when the document and the router disagree (d14, step 1).
    - **Designed.** A second document, `docs/api/openapi-designed.yaml`, holds under `paths` the
      45 designed operations that are not built, and under a top-level `x-oml-designed-fields`
      map, keyed by a served operation's `operationId`, the designed fields of served operations:
      the vocabulary fields `terms`, `term_status`, `successors` and `mapping` on submit and
      supersede, `terms` and `expand_terms` on recall, `attachments`, `simulation_id` with
      `simulated_time`, and the status read's review-policy and pending-transition fields on
      `getWorkarea`. No served path appears under its `paths`, so the step-1 test reads `paths`
      alone. Its schemas refer to the served document's `ErrorCode` rather than declare one.
      Nothing is generated from it and no server reads it. An operation moves from it into
      openapi.yaml in the change that routes it, and a field in the change that accepts it, so the
      SDKs gain a call when the server answers it. Nothing moves in v0.3.2.
    - **Contract sync.** `check_contract_sync.py` compares http-api.md's endpoints with the union
      of both documents' `paths`, and fails when a path and method appear in both. http-api.md
      keeps documenting all 67 operations; each designed one is marked "designed, not routed",
      and the check fails when that mark and the document that declares the operation disagree.
    - **A designed field sent to a served operation** answers what it answers in v0.3.0, so the
      d12 aliases and the HTTP routes keep v0.3.0's behaviour for the same input: `attachments`
      answers `submission.attachment_missing`, `simulation_id` answers `access.not_found` (fail
      closed as absence), and a vocabulary field answers `request.validation` naming it
      (ADR-0025 d16). Any other undeclared field is `request.validation` with `details.field`
      (d11).
    - **Removed as a duplicate.** `GET /v2/workareas/{id}/status` goes, because the workarea read
      already carries status; its review-policy and pending-transition fields join that read's
      designed fields.
    - **Merged.** The claim-edge family (`GET /v2/claim-edges`, `POST /v2/claim-edges`,
      `POST /v2/claim-edges/{edge_id}/ratify`) and the workarea-edge family
      (`GET /v2/workareas/{id}/edges`, `POST /v2/workareas/{id}/edges`,
      `POST /v2/workarea-edges/{edge_id}/ratify`) become one designed family of three,
      `GET /v2/edges`, `POST /v2/edges` and `POST /v2/edges/{edge_id}/ratify`, with `kind` ∈
      `claim` | `workarea`, as `oml_relate_propose` already takes. Only the shape merges: each
      kind keeps its own authority ([HTTP API](../api/http-api.md#the-surface): a claim edge is
      proposed under dreamer(propose) and ratified under dreamer(review); a workarea edge is
      proposed under dreamer(propose) on at least one endpoint and reader on both, and ratified
      under dreamer(review) on both; neither by its proposer), its own refusal codes, and its own
      journal events, `claim_edge.*` hidden from the tail and `workarea.relate_*` shown (ADR-0025
      d15). `GET /v2/edges?kind=workarea` carries the proposer and the ratifier that
      `GET /v2/workareas/{id}/edges` carries, and the workarea endpoint that was a path parameter
      is the request's `from`. That is 49 − 1 − 3 = 45 designed operations.
    - **Renamed operationIds.** `searchMemories` becomes `listMemories`, `proposeSupersession`
      becomes `supersedeMemory` and `updateCrewMetadata` becomes `setWorkareaCrew`, the name of
      its handler (`lib.rs:218-221`). Paths do not change.

    A simulation field on a served write is moved, not lost: no simulation can exist in this
    build, and the server refuses every `simulation_id` it is sent (`lib.rs:1973-1980`). The
    error taxonomy stays whole in [errors.md](../api/errors.md), since one closed taxonomy is
    shared by both documents; a code that only a designed operation raises is marked "raised by
    designed operations only".

- **d10 — Every served tool, route, CLI verb and SDK method maps to its successor.**

    **MCP tools.**

    | v0.3.0 | v0.3.2 |
    |---|---|
    | `oml_remember {workarea, claim_key, text, kind, derives_from, idempotency_key}` | `oml_write {workarea, claim_key?, text, kind, idempotency_key}`; with `derives_from` it is journalled as learn (d3) |
    | `oml_remember`/`oml_learn` output `{artifact_id, claim_key, version, state, workarea, attachments_bound, idempotent_replay}` | `oml_write` output (d3), without `attachments_bound` |
    | `oml_learn {workarea, claim_key, text, derives_from, idempotency_key}` | `oml_write {workarea, claim_key?, text, derives_from, idempotency_key}` |
    | `oml_supersede {artifact_id, text, kind, derives_from, reason, idempotency_key}` | `oml_write {replaces, text, kind, derives_from, reason, idempotency_key}` |
    | `oml_supersede {…, retirement: true}` | `oml_write {replaces, retirement: true, text, idempotency_key}` |
    | `oml_supersede` output `{…, retirement, replaces, workarea, idempotent_replay}` | `oml_write` output (d3), with `mode: supersede` |
    | `oml_recall {query, scope, k, include_own_held, max_tokens}` | `oml_read {query, workarea, k, include_own_held, max_tokens}`, and `kinds` (d4) |
    | `oml_get {artifact_id}` | `oml_read {artifact_id}` |
    | `oml_my_work {workarea, claim_key, state, limit, cursor}` | `oml_read {workarea, claim_key, state, limit, cursor}` (`own` defaults to `true`), and `kind`, `since`, `until` (d4) |
    | — | `oml_read {own: false, …}`, the scoped listing, new on MCP |
    | `oml_workarea_info {workarea}` | `oml_workarea {workarea}` |
    | `oml_events_tail {scope, after_seq, limit}` | `oml_events {workarea, after_seq, limit}` |
    | `attachments` (`[{upload_session}]`) on `oml_remember` and `oml_learn`, and the `attachments_bound` output | the designed annex (d9), with `oml_attach`; sent anyway, it answers `submission.attachment_missing` as today |
    | `simulation_id`, `simulated_time` on the three writes | the designed annex (d9); sent anyway, `simulation_id` answers `access.not_found` as today |
    | the claim item and every other tool output | unchanged (d7) |
    | `oml_attach`, `oml_relate_propose`, `oml_graph`, `oml_vocab` (listed, never served) | `mcp-tools.designed.json` (d2) |

    **HTTP routes.** The paths of all 22 are unchanged.

    | Route | operationId, v0.3.0 → v0.3.2 | Request and response changes |
    |---|---|---|
    | `GET /healthz`, `GET /readyz` | unchanged | none |
    | `POST /v1/memories` | `submitMemory` | `body` → `text` in the request; `claim_key` optional (d6); `verb` optional, derived as d3 states, and an explicit `verb` is still honoured; designed fields to the annex (d9) |
    | `GET /v1/memories` | `searchMemories` → `listMemories` | query `workarea_id` → `workarea`; the summary unchanged (d7) |
    | `POST /v1/memories/recall` | `recallMemories` | `include_bodies` → `include_text`; `body` → `text` in steering and hits; `terms` and `expand_terms` to the annex |
    | `GET /v1/memories/{artifact_id}` | `getMemory` | `body` → `text` in the response |
    | `POST /v1/memories/{artifact_id}/supersede` | `proposeSupersession` → `supersedeMemory` | `body` → `text` in the request; designed fields to the annex (d9) |
    | `POST /v2/review/next` | `reviewNext` | `item.body` → `item.text` |
    | `POST /v2/review/verdicts` | `postVerdict` | none |
    | `POST /v2/keys` | `issueKey` | request: `default_workarea_id` → `default_workarea`, `tenant_id` → `tenant`, each grant's `workarea_node_id` → `workarea`, each grant's `deny[].subtree_node_id` → `deny[].workarea`; the response unchanged (d7) |
    | `GET /v2/keys/{key}` | `getKey` | path parameter renamed; the response unchanged |
    | `POST /v2/keys/{key}/denials` | `addDenial` | request `workarea_node_id` → `workarea`; the response unchanged |
    | `POST /v2/crews`, `GET /v2/crews`, `GET /v2/crews/{crew}`, `POST /v2/crews/{crew}/update` | `createCrew`, `listCrews`, `getCrew`, `updateCrew` | path parameter renamed |
    | `POST /v2/workareas` | `createWorkarea` | `parent_id` → `parent` |
    | `GET /v2/workareas/{workarea}`, `GET /v2/workareas/{workarea}/tree` | `getWorkarea`, `getWorkareaTree` | path parameter renamed |
    | `POST /v2/workareas/{workarea}/crew` | `updateCrewMetadata` → `setWorkareaCrew` | path parameter renamed |
    | `GET /v2/events` | `tailEvents` | query `workarea_id` → `workarea`; the response unchanged |
    | `POST /v2/bootstrap/claim` | `bootstrapClaim` | none |

    **CLI verbs.** The writer conveniences mirror the tools, and the review verbs become the
    designed ones.

    | v0.3.0 | v0.3.2 |
    |---|---|
    | `oml submit --claim <text> [--verb remember\|learn] [--claim-key] [--derives-from …] [--workarea]` | `oml write <text> [--claim-key] [--kind] [--derives-from …] [--workarea]` |
    | `oml supersede …` | `oml write <text> --replaces <artifact_id> [--retirement] [--reason]` |
    | `oml recall --query <q> [--k] [--workarea]` | `oml read --query <q> [--k] [--workarea]`; `oml read <artifact_id>`; `oml read [--own\|--all] [filters]` |
    | `oml review next` | `oml review next`, printing the `serve_id` |
    | `oml review verdict --memory <uuid> --approve\|--reject [--reason]` | `oml review approve <serve_id> [--note]`, `oml review reject <serve_id> --reason <text>` |

    The CLI's other built verbs (bootstrap, tenants, keys, deny, workareas, crews, audit, doctor,
    hook) keep their names; their flags follow d7's request spellings. The reference gains a row
    for every built verb.

    **SDK wrapper methods.** Python and TypeScript take the same names, matching the tools:
    `write`, `read`, `workarea`, `events` (an iterator that follows `next_after_seq`),
    `review_next`, `approve` and `reject` (`reviewNext`, `approve`, `reject` in TypeScript).
    `write` takes the tool's arguments under the tool's rules, `workarea` included. `read` takes
    the tool's arguments with the tool's meaning in each mode, plus what the served HTTP reads
    offer and the tool does not: `include_text` on recall, and on the listing a separate scope
    argument, `x_oml_workarea` in Python and `xOmlWorkarea` in TypeScript, named after the header
    it sets, since `workarea` there is the subtree filter. So every filter of `recall`
    (`kinds`, `include_bodies`) and of `list_memories`/`listMemories` (`own`, `since`, `until`,
    the `workarea_id` filter and the `workarea` scope) has a `read` argument. `remember`, `learn`
    and `supersede` map to `write`; `recall`, `get`, `list_memories`/`listMemories`, `pages` and
    `iter_memories`/`memories` map to `read`; `verdict` maps to `approve` and `reject`.
    `upload_attachment`/`uploadAttachment`, a stub that raises today, leaves the wrapper with
    `oml_attach` for the designed annex and returns with the upload routes. The three reads the
    agent loop uses and no wrapper covered, my work, the workarea and the events tail, are now
    covered.

- **d11 — One error shape on HTTP, MCP, the CLI and the SDKs.** Every refusal is the envelope of
  [errors.md](../api/errors.md#the-envelope), `{error: {code, message, rule, details,
  retryable}, request_id}`. On MCP a tool that fails answers a result with `isError: true`, the
  envelope in `structuredContent`, and the same JSON as the text of its one content item, as the
  facade already does (`mcp.rs:314-325`). That includes arguments outside the tool's schema
  (`request.validation`) and d3's and d4's field rules. JSON-RPC errors are kept for faults of the
  protocol, not of a tool: an unparseable message (-32700), an invalid request (-32600), an
  unknown method (-32601), and `tools/call` naming a tool the server does not serve (-32602).
  Since d2 leaves no unserved tool in the catalog, -32602 now means only "no such tool". The CLI
  prints the envelope with `--json` and its `rule` without, and the SDKs raise the family class.
  No code is added: d3's, d4's, d6's and d12's refusals use `request.validation` with
  `details.field`, and a request that sends a field this record renamed is refused with
  `details.renamed_to` naming the new spelling. errors.md's "Implementation mapping" gains the
  MCP mapping as one section, so the rule has one declaration site.

- **d12 — The old names are kept for one minor release as aliases, on MCP and the CLI only.
  Recommended; the maintainer confirms.** As of 2026-11-03 the maintainer has not confirmed this
  window; step 3 of d14 is built only once the confirmation is recorded here with its date.

    - **MCP.** The eight v0.3.0 tool names stay callable in every v0.3.x release, with their
      v0.3.0 input and output shapes and refusal codes (d9), translated at the facade onto the
      four tools. They are not listed in `tools/list`, and each call is logged with `event`
      `mcp_alias_called` and the tool name. v0.4.0 removes them, and a call then answers -32602.
      The window serves raw MCP clients that hard-code tool names, such as
      `scripts/quickstart.sh` and a user's own scripts. It does not serve Claude Code: Claude
      Code discovers a server's tools through `tools/list`, and
      [MCP in Claude Code](https://code.claude.com/docs/en/mcp) describes no other path, so an
      unlisted alias is never offered to the model there. A session on the cached plugin 0.3.0 still sees the
      four new tools, because the catalog comes from the server; what goes stale is the cached
      skill text and the user's permission rules, which no alias fixes and d13 migrates.
    - **CLI.** `oml submit`, `oml recall`, `oml supersede` and `oml review verdict` stay as hidden
      aliases through v0.3.x and print one stderr line naming the new verb. `review verdict
      --memory` keeps building the serve id itself, as today, until it is removed.
    - **HTTP.** No alias. A renamed request field is refused with `details.renamed_to` (d11).
      Old CLIs on `PATH` therefore get no session context from the v0.3.2 server, and the hook
      fails open without an error (ADR-0029 d5); the fix is the v0.3.2 CLI, which ships in the
      same release.
    - **SDKs.** No alias. Neither SDK is published to a registry, and both install from a checkout
      ([client SDKs](../engineering/sdks.md)); each moves to `0.2.0`.

    The cost of the window is one translation layer in the facade for one release, and tests for
    it. Without it, a script that calls a v0.3.0 tool by name fails on the day the server
    upgrades, rather than after a release in which its log line names the call.

- **d13 — The breaking changes are these, and each has its migration.**

    | Break | Who it reaches | Migration |
    |---|---|---|
    | The four tool names replace the eight | users whose `permissions.allow` names `mcp__plugin_oml_oml__oml_recall` and the other read tools | replace the read rule with `mcp__plugin_oml_oml__oml_read`, `__oml_workarea` and `__oml_events`; the server-wide rule `mcp__plugin_oml_oml` still allows everything. The plugin README carries both rules (amends ADR-0029 d8) |
    | The cached skill and the session hook name the old tools | plugin 0.3.0 installs, which `tools/list` already offers the four new tools | `claude plugin update oml@open-memory-lab` and the v0.3.2 CLI, whose hook names the new tools (amends ADR-0029 d6) |
    | Scripts that call the old tools by name | raw MCP clients, `scripts/quickstart.sh` | the new names; the aliases answer until v0.4.0 if d12 is confirmed |
    | Request field renames (d7), optional `verb` and `claim_key`, an explicit `claim_key` starting with `auto-` refused (d6) | HTTP callers, the CLI, the hook | upgrade the CLI with the server; the refusal names the new spelling. The hook's recall sends `include_text` (amends ADR-0029 d2) |
    | `body` → `text` in the memory, recall and review responses, the one response rename (d7) | HTTP readers, the SDKs | read `text`; every other response field keeps its name |
    | `remember` with `derives_from` is journalled `submission.learn` | journal readers, the evidence export | none; the event names a claim with declared lineage, as `learn` always did. An explicit HTTP `verb` keeps the old event |
    | Generated SDK modules renamed (`search_memories` → `list_memories`, `propose_supersession` → `supersede_memory`, `update_crew_metadata` → `set_workarea_crew`) and the 49 unrouted operations gone from the generated layer, which is generated from the served document alone | SDK users | move to the wrapper's `write`/`read`; the removed generated calls never reached a route |
    | CLI verbs renamed | scripts, `scripts/quickstart.sh`, the README | the new verbs; the old ones warn through v0.3.x |
    | `oml.doublegate.io` serving v0.3.0 | the maintainer's own deployment and its agent keys | upgrade the server image, the CLI and the plugin together, run `claude plugin configure` as [ADR-0040](ADR-0040-plugin-setup-in-claude-code.md) states, replace the permission rules, and record the upgrade with its date |

    Keys, grants, stored memories, the journal and the database schema do not change: the rewrite
    touches only the shapes on the wire.

- **d14 — The build is one change set in six steps, ordered so that every gate passes after
  each step.** The four doc gates (`mkdocs build --strict`, `check_contract_sync.py`, the
  OpenAPI validator and `check_plugin.py`) and the CI step "Quickstart, then the agent plugin
  over /mcp" pass after every step, not only after the last. So whatever names a tool or a
  request field moves in the step that renames it. Each step names the tests that close it,
  written as sentences.

    1. **The contract, the facade and everything that names a tool or a field, together.**
       openapi.yaml (served only), the new `openapi-designed.yaml`, mcp-tools.json (four tools
       with hints), the new `mcp-tools.designed.json`, http-api.md, errors.md and mcp-plugin.md
       change in one commit with the facade and the router, because
       `scripts/check_contract_sync.py` holds errors.md, openapi.yaml and http-api.md together,
       and mcp-tools.json changes only with the code that serves it. In the same commit: the
       `oml-memory` skill names the four tools and drops the paragraph on argument spellings; the
       plugin README carries d13's permission rules; `scripts/check_plugin.py` reads the new
       `SERVED_TOOLS`; `scripts/quickstart.sh` calls `oml_write` and `oml_read`; the CLI's and
       the hook's requests use d7's spellings and `include_text`; and the session hook's lines
       (ADR-0029 d6) and the CLI reference's exact-text block name the new tools (below).
       `check_contract_sync.py` gains the router comparison and d9's union. `info.version` moves
       to `0.3.2-design`. Tests:
        - `the_served_openapi_document_declares_exactly_the_routed_operations`
        - `no_path_in_the_designed_annex_is_routed`
        - `tools_list_names_four_tools_each_with_its_annotations`
        - `every_property_an_input_schema_declares_is_accepted_by_its_tool`
        - `an_unknown_tool_is_invalid_params_and_every_tool_failure_is_the_envelope_with_is_error`
        - `an_annexed_field_sent_to_a_served_operation_answers_its_v0_3_0_code`
        - `the_plugin_skill_names_only_served_tools`
        - `the_readme_permission_rules_name_only_served_tools`
        - `the_session_hook_names_only_served_tools`

       The hook's lines become:

        - "OML memory for this session, from {server}, in this key's default workarea
          (oml_workarea names it)."
        - "To record a durable fact or decision, call oml_write with one checkable claim, its
          source and date, and an idempotency_key such as <task>/<date>/<topic>; give a
          claim_key only to name a claim you will revise; never a secret."
        - "The conversation was just compacted. Before continuing, record with oml_write the
          durable facts and decisions from the summary that are not in OML yet."
        - "- {n} more matched beyond the {max_tokens}-token budget; oml_read shows them."

       and the CLI reference's note that `oml_get` serves a flattened entry whole names
       `oml_read` with `artifact_id`.
    2. **The write and read rules.** Tests:
        - `a_write_without_claim_key_is_stored_under_the_key_derived_from_its_author_and_text`
        - `the_same_key_writing_the_same_text_twice_in_one_workarea_meets_claim_key_conflict`
        - `a_derived_claim_key_conflict_reveals_no_other_keys_text`, in which a second key writing
          a held row's exact text without `claim_key` is stored as its own claim
        - `an_explicit_claim_key_starting_with_auto_is_refused_naming_claim_key`
        - `a_write_without_idempotency_key_is_refused_as_request_validation`
        - `a_replayed_write_answers_idempotent_replay_true_with_the_first_response`
        - `a_write_with_derives_from_is_journalled_as_submission_learn`
        - `a_write_with_replaces_and_claim_key_is_refused_naming_claim_key`
        - `a_write_replacing_a_vocabulary_kind_target_is_refused`
        - `retirement_without_replaces_is_refused_naming_replaces`
        - `a_read_with_artifact_id_and_query_is_refused_naming_both`
        - `a_read_argument_outside_its_mode_is_refused_naming_it`
        - `a_read_with_neither_lists_the_keys_own_work_newest_first`
        - `a_renamed_request_field_is_refused_naming_its_new_spelling`
    3. **The aliases (d12), once confirmed.** Tests:
        - `every_v0_3_0_tool_name_answers_with_its_v0_3_0_output_shape`
        - `no_v0_3_0_tool_name_appears_in_tools_list`
        - `each_old_cli_verb_runs_its_new_verb_and_names_it_on_stderr`
    4. **The CLI verbs.** `oml write`, `oml read`, `oml review approve|reject <serve_id>`;
       reference.md lists every built verb, the old ones marked deprecated. Tests:
        - `review_approve_sends_the_serve_id_review_next_printed`
        - `the_session_hook_reads_the_v0_3_2_recall_shape`
        - `the_cli_reference_lists_every_built_verb` (a docs check over `main.rs`)
    5. **The SDKs.** `python3 scripts/generate_sdks.py` regenerates both from the served
       openapi.yaml; the wrappers take d10's names; sdks.md's "What the wrapper covers" changes.
       Tests:
        - `the_generated_sdk_carries_no_operation_the_server_does_not_route`
        - `the_agent_loop_runs_through_write_read_workarea_and_events` (replacing
          `test_agent_loop.py`'s calls)
    6. **The docs.** The quickstart's CLI verbs, the README and the changelog's breaking-change
       list follow. Test: the end-to-end CI step "Quickstart, then the agent plugin over /mcp"
       passing on the new CLI verbs.

## Consequences

- **The surfaces change at build, not now.** openapi.yaml, mcp-tools.json, http-api.md,
  errors.md, mcp-plugin.md, cli/reference.md, sdks.md and the plugin skill keep the v0.3.0
  contract until step 1 of d14 lands, so that design and code never disagree in a landed
  change. Until then this record is the new contract's only statement.
- **Status-line pointers at acceptance.** ADR-0014, ADR-0022, ADR-0025 and ADR-0029 gain
  `· amended by ADR-0039`, and the D13 row of the [D-register](index.md) names this record.
- **What the agent learns shrinks.** Four tools instead of eight, one workarea argument, and no
  invented `claim_key`. The served catalog's character count is not measured; the planning lane
  measured 18,893 characters for today's twelve entries, and the build records the new figure in
  the changelog.
- **Rebases owed.** The proposed ADR-0027, ADR-0030, ADR-0031, ADR-0035, ADR-0036, ADR-0037 and
  ADR-0038 name the old tools or fields, or add operations to openapi.yaml. Each rebases onto
  d2's names, d7's spellings and d9's two documents, and ADR-0030 and ADR-0031, which move
  `info.version` to `0.4.0-design`, move it from `0.3.2-design`.
- **The serve binding is not fixed here.** The server builds `serve_id` from the memory's own id
  (`lib.rs:669-673`) and reads the id back out of it (`lib.rs:719-723`); the verdict is bound to
  the reviewer only through the queue row it claimed, not to a serve record with its digests and
  expiry as the [HTTP API's review section](../api/http-api.md#review) designs. d10 makes the CLI send the `serve_id`
  that `review next` printed, so the CLI no longer depends on that shape. Building the binding is
  FR-66.
- **Opened:** FR-65 (the rewrite, built and released as v0.3.2, and the aliases removed in
  v0.4.0) and FR-66 (the serve binding) on the [roadmap](../product/roadmap.md), marked proposed.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Layering: keep every route, tool and field, and add markers, wrappers and aliases on top** (the delivery plan's recommendation) | The maintainer chose the rewrite on 2026-11-03. Layering keeps four names for one read and three spellings of one argument for good, and after FR-21's freeze they could never be removed from `/v1`. Its strongest argument, that renaming breaks permission rules and installed plugins, is answered by d12's window and d13's migration at a cost paid once, while only one deployment is live. |
| **One `x-oml-built` marker per operation in a single openapi.yaml** | The generators would still emit the 49 unrouted operations, so the SDKs would still offer calls the server answers with 404, which is the pretend contract d9 removes. The marker also needs every reader to know to look for it. |
| **Delete the 45 designed operations outright** | They are decided design, cited from accepted records (ADR-0010, ADR-0011, ADR-0020, ADR-0021). Deleting them would lose the decisions, not just the declarations. The annex keeps them out of the contract and in the design. |
| **One MCP tool for everything** | A read-only tool and a write tool differ in what Claude Code may run without asking and in parallel (d5). One tool would carry the write's hints on every read, and one permission rule could no longer allow reads while asking for writes. |
| **Keep `oml_remember`, `oml_learn` and `oml_supersede` as three write tools** | They are one endpoint and one supersede path whose schemas differ in one required field and one boolean. The verb follows from the lineage the caller declares, and the supersede path from the version it replaces. |
| **Fold `oml_workarea` and `oml_events` into `oml_read`** | Neither returns claim items: one returns a node with the caller's verbs, the other an event page with gap semantics. One tool with three output shapes costs the agent more than a fourth tool name. |
| **Make `idempotency_key` optional, defaulting to a digest of the arguments** | Two deliberate identical writes would collapse into one, and the maintainer chose to keep it required on 2026-11-03. |
| **Keep the old names for longer than one minor release, or on HTTP too** | Every release with aliases is a second surface to test. HTTP callers are the CLI and the SDKs, which ship in lockstep with the server or install from a checkout; the MCP window covers scripts that call a tool by name, and Claude Code itself takes the new names from `tools/list`. |
| **Derive `claim_key` from the text alone** | Any writer could then submit a guessed sentence and learn from `submission.claim_key_conflict` whether another key's held row, or a row it cannot read, holds exactly that text: an exact-text oracle that breaks ADR-0025 d5 and FR-31 (d6). |
| **Leave the simulation fields on the served writes** | No simulation can exist in this build, and the server refuses every `simulation_id` (`lib.rs:1973-1976`). A field that can only be refused is part of the pretend contract d9 removes; it returns with the build of simulations. |
