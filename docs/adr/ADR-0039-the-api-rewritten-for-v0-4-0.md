<!--
id: adr/ADR-0039-the-api-rewritten-for-v0-4-0
tier: engineering
status: accepted design · 2026-11-04
answers: why the agent and HTTP surfaces are rewritten for v0.4.0 while OML is pre-release instead of layered; the three MCP tools that replace the eight served ones, their hints and their argument rules; why the events tail leaves MCP and no file tool returns; the server-derived claim_key beside a required idempotency_key; one workarea field in place of the X-OML-Workarea header; one include_unreviewed flag; which HTTP routes collapse and where the 49 declared-but-unrouted operations go; the old-to-new mapping that shows nothing is lost; the breaking changes, their migration and the alias window; one error shape on HTTP and MCP; the URL and key that are every client's only start settings, and the whoami read that derives the rest; the build plan and its tests
-->

# ADR-0039 — The API is rewritten for v0.4.0 while OML is pre-release: three task-shaped MCP tools, a contract that declares only what is served, one URL and one key to start, and nothing lost

**Status:** accepted design · 2026-11-04 · amended by [ADR-0067](ADR-0067-a-directory-reviewer-reaches-a-demo-workspace-through-a-connect-code-that-lives-longer-and-the-tools-and-instructions-describe-instead-of-command.md) (d5: each tool also carries a `title`), [ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md), [ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) · amended by [ADR-0076](ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md) (d15) · amended by [ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md) (d15: `whoami` tells a `<user>-web` session key whether it carries a denial; accepted 2026-11-12) · amended by [ADR-0080](ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) (d3, 2026-11-14), [ADR-0081](ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md) (d3, d4, 2026-11-14)

**Release:** introduced in v0.4.0 ([releases](../changelog.md))

**Amends:** ADR-0014 (d2), ADR-0022 (d2, d4, d6), ADR-0025 (d4, d12, d13), ADR-0028 (d1), ADR-0029 (d2, d6, d8)

## Context

On 2026-11-03 the maintainer chose to **rewrite the API while pre-release**, first as a patch
release and then, the same day, as v0.4.0 ("we should be targeting 0.4.0"), over
the delivery plan's layering of markers, wrappers and aliases, then settled the first draft's
open questions and took six findings of the use-case call map, which applies Occam's razor to
every use case (`docs/engineering/use-case-call-map.md` on `feat/use-cases`, not on this branch).
Every decision below is the maintainer's. Five facts, checked against `feat/v0.3.1` (`beba0fc`):
[openapi.yaml](../api/openapi.yaml) declares 71 operations, the router serves 22 plus `/mcp`
(`crates/transports/oml-transport-http/src/lib.rs:192-225`), and the SDKs generated from all 71
offer calls answered with 404. The own-work listing has four names, and a workarea seven
spellings, the `X-OML-Workarea` header among them ([mcp-tools.json](../api/mcp-tools.json)). The
writes are one endpoint behind three tools (`mcp.rs:287-288`), each needing a `claim_key` the
agent invents. Four built CLI verbs are in no row of [the CLI reference](../cli/reference.md),
which documents `bootstrap claim --token-stdin` where the build takes `--token <value>`
(`crates/bins/oml-cli/src/main.rs:120-122`). And the CLI starts from `OML_CLI_SERVER` and
`OML_CLI_KEY` and stores five fields (`main.rs:35-38`, `creds.rs:14-25`), while the plugin, the
hook and the agent kit read `OML_URL` and `OML_API_KEY`.

The v1 covenant binds only from #150's v1-stable declaration
([HTTP API](../api/http-api.md#versioning-and-deprecation)). Against a rewrite, v0.3.0 is live on
a public server, and a renamed tool breaks users' permission rules for
`mcp__plugin_oml_oml__<tool>` ([ADR-0029](ADR-0029-session-start-hook.md) d8). This record pays
those costs once, before the freeze. Decided, not built.

v0.4.0 also carries three changes this record does not decide, each from its own branch and
pull request: turn context (ADR-0038), the mobile OAuth connector (ADR-0031) and the librarian
subagent. Each rebases onto this record's build, d14's steps, rather than ahead of it.

## Decision

- **d1 — While #150 is open, the contract may be rewritten, and every rewrite carries its
  mapping, its breaking-change list and its migration in the same release.** A rename or removal
  is allowed when the release (a) maps every removed capability to its successor, as d10 does,
  (b) lists every break in the changelog with its migration, and (c) states its alias window
  (d12). A removal without a successor needs its own record. #150 stays open, and d10's table
  is the test a reviewer runs for "without loss"; every early adopter migrates once.

- **d2 — The served MCP catalog is three tools: `oml_write`, `oml_read` and `oml_workarea`.**

    | Tool | Task | Rides |
    |---|---|---|
    | `oml_write` | store a claim, a derived claim, a new version, or a proposal to retire one | `POST /v1/memories`, or `…/{artifact_id}/supersede` when `replaces` is sent |
    | `oml_read` | find, fetch or list claims | `POST /v1/memories/recall`, `GET /v1/memories/{artifact_id}`, `GET /v1/memories` |
    | `oml_workarea` | see one workarea as this key may see it | `GET /v2/workareas/{workarea}` |

    `my_work` folds into `oml_read`, the same ring read; `oml_workarea` returns a node, not claim
    items. **The events tail leaves MCP** (call map C1): an agent's one use for it, learning
    whether a write was approved or rejected, is `oml_read` with `state` over its own work, with
    no cursor and one permission rule fewer. It stays on `GET /v2/events`, the SDKs' `events` and
    a new `oml events`, for auditors. `mcp-tools.designed.json`, never served, holds
    `oml_relate_propose`, `oml_vocab` and the tools' designed arguments. **`oml_graph`** becomes
    `oml_read`'s designed lineage mode, lineage returning claim items (call map M4).
    **`oml_attach`** has no successor tool: MCP carries no file bytes, so an agent would still
    need a terminal for the parts (call map C2). Files go through the designed `oml attach put
    <file>` and the SDKs' upload, whose session `oml_write`'s designed `attachments` names.

- **d3 — `oml_write` takes one flat argument set, and the server chooses the endpoint and the
  verb from which fields are present.** `text` and `idempotency_key` are required; `claim_key`,
  `kind` (default `memory`, call map D3), `derives_from`, `workarea`, `replaces`, `retirement` and
  `reason` are optional. Without `replaces` it is a submission, journalled `submission.learn`
  when `derives_from` is non-empty and `submission.remember` otherwise, as
  [the HTTP API](../api/http-api.md#submit-post-v1memories) defines `learn`; `retirement` and
  `reason` are refused. With `replaces` it supersedes that version, and `retirement: true` makes
  it ADR-0025 d11's retire-proposal, `text` stating why; refused are `claim_key` (the successor
  takes the target's), a `workarea` not the target's (`app.rs:2744-2750`), a `kind` other than
  `memory`, `derived_fact` or `steering`, and a vocabulary-kind target, since the facade sets
  `refuse_vocabulary_target` as `oml_supersede` does ([ADR-0021](ADR-0021-ai-human-ontologies.md)).
  Each refusal is `request.validation` with `details.field` (ADR-0025 d16), amending ADR-0025
  d12's "no conditionally required fields" to one rule per flat field. The output is `{mode,
  artifact_id, claim_key, version, state, workarea, retirement, replaces, idempotent_replay}`.

- **d4 — `oml_read` chooses its mode the same way: `artifact_id` fetches, `query` recalls, and
  neither lists.** An argument outside its mode, or both `artifact_id` and `query`, is refused
  naming it:

    | Mode | Arguments | Rides |
    |---|---|---|
    | get | `artifact_id`; designed: `lineage` (`ancestors` \| `descendants`), `depth` | `GET /v1/memories/{artifact_id}`; the designed lineage read |
    | recall | `query`, `workarea`, `k`, `kinds`, `include_unreviewed`, `max_tokens` | `POST /v1/memories/recall` |
    | listing | `own` (default `true`), `workarea`, `claim_key`, `state`, `kind`, `since`, `until`, `limit`, `cursor` | `GET /v1/memories` |

    `workarea` names the subtree read. Omitted, recall and the scoped listing (`own: false`, new
    on MCP) read the key's default workarea, and the own-work listing everything the key wrote.
    Every mode returns ADR-0025 d12's claim item, with its text, under a `mode` field. The lineage
    mode is served once the lineage read is routed. **One flag for unreviewed rows** (call map
    M3): `include_unreviewed` replaces `include_own_held` everywhere; in v0.4.0 it reaches the
    key's own held rows (ADR-0025 d4), default `false`. The proposed ADR-0030 widens its reach and
    sets its default inside an in-progress subtree, under this name, with no flag of its own.

- **d5 — Each tool declares its MCP annotations.** `oml_read` and `oml_workarea` carry
  `readOnlyHint: true` and `idempotentHint: true`; `oml_write` carries `readOnlyHint: false`,
  `destructiveHint: false` and `idempotentHint: true`; all three `openWorldHint: false`. A write
  is held for review and a retirement is a proposal; `idempotency_key` makes a repeat replay.
  `oml_write` keeps `destructiveHint: false` because a call only adds a held row and never changes
  or deletes an existing one ([ADR-0067](ADR-0067-a-directory-reviewer-reaches-a-demo-workspace-through-a-connect-code-that-lives-longer-and-the-tools-and-instructions-describe-instead-of-command.md) d1). Each tool also carries a `title`, Remember, Recall and
  Workarea, which the Connectors Directory asks for.
  Claude Code runs read-only calls in parallel and may repeat a pending idempotent one
  ([MCP in Claude Code](https://code.claude.com/docs/en/mcp), fetched 2026-11-03).

- **d6 — The server derives `claim_key` from the author and the text when it is omitted, and
  `idempotency_key` stays required. Confirmed by the maintainer on 2026-11-03.** The derived key
  is `auto-` plus the first 16 lowercase hex digits of the SHA-256 of the authoring key's id, a
  zero byte and the NFC-normalized text; once rotation is built, the id is the root of the key's
  `rotated_from` chain ([roles and keys](../architecture/roles-and-keys.md#own)). An explicit
  `claim_key` starting `auto-` is refused, and the response names the key. The rule lives on
  `POST /v1/memories`, so the CLI stops deriving `cli-` keys; a supersession derives none. With
  the author's id in the digest, `submission.claim_key_conflict` tells a writer only about its
  own rows, not whether a row it cannot read holds a guessed text (ADR-0025 d5, #129); two
  agents recording one fact leave two claims for the reviewer.

- **d7 — One spelling per concept in requests on every surface; responses keep their field
  names, except `body`.** A request field that references a workarea, crew, key or tenant is
  named after the kind and takes an id or a name ([ADR-0026](ADR-0026-names-are-keys.md) d4):

    | Where | v0.3.0 | v0.4.0 |
    |---|---|---|
    | `POST /v2/keys` | `default_workarea_id`; `tenant_id`; each grant's `workarea_node_id`; `deny[].subtree_node_id` | `default_workarea`; `tenant`; `workarea`; `deny[].workarea` |
    | `POST /v2/keys/{key}/denials`; `POST /v2/workareas` | `workarea_node_id`; `parent_id` | `workarea`; `parent` |
    | `POST /v1/memories`, `…/supersede`, `…/recall` | header `X-OML-Workarea` | body field `workarea` |
    | `GET /v1/memories`, `GET /v2/events` | header `X-OML-Workarea` and query `workarea_id` | query `workarea` |
    | recall, every surface; MCP `oml_recall`, `oml_events_tail` | `include_own_held`; `scope` | `include_unreviewed` (d4); `workarea` |
    | path parameters | `{workarea_id}`, `{crew_id}`, `{key_id}` | `{workarea}`, `{crew}`, `{key}` |

    **The header goes (call map M7).** ADR-0022 d4's `X-OML-Workarea` is a second form of an
    argument every other request carries as a field, and on the listing it sat beside a
    `workarea_id` filter with another meaning. One `workarea` field, the subtree the call reads or
    writes in, replaces both, with d4's defaults. Responses keep their v0.3.0 names, since a
    response names a stored record by its id where a request may use the name; the one rename is
    `body` → `text` (`include_bodies` → `include_text`), "body" staying
    [ADR-0004](ADR-0004-bodies-and-attachments.md)'s storage word.

- **d8 — A new agent capability is a parameter, a resource or a hook before it is a tool.**
  ADR-0035, ADR-0027 and ADR-0037 already reject a tool each for a parameter; a fourth served
  tool needs a record saying why no parameter of the three, no resource and no hook carries it.

- **d9 — The HTTP contract declares only what the server routes; the rest is a designed annex.**
    - **Served.** openapi.yaml holds exactly the routed operations, today's 22 and d15's `whoami`,
      each with only the fields the build accepts and returns, and the complete `ErrorCode` enum.
    - **Designed.** `docs/api/openapi-designed.yaml` holds under `paths` the 45 designed
      operations, and under `x-oml-designed-fields`, keyed by `operationId`, served operations'
      designed fields: the vocabulary fields, `attachments`, `simulation_id` with
      `simulated_time`, and `getWorkarea`'s review-policy and pending-transition fields. Nothing
      is generated from it; an entry moves to openapi.yaml in the change that serves it, and a
      designed field sent to a served operation answers its v0.3.0 code. `check_contract_sync.py`
      compares http-api.md with the union of both documents' `paths` and fails on a path in both.
    - **Removed and merged.** `GET /v2/workareas/{id}/status` duplicates the workarea read and goes.
      The claim-edge and workarea-edge families, three operations each, become one designed
      `/v2/edges` family of three with `kind` ∈ `claim` | `workarea`, each kind keeping its
      authority, refusal codes and journal events ([HTTP API](../api/http-api.md#the-surface)):
      49 − 1 − 3 = 45. `searchMemories` → `listMemories`, `proposeSupersession` →
      `supersedeMemory`, `updateCrewMetadata` → `setWorkareaCrew` (`lib.rs:218-221`).

- **d10 — Every served tool, route, CLI verb and SDK method maps to its successor.**

    | v0.3.0 | v0.4.0 |
    |---|---|
    | `oml_remember`, `oml_learn` `{workarea, claim_key, text, kind, derives_from, idempotency_key}` | `oml_write`, `claim_key` and `kind` optional; `derives_from` journals learn (d3) |
    | `oml_supersede {artifact_id, …, retirement?, reason}` | `oml_write {replaces, …, retirement?, reason}` |
    | `oml_recall {query, scope, k, include_own_held, max_tokens}` | `oml_read {query, workarea, k, include_unreviewed, max_tokens}`, and `kinds` |
    | `oml_get {artifact_id}` | `oml_read {artifact_id}` |
    | `oml_my_work {workarea, claim_key, state, limit, cursor}` | `oml_read` listing, and `kind`, `since`, `until`; `own: false` is new on MCP |
    | `oml_workarea_info {workarea}` | `oml_workarea {workarea}` |
    | `oml_events_tail {scope, after_seq, limit}` | `GET /v2/events`, SDK `events`, `oml events`; for an agent's own writes, `oml_read {state}` (d2) |
    | `attachments`, `attachments_bound`; `simulation_id`, `simulated_time` | the designed annex (d9), answering their v0.3.0 codes |
    | `oml_attach`, `oml_graph` (listed, never served) | the designed upload verbs; `oml_read`'s lineage mode (d2) |
    | `oml_relate_propose`, `oml_vocab` (listed, never served) | `mcp-tools.designed.json` |

    **HTTP routes.** All 22 paths stay. Beyond d9's operationId renames, d7's field renames and
    `body` → `text`: `POST /v1/memories` takes `claim_key`, `verb` and `kind` as optional, an
    explicit `verb` still honoured; `POST /v2/review/verdicts` takes `serve_id` as optional,
    defaulting to the reviewer key's one open review item and refused naming `serve_id` when it
    holds none or several (call map D5); `GET /v2/whoami` is added (d15).

    | CLI, v0.3.0 | v0.4.0 |
    |---|---|
    | `oml submit --claim <text> [--verb] [--claim-key] [--derives-from …] [--workarea]` | `oml write <text> [--claim-key] [--kind] [--derives-from …] [--workarea]` |
    | `oml supersede …` | `oml write <text> --replaces <artifact_id> [--retirement] [--reason]` |
    | `oml recall --query <q> [--k] [--workarea]` | `oml read --query <q> …`; `oml read <artifact_id>`; `oml read [--own\|--all] [filters]` |
    | `oml review verdict --memory <uuid> --approve\|--reject [--reason]` | `oml review approve [<serve_id>] [--note]`, `oml review reject [<serve_id>] --reason <text>` |
    | — | `oml events [--after-seq] [--workarea] [--limit]`; `oml login <url>` (d15) |
    | `--server`, `--key`; `OML_CLI_SERVER`, `OML_CLI_KEY` | `--url`, `--api-key`; `OML_URL`, `OML_API_KEY` (d15) |
    | `oml bootstrap claim <key-name> --token <secret>` | `oml bootstrap claim <key-name> --url <https://…> --token-stdin` |

    `--token-stdin` keeps the enrollment secret out of argv and shell history, and `--token` gets
    no alias, which would keep that path open. Other verbs keep their names, with d7's flags.
    **SDKs:** the wrappers take `write`, `read`, `workarea`, `events`, `review_next`, `approve` and
    `reject`, replacing `remember`, `learn`, `supersede`, `recall`, `get`, the listings and
    `verdict`; the raising stub `upload_attachment` leaves until uploads are built.

- **d11 — One error shape on HTTP, MCP, the CLI and the SDKs.** Every refusal is the envelope of
  [errors.md](../api/errors.md#the-envelope). On MCP a failing tool answers `isError: true` with
  the envelope in `structuredContent` and as its text (`mcp.rs:314-325`); JSON-RPC errors are for
  protocol faults, -32602 for a tool not served. No code is added: a renamed field or header is
  `request.validation` with `details.renamed_to`.

- **d12 — The old names stay callable but hidden through v0.4.x on MCP and the CLI only, and
  v0.5.0 removes them.** The maintainer confirmed a window on 2026-11-03 while the rewrite was a
  patch release: hidden through v0.3.x, removed in v0.4.0. Moving the rewrite to v0.4.0 the same
  day left that window empty, so this record reads it as one minor release later. **The
  maintainer has not decided this reading.** The other reading is no aliases at all: v0.4.0
  answers only the new names, and d14's step 3 and the alias rows of d13 drop out.
    - **MCP.** The eight v0.3.0 tool names keep their v0.3.0 inputs, outputs and refusal codes,
      translated at the facade, `oml_events_tail` onto `GET /v2/events`. None is in `tools/list`;
      each call logs `mcp_alias_called`. The window serves raw MCP clients such as
      `scripts/quickstart.sh`; Claude Code offers the model only what `tools/list` names.
    - **CLI.** `oml submit`, `oml recall`, `oml supersede`, `oml review verdict`, `--server` and
      `--key` stay hidden, each printing one stderr line naming its successor. `OML_CLI_SERVER`
      and `OML_CLI_KEY` are not read; when a start setting is missing and either is set, the
      failure names the variable to rename.
    - **HTTP and the SDKs: no aliases.** The CLI and the hook ship with the server, and neither SDK
      is on a registry ([client SDKs](../engineering/sdks.md)); each moves to `0.2.0`.
    - **The removal moves to v0.5.1 (2026-11-04).** v0.5.0 keeps the window open: the aliases
      are not one contained deletion, since 30 of the facade's Postgres suite's end-to-end tests
      (`crates/bins/oml-server-lead/tests/mcp_facade.rs`) still drive the MCP surface through the
      v0.3.0 names and assert their v0.3.0 shapes, and each must first move to `oml_write`,
      `oml_read` and `oml_workarea`. v0.5.1 rewrites those tests on the three tools and then
      removes the eight names, the four verbs, `--server`, `--key` and a v0.3.0 credentials file's
      `server` and `key` (d15) together.
    - **Removed in v0.5.1 (2026-11-04).** The facade tests drive `oml_write`, `oml_read` and
      `oml_workarea`, and then the window closed: each of the eight names answers JSON-RPC -32602
      with the `Unknown tool` message, as any tool not served does; `mcp_alias_called` and the
      `mcp_call` line's `alias_of` field are gone; the four verbs, `--server` and `--key` are the
      CLI's usage errors, exit 2, with nothing sent; and a v0.3.0 credentials file's `server`
      and `key` are not read.

- **d13 — The breaking changes are these, and each has its migration.**

    | Break | Who it reaches | Migration |
    |---|---|---|
    | Three tool names replace the eight | `permissions.allow` rules naming old read tools | allow `mcp__plugin_oml_oml__oml_read` and `__oml_workarea`, or the server-wide `mcp__plugin_oml_oml`; the README carries both (amends ADR-0029 d8) |
    | The events tail leaves MCP | callers of `oml_events_tail` | `oml_read {state}` for an agent's own writes; `GET /v2/events`, SDK `events` or `oml events` to audit; the alias answers until v0.5.0 |
    | The cached skill and the hook name old tools | plugin 0.3.0 installs | `claude plugin update oml@open-memory-lab` and the v0.4.0 CLI (amends ADR-0029 d6) |
    | Request renames and the header removed (d7), `auto-` keys refused (d6) | HTTP callers, the CLI, the hook | upgrade the CLI with the server; the refusal names the new spelling. The hook sends `include_text` and `include_unreviewed` (amends ADR-0029 d2) |
    | `body` → `text` in responses | HTTP readers, the SDKs | read `text` |
    | `remember` with `derives_from` journalled `submission.learn` | journal readers | none; an explicit HTTP `verb` keeps the old event |
    | Generated SDK modules renamed; the 49 unrouted operations gone | SDK users | the wrapper's `write` and `read`; the removed calls never reached a route |
    | CLI verbs, flags and variables renamed; `--token` removed | scripts, the README, shell profiles | the new names; `oml login <url>` once; `--token-stdin` |
    | a server on v0.3.0 | its operator | upgrade server, CLI and plugin together, configure the plugin ([ADR-0040](ADR-0040-plugin-setup-in-claude-code.md)), replace the permission rules, record the upgrade with its date |

    Keys, grants, stored memories, the journal and the schema do not change.

- **d14 — The build is one change set in five steps; the four doc gates and the CI step
  "Quickstart, then the agent plugin over /mcp" pass after every step,** so whatever names a tool
  or a field moves in the step that renames it. Each step closes on the tests named.

    1. **The contract and the facade, with everything that names them**, in one commit:
       openapi.yaml (served only, with `whoami`), `openapi-designed.yaml`, mcp-tools.json (three
       tools with hints), `mcp-tools.designed.json`, http-api.md, errors.md, mcp-plugin.md, the
       router, the facade, the skill, the README's permission rules, `check_plugin.py`, the
       quickstart, the CLI's and the hook's request spellings and the hook's lines; `info.version`
       becomes `0.4.0-design`. `the_served_openapi_document_declares_exactly_the_routed_operations`,
       `tools_list_names_three_tools_each_with_its_annotations`,
       `the_x_oml_workarea_header_is_refused_naming_workarea`,
       `whoami_answers_any_live_key_with_its_tenant_role_name_and_default_workarea`.
    2. **The write, read and review rules.**
       `a_write_without_claim_key_is_stored_under_the_key_derived_from_its_author_and_text`,
       `a_derived_claim_key_conflict_reveals_no_other_keys_text`,
       `a_read_argument_outside_its_mode_is_refused_naming_it`,
       `a_verdict_without_serve_id_applies_to_the_reviewers_one_open_item_and_is_refused_with_two`.
    3. **The aliases (d12).** `every_v0_3_0_tool_name_answers_with_its_v0_3_0_output_shape`,
       `each_old_cli_verb_runs_its_new_verb_and_names_it_on_stderr`.
    4. **The CLI** (d10, d15). `the_cli_reference_lists_every_built_verb`,
       `a_flag_beats_the_environment_and_the_environment_beats_the_file`,
       `login_stores_only_url_and_key_owner_only_and_nothing_when_whoami_refuses`,
       `bootstrap_claim_reads_the_secret_from_stdin_and_takes_no_token_flag`.
    5. **The SDKs and the docs**: `generate_sdks.py`, d10's wrapper names, sdks.md, the READMEs,
       the agent kit and the changelog's break list.
       `the_generated_sdk_carries_no_operation_the_server_does_not_route`, and the CI step.

- **d15 — A client starts from a URL and a key; the server derives everything else.** The
  maintainer's rule of 2026-11-03: "cli tool should have mainly api key and host name where it
  should connect to in order to start using, same for Claude and other plug-in".
    - **One pair**, `OML_URL` (without `/mcp`) and `OML_API_KEY`, on every client: the CLI (flags
      `--url`, `--api-key`), the plugin's options ([ADR-0040](ADR-0040-plugin-setup-in-claude-code.md)
      d1), the hook's fallback (ADR-0040 d3), and the agent kit, whose configurations read both
      where the tool expands a variable in a URL and whose README names the one edit elsewhere.
    - **Flag, then environment, then the credentials file**, the pair taken from the first source
      that sets either value; a source setting one alone is refused naming the other, so a key is
      never sent to a URL from another source.
    - **`oml login <url>`** reads the key without echo from the terminal, or from stdin when that
      is not a terminal, calls `whoami`, and only on success writes `url` and `api_key`, nothing
      else, to the owner-only credentials file. It replaces the designed `oml context add`, `oml
      doctor` replaces `oml context show`, and a second server is a second file through
      `OML_CLI_CREDENTIALS`. `bootstrap claim` writes the same two fields; a v0.3.0 file's
      `server` and `key` are read through v0.4.x and its other fields ignored. **That window
      follows d12's reading, which the maintainer has not decided**; under the other reading
      v0.4.0 reads only `url` and `api_key`.
    - **`GET /v2/whoami` derives the rest**, since no served read does: `getKey` is admin-only and
      needs the key's own reference, and `getWorkarea` names no tenant, role or key. Any live key
      calls it for itself and gets `{key_id, name, role, principal, held_by, tenant,
      default_workarea, crew, grants}`, each reference as `{id, name}` and the workarea with its
      `path`, and each grant as `{grant_id, workarea_node_id, role, epoch}`. It omits denials and
      the grants' per-grant deny masks, both of which name subtrees the key may not read, logs no
      read, since it reads no claim, and gains the guardrail scope when ADR-0037's is built.
      Amended by [ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)
      (2026-11-12, #343; accepted by the operator the same day): a `<user>-web` session key's
      record also carries `denied`, a boolean: whether the key holds a denial in force on any
      workarea. It names no workarea, no count, no author and no time. Every other key's record
      has no such member, so an agent key or an admin key still learns nothing of its denials.
      The flag tells a person nothing the refusal of a key act does not already tell them.
    - **`oml doctor` checks the pair alone**: each value's source, never the key; `/readyz`; and
      `whoami`, printing role, tenant and default workarea.
    - **Admin extras stay off the start path**: the enrollment secret and `--signing-key` are read
      only by the verbs that take them. A new client setting is derivable through `whoami` or
      defaulted, or its record says why it is neither.

## Consequences

- **The surfaces change at build, not now**: the contract pages, the CLI reference, sdks.md and
  the skill keep v0.3.0 until d14's step 1. At acceptance the amended records gain `· amended by
  ADR-0039`, and the D13 and D14 rows of the [D-register](index.md) name it.
- **What the agent learns shrinks** to three tools, one `workarea` and no invented `claim_key`
  (18,893 characters for today's twelve catalog entries; the build records the new figure).
- **Rebases owed.** The proposed ADR-0027, ADR-0030, ADR-0031, ADR-0035, ADR-0036, ADR-0037 and
  ADR-0038 rebase onto d2, d7 and d9: ADR-0030 uses `include_unreviewed` (d4) rather than its own
  flag, and ADR-0035 names files through the upload verbs. ADR-0031, which lands in v0.4.0, bumps
  `info.version` from `0.3.0-design` to `0.4.0-design` in its d15; that is the same bump as d14's
  step 1, made once, under one `0.4.0-design` changelog section. ADR-0030, which is not in v0.4.0,
  moves `info.version` from `0.4.0-design` to the next version its own record decides
  (`0.5.0-design` unless it says otherwise).
- **The serve binding is not fixed here.** `serve_id` is built from the memory's id
  (`lib.rs:669-673`) and binds the reviewer only through the claimed queue row, on which d10's
  default rests, not a serve record ([review](../api/http-api.md#review)); that is #130.
- **Opened:** #130 (the serve binding).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Layering on every route, tool and field** (the delivery plan) | Four names for one read for good, unremovable from `/v1` after #150; the broken rules it avoids are paid once by d12 and d13. |
| **One `x-oml-built` marker per operation in one openapi.yaml** | The generators would still emit the 49 unrouted operations. |
| **Delete the 45 designed operations** | They are decided design cited from accepted records (ADR-0010, ADR-0011, ADR-0020, ADR-0021). |
| **One MCP tool for everything, or three write tools** | Reads and writes differ in what Claude Code runs without asking (d5); the three writes are one endpoint and one supersede path. |
| **Keep `oml_events` on MCP** (the first draft's d2) | `oml_read {state}` answers an agent's one use with no cursor, and the tool costs a permission rule (call map C1). |
| **Fold `oml_workarea` into `oml_read`** | It returns a node with the caller's verbs, not claim items. |
| **Keep `oml_graph`, its fold left to a later record** (the first draft) | Lineage returns claim items, so it passes d4's one-shape test as a mode (call map M4). |
| **`oml_attach` returning when uploads are built** | MCP carries no file bytes; the agent would still need a terminal for the parts (call map C2). |
| **Keep `X-OML-Workarea` beside the `workarea` field** (the first draft's d7) | Two forms of one argument, with different meanings on the listing (call map M7). |
| **Keep `include_own_held` beside `include_unreviewed`, or name the one flag `include_held`** | Two flags for one question; ADR-0030 already uses `include_unreviewed`, and the maintainer chose it. |
| **`kind` required on `POST /v1/memories`** | `memory` is MCP's default already and covers the common write (call map D3). |
| **`serve_id` required on approve** (the first draft's d10) | The reviewer copies what `review next` printed though it holds one item (call map D5). |
| **Make `idempotency_key` optional** | Two deliberate identical writes would collapse into one. |
| **Derive `claim_key` from the text alone** | An exact-text oracle through `submission.claim_key_conflict` (d6). |
| **Aliases past v0.3.x, or on HTTP and the SDKs** (as the maintainer confirmed it while the rewrite was a patch release; d12 reads the bound as v0.4.x, a reading the maintainer has not decided) | Each aliased release is a second surface to test; HTTP callers ship with the server or install from a checkout. |
| **`bootstrap claim --token <secret>`, as built** | The secret enters argv and shell history. |
| **Keep `OML_CLI_SERVER` and `OML_CLI_KEY` beside the plugin's pair** (call map K7) | Two names for one pair. An admin key still never feeds an agent's context: the hook never reads the credentials file (ADR-0040 d3), and an admin key recalls no claim content ([roles and keys](../architecture/roles-and-keys.md)). |
| **Resolve the URL and the key independently across sources** | A flag's URL could be paired with an exported key meant for another server. |
| **`GET /v2/keys/me` for the self read** | `me` fits the name grammar (ADR-0026 d2), so a tenant may hold it, and the key read carries denials. |
| **Keep tenant and workarea ids in the credentials file** (v0.3.0) | They go stale when an admin changes the key's grants. |
| **Leave the simulation fields on the served writes** | The server refuses every `simulation_id` (`lib.rs:1973-1976`). |
