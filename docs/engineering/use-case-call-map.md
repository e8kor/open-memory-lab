<!--
id: engineering/use-case-call-map
tier: engineering
status: design draft for review · decided, not built
answers: for every use case on the use-cases page, its UC id, its status and the calls it needs today on MCP, HTTP, the CLI and the SDKs; the minimal client call set by Occam's razor that serves every use case; every cut, merge and default candidate in today's and the designed surface; and which of ADR-0039's proposals the map supports or contradicts
-->

# Use-case call map

**Status:** design draft for review · decided, not built

This page maps each use case of [use cases](../product/use-cases.md) to the calls it needs today,
then derives the smallest client surface that serves them all. It is the input the maintainer
asked for on 2026-11-03 for the v0.3.2 API rewrite, ADR-0039: "when designing client interaction
api use Occams Razor principle, if something can be done simple it should be simple". The page
decides nothing. Its candidates are inputs to ADR-0039 and to the proposed records named below,
each of which decides for itself.

This page is the declaration site of the `UC-` ids. Each call it names is declared on its own
page and linked from here: [MCP plugin](../api/mcp-plugin.md), [HTTP API](../api/http-api.md),
[CLI reference](../cli/reference.md), [client SDKs](sdks.md),
[reviewer crews](../api/reviewer-crews.md).

## Sources and their state

Read on 2026-11-03 against `feat/v0.3.1` at `beba0fc`:

- **Built:** [mcp-tools.json](../api/mcp-tools.json), [openapi.yaml](../api/openapi.yaml), the
  pages above, the plugins' READMEs and skills, `crates/bins/oml-cli/src/main.rs` for the CLI's
  built flags, and [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md),
  [ADR-0026](../adr/ADR-0026-names-are-keys.md), [ADR-0028](../adr/ADR-0028-agent-key-issuance.md),
  [ADR-0029](../adr/ADR-0029-session-start-hook.md), [ADR-0033](../adr/ADR-0033-interim-helm-chart.md)
  and [ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md).
- **Proposed, on open draft PRs:** ADR-0027 (`feat/ontology-crews`, #10), ADR-0030
  (`feat/in-progress-workareas`, #12), ADR-0031 (`feat/oauth-connect`, #14), ADR-0035
  (`feat/deliverables`, #17), ADR-0036 (`feat/temporality`, #19), ADR-0037
  (`feat/session-claims`, #20) and ADR-0038 (`feat/turn-context`, #24, built on that branch).
  They are not on this branch, so they are named, not linked.
- **Drafting, not committed:** ADR-0039 (the API rewrite) and ADR-0040 (plugin setup in Claude
  Code) exist as uncommitted drafts in the `feat/api-rewrite` worktree. This page reads the
  2026-11-03 draft and cites its decisions by number; where the committed record renumbers them,
  [ADR-0039's verdict](#adr-0039-what-the-map-supports-and-contradicts) follows.

Statuses: **built** (released in v0.3.0) · **built, unreleased** (on `feat/v0.3.1`) · **built on
a branch** (not in any release candidate) · **designed** (decided, not built) · **decided, not
designed** (the maintainer decided to do it; the shape is open). The client page shows these as
available now, in the next release, designed and planned.

## The razor, as three tests

A client call is one MCP tool call, one HTTP request, one CLI verb or one SDK method. The
surface is minimal when no call, argument or concept fails one of these:

1. **Cut.** A call, parameter or concept that no use case on this page needs is a cut candidate.
   A designed one is cut from the design, not only from the contract.
2. **Merge.** Two calls, or two parameters, that do one job are a merge candidate. One job means
   one use case served the same way and one result shape.
3. **Default.** A required argument that the server can derive from the key, the request or the
   stored state is a default candidate. The common case then needs no argument.

The tests are bounded by decisions already taken. R1, that review and every other gate act are
never reachable over MCP ([ADR-0001](../adr/ADR-0001-the-constitution.md)), is never a merge
candidate. Nor is a separation a recorded decision keeps for a stated reason; each such
exception below names its reason.

## Calls each use case needs today

Each row's "Minimal" column names the call of [the minimal set](#the-minimal-call-set) that
serves it. Arguments are shown only where the minimal form differs from today's.

### Getting started

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-DP-01 | [Try OML on a laptop](../product/use-cases.md#try-oml-on-a-laptop) | built | `docker compose up -d`; `bash scripts/quickstart.sh`; `oml review next`; `oml review verdict --memory <id> --approve` | the same, through `review next` and `review approve` |
| UC-KA-01 | [Claim the first admin key](../product/use-cases.md#claim-the-first-admin-key) | built | `oml bootstrap claim <key-name> --token <enr_…> [--tenant-name] [--principal]`; `POST /v2/bootstrap/claim` | unchanged. The reference designs `--token-stdin`, and the build takes `--token` in argv ([sources](#sources-and-their-state)); the reference and the build should agree |
| UC-KA-02 | [Give an agent its key](../product/use-cases.md#give-an-agent-its-key-without-showing-it) | built | `oml keys issue-agent <key-name> --principal --workarea [--crew] [--held-by] [--write-only] [--out]`; skill `oml-admin:issue-agent-key`; `POST /v2/keys` | `keys issue <key-name> --principal --workarea --out` (merge M9) |
| UC-SS-05 | [Enter the server and key once](../product/use-cases.md#enter-the-server-and-key-once-kept-in-the-keychain) | decided, not designed (ADR-0040 drafting) | `export OML_URL OML_API_KEY`; `claude plugin marketplace add`; `claude plugin install` | plugin `userConfig`, no call |
| UC-DP-06 | [Install the CLI](../product/use-cases.md#install-the-command-line-tool-from-a-verified-download) | built, unreleased | `cosign verify-blob` on `oml-cli-X.Y.Z-<target>` | unchanged |
| UC-SDK-03 | [Call OML from any language](../product/use-cases.md#call-oml-from-any-language) | built | the routed subset of openapi.yaml plus `/mcp` | the served document only (ADR-0039 d9) |
| UC-SDK-04 | [One obvious call per task](../product/use-cases.md#one-obvious-call-for-each-task) | decided, not designed (ADR-0039 drafting) | today's surface | this page's minimal set |

### Everyday memory

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-LC-01 | [Save something](../product/use-cases.md#save-something-worth-keeping) | built | `oml_remember {claim_key, text, idempotency_key, kind?, workarea?}`; `POST /v1/memories {verb, claim_key, kind, body}`; `oml submit --claim` | `write {text}` |
| UC-LC-02 | [Record a conclusion](../product/use-cases.md#record-a-conclusion-and-what-it-came-from) | built | `oml_learn {claim_key, text, derives_from, idempotency_key}`; `POST /v1/memories {verb: learn}`; `oml submit --verb learn --derives-from` | `write {text, derives_from}` |
| UC-LC-03 | [Search before acting](../product/use-cases.md#search-before-acting) | built; keyword search only, semantic search waits on RS-1 | `oml_recall {query, scope?, k?, max_tokens?}`; `POST /v1/memories/recall`; `oml recall --query` | `read {query}` |
| UC-LC-04 | [See its own work](../product/use-cases.md#see-its-own-work-while-it-waits) | built | `oml_my_work {…}`; `oml_recall {include_own_held: true}`; `GET /v1/memories?own=true` | `read {}`; `read {query, include_held: true}` (merge M3) |
| UC-LC-05 | [Read one memory back](../product/use-cases.md#read-one-memory-back) | built | `oml_get {artifact_id}`; `GET /v1/memories/{artifact_id}` | `read {artifact_id}` |
| UC-LC-06 | [Correct a memory](../product/use-cases.md#correct-a-memory) | built | `oml_supersede {artifact_id, text, idempotency_key}`; `POST /v1/memories/{id}/supersede`; `oml supersede --target --claim` | `write {text, replaces}` |
| UC-LC-07 | [Propose retiring](../product/use-cases.md#propose-retiring-a-memory) | built; no CLI flag | `oml_supersede {artifact_id, retirement: true, reason}` | `write {text, replaces, retire: true}`, `text` stating why (cut C5) |
| UC-LC-08 | [Learn when a write was promoted or rejected](../product/use-cases.md#learn-when-a-memory-was-approved-or-rejected) | built | `oml_events_tail {after_seq?, scope?}`; `oml_my_work {state}`; `GET /v2/events` | `read {state?}` (cut C1) |
| UC-LC-09 | [Check where it works](../product/use-cases.md#check-where-it-works-and-what-it-may-do) | built | `oml_workarea_info {workarea?}`; `GET /v2/workareas/{w}` | `workarea {}` |
| UC-LC-10 | [Link two memories](../product/use-cases.md#link-two-memories) | designed; `oml_relate_propose` and `oml_graph` answer "not built yet" | designed: `oml_relate_propose {kind, label, from, to}`, `oml_graph`, `/v2/claim-edges`, `GET /v2/memories/{id}/lineage` | `relate {from, to, label}` (designed); browsing is `read {artifact_id, lineage}` (merge M4) |
| UC-LC-11 | [Save many at once](../product/use-cases.md#save-many-memories-at-once) | designed | reserved `POST /v1/memories:batch` | `write` taking a list, on HTTP and the SDKs only |
| UC-SDK-01 | [Store and search from an application](../product/use-cases.md#store-and-search-from-an-application) | built; not on PyPI or npm | `remember`, `learn`, `recall`, `get`, `list_memories`/`iter_memories`, `supersede` | `write`, `read`, `workarea` |
| UC-FL-01 | [When a request is refused](../product/use-cases.md#when-a-request-is-refused) | built | the typed error envelope on every surface | unchanged; one envelope on MCP too (ADR-0039 d11) |
| UC-FL-02 | [Retry without a duplicate](../product/use-cases.md#retry-without-creating-a-duplicate) | built | `Idempotency-Key` header; `idempotency_key` on MCP; SDK-generated | unchanged (see [D2](#default-candidates)) |
| UC-FL-04 | [Reuse a label that is taken](../product/use-cases.md#reuse-a-label-that-is-taken) | built | `oml_remember` → `submission.claim_key_conflict`; `oml_my_work {claim_key}` | arises only when the caller sends `claim_key`, or writes the same text twice (D1) |

### Review

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-RV-01 | [Take the next held item and record a verdict](../product/use-cases.md#approve-or-reject-the-next-memory) | built | `oml review next`; `oml review verdict --memory <id> --approve\|--reject --reason`; `POST /v2/review/next`, `POST /v2/review/verdicts {serve_id, verdict}`; SDK `review_next`, `verdict`; no MCP (R1) | `review next {}`; `review approve {}`, `review reject {reason}` (default D5) |
| UC-SDK-02 | Reviewer bot | built | SDK `review_next()`, `verdict(…)` | the same two calls |
| UC-RV-02 | [See how much waits](../product/use-cases.md#see-how-much-waits) | designed | designed: `oml review queue`, `GET /v2/review/queue` | `review queue {}` |
| UC-RV-03 | [Reopen a wrong rejection](../product/use-cases.md#reopen-a-wrong-rejection) | designed | designed: `oml review reopen <artifact>@<version>`, `POST /v2/review/reopen {artifact_id, version, reason}` | `review reopen {artifact_id, reason}`; `version` derived (D6) |
| UC-RV-04 | [Review with its files](../product/use-cases.md#review-a-memory-with-its-files) | designed | designed: `GET /v1/attachments/{digest}?artifact_id=`; `oml review next` fetches and checks | `review next` fetches; no client call |
| UC-RV-05 | [k > 1, or a human verdict](../product/use-cases.md#require-more-than-one-approval-or-a-persons) | designed | designed: tenant policy through `oml settings apply` | `settings apply` |
| UC-RV-06 | [A self-signed promotion, alone](../product/use-cases.md#approve-your-own-work-when-you-work-alone) | designed | designed: a verdict on one's own row in a development workarea | `review approve`; no new call |
| UC-RV-07 | [A rubric beside the item](../product/use-cases.md#a-rubric-beside-the-item) | decided, not designed (FR-59) | none | a field of the `review next` answer, not a call |

### Teams and access

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-WA-01 | [Create a workarea](../product/use-cases.md#organize-work-into-workareas) | built | `oml workareas create <parent> <name> [--crew]`; `POST /v2/workareas {parent_id, name}` | unchanged; `parent` stays required (kept K3) |
| UC-WA-02 | See the tree and one workarea | built | `oml workareas tree [--root]`, `oml workareas show`; `GET /v2/workareas/{w}/tree`, `GET /v2/workareas/{w}` | unchanged |
| UC-WA-03 | Record crews | built | `oml crews create\|update\|list\|show`; `oml workareas crew set`; `/v2/crews…`, `POST /v2/workareas/{w}/crew` | unchanged |
| UC-WA-04 | Address by name | built | every reference argument takes an id or a name; `X-OML-Workarea` | one spelling per concept (ADR-0039 d7) and one place for the workarea on HTTP (merge M7) |
| UC-KA-03 | [Issue a key with a role](../product/use-cases.md#issue-a-key-with-a-chosen-role) | built | `oml keys issue <name> --role --grant … [--default-workarea] [--crew] [--write-only] [--label]`; `POST /v2/keys {role, name, …}` | `keys issue` (merge M9) |
| UC-KA-04 | [Look up a key](../product/use-cases.md#look-up-a-key) | built | `oml keys show`; `GET /v2/keys/{key}` | `keys show`, carrying effective access (merge M6) |
| UC-KA-09 | List keys | designed | designed: `oml keys list [--role] [--expiring]`; `GET /v2/keys` | unchanged |
| UC-KA-05 | [Cut a key off](../product/use-cases.md#cut-a-key-off) | designed; workaround `oml deny set <key> universe` | designed: `oml keys revoke`, `POST /v2/keys/{key}/revoke` | unchanged; revoke and deny stay separate (kept K4) |
| UC-KA-06 | [Keep a key out of a subtree](../product/use-cases.md#keep-a-key-out-of-part-of-the-tree) | built; lift designed | `oml deny set <key> <subtree> [--reason]`; `POST /v2/keys/{key}/denials`; designed lift | unchanged; `--reason` is cut C7 |
| UC-KA-07 | [Replace a key](../product/use-cases.md#replace-a-key) | designed | designed: `oml keys rotate <key> <new-name> [--grace]` | unchanged |
| UC-KA-08 | [Change a key's access](../product/use-cases.md#change-a-keys-access) | designed | designed: `oml grants add\|remove\|list\|effective`, three routes | `grants add\|remove`; `list` and `effective` fold into `keys show` (merge M6) |
| UC-KA-10 | [Stop an author or source](../product/use-cases.md#stop-an-author-or-source) | designed | designed: `oml bans add\|lift\|list`, `/v2/bans` | unchanged, under one preview convention (merge M10) |
| UC-KA-11 | [Host another organization](../product/use-cases.md#host-another-organization) | designed; `oml tenants create` refuses locally | designed: `oml tenants create`, `oml tenants issue-admin`, `POST /v2/tenants` | unchanged |
| UC-KA-12 | [Switch servers and keys](../product/use-cases.md#switch-between-servers-and-keys) | designed | today `--server`/`--key`, `OML_CLI_SERVER`/`OML_CLI_KEY`, the credentials file; designed `oml context add\|use\|show\|remove` | unchanged |
| UC-WA-05 | [A workarea grows up](../product/use-cases.md#a-workarea-grows-up) | designed (ADR-0009) | designed: `oml workareas status propose\|ratify\|pending`; three routes | `status propose\|ratify`; `pending` reads from `workareas show` (ADR-0039 d9) |
| UC-WA-06 | Relate two workareas | designed | designed: `oml workareas relate propose\|ratify`; `oml_relate_propose {kind: workarea}`; workarea-edge routes | one edge family (ADR-0039 d9) |
| UC-WA-07 | Move a workarea | designed | designed: `oml workareas re-parent <w> --new-parent [--apply]` | unchanged |
| UC-WA-08 | Move a memory, or serve an earlier version | designed | designed: `oml claims reassign`, `oml claims tip-override`; two routes | unchanged |
| UC-IP-01 | [Open an in-progress workarea](../product/use-cases.md#work-together-before-review) | designed (ADR-0030) | designed: `oml workareas create --in-progress`; `oml workareas in-progress <w>`; two routes | `workareas in-progress <w>` (merge M8) |
| UC-IP-02 | Teammates' held memories inside | designed (ADR-0030 d5, d8) | designed: `oml_recall {include_unreviewed}`, default `true` inside | `read {query}`; the default decides (merge M3) |
| UC-IP-03 | Keep a key from other keys' held memories | designed (ADR-0030 d6) | designed: reader grant `unreviewed: block` at issuance | `keys issue … --block-unreviewed` |
| UC-IP-04 | Release a finished workarea | designed (ADR-0030 d12) | designed: `status propose --to committed`, ratify by another identity | `status propose\|ratify` |
| UC-MB-03 | See and cut connections | designed (ADR-0031 d12, d14) | designed: `oml doctor` fields; revocation closes a connection | `doctor`, `keys revoke` |
| UC-ON-01 | Look up a term | designed (ADR-0021, ADR-0027) | designed: `oml_vocab {action: suggest\|get\|hierarchy\|mappings}`; four routes | `read` over the vocabulary kinds (merge M5) |
| UC-ON-02 | Propose terms | designed | designed: `oml_remember {kind: term \| term_mapping \| vocab_release}`; supersede on HTTP | `write {text, kind}` |
| UC-ON-03 | Tag memories | designed (ADR-0027 d6) | designed: `oml_relate_propose {label: oml:about}` | `relate` |
| UC-ON-04 | Curate | designed | designed: review routes; claim-edge ratify and reject | `review next\|approve\|reject` and edge ratify |
| UC-ON-05 | Set up an ontology crew | designed (ADR-0027 d14) | partly `oml crews create`, `oml keys issue-agent`; designed `--kind ontology`, `oml crews add-member` | unchanged; `add-member` composes `keys issue` |
| UC-ON-06 | Search by term | designed | designed: recall `terms`, `expand_terms` | `read {query, terms}` |
| UC-ON-07 | Require valid terms | designed | designed: workarea policy through settings | `settings apply` |

### Sessions and agents

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-SS-01 | [Start every session from memory](../product/use-cases.md#start-every-session-from-memory) | built; a recorded Claude Code run is owed (FR-33) | hook `oml hook session-start` → `GET /v1/memories?own=true&limit=10`, `POST /v1/memories/recall {k: 10, include_bodies, include_own_held, max_tokens}` | unchanged: two reads, made by the hook, not the agent |
| UC-SS-02 | [Save what a compaction summarized](../product/use-cases.md#save-what-a-compaction-summarized) | built | hook asks; `oml_remember` | `write {text}` |
| UC-SS-03 | [Be told when to search](../product/use-cases.md#be-told-when-to-search) | built on a branch (ADR-0038 d1–d3) | MCP `initialize` → `instructions`; `oml_recall` | `read {query}`; no new call |
| UC-SS-04 | [Recall fixes on failure](../product/use-cases.md#recall-known-fixes-when-a-command-fails) | built on a branch (ADR-0038 d4–d9) | `oml hook tool-failure` with `OML_FAILURE_RECALL=1`; `oml hook session-end`; recall `{k: 3, max_tokens: 200}` | unchanged; hooks, not agent calls |
| UC-SS-06 | [Allow reads once](../product/use-cases.md#allow-reads-once-keep-asking-on-writes) | built | five `mcp__plugin_oml_oml__…` rules in `permissions.allow` | two rules, `…__oml_read` and `…__oml_workarea` (cut C1) |
| UC-SS-07 | [Hand lookups to a librarian](../product/use-cases.md#hand-lookups-to-a-librarian) | decided, not designed | none | a plugin subagent over `read`; no new call |
| UC-ST-01 | [Standing instructions](../product/use-cases.md#standing-instructions) | built | `oml_remember {kind: steering}`; `POST /v1/memories {kind: steering}`; `oml submit --kind steering`; then review | `write {text, kind: steering}` |
| UC-ST-02 | Receive steering first | built | `oml_recall`; the session-start hook | `read {query}` |
| UC-MB-01 | [Use OML from another tool](../product/use-cases.md#use-oml-from-another-tool) | built | `/mcp` with a bearer key, the eight served tools | the three agent tools |
| UC-MB-02 | [Connect claude.ai with a code](../product/use-cases.md#connect-claudeai-and-the-claude-apps-with-a-one-time-code) | designed (ADR-0031) | designed: `oml keys issue-agent … --connect-code`; OAuth discovery, authorize and token endpoints | `keys issue … --connect-code`; the OAuth endpoints are the client's, not the user's |
| UC-MB-05 | Told when to search, without a plugin | built on a branch (ADR-0038 d1) | MCP `initialize` → `instructions` | no call |
| UC-BR-01 | [Brief on entering an area](../product/use-cases.md#briefs-summaries-and-handoff-notes) | decided, not designed (FR-60; ratified edges) | none; steering is the nearest | a kind read by `read`, or a hook; no tool (ADR-0039 d8) |
| UC-BR-02 | Reviewed area summary | decided, not designed (FR-61) | workaround `oml_learn` with `derives_from` | `write {text, derives_from}` |
| UC-BR-03 | Handoff note | decided, not designed (FR-55) | workaround `oml_remember`, read back by the hook's own-work lines | `write {text}` with a kind; read by the hook |
| UC-BR-04 | Definition of done | decided, not designed (FR-56) | none | a hook |
| UC-BR-05 | Instructions on touch | decided, not designed (FR-58) | none; all steering loads at start | a hook |

### Files and deliverables

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-DL-01 | [Hand in finished work](../product/use-cases.md#hand-in-finished-work) | designed (ADR-0035 d1, d4, d7, d8; the object store S1 on `feat/objectstore`, #23) | designed: `oml_remember {kind: deliverable, attachments, derives_from}`; `oml attach put` | `write {text, kind: deliverable, attachments, derives_from}` |
| UC-DL-02 | [Attach files](../product/use-cases.md#attach-files-to-a-memory) | designed; `oml_attach` answers "not built yet", SDK `upload_attachment` raises `NotBuiltError` | designed: `oml_attach {action: open\|status}`; `POST /v1/attachments/uploads`, `PUT …/parts/{n}`, `POST …/complete` | `oml attach put <file>` in the terminal, then `write {attachments: [{upload_session}]}` (cut C2) |
| UC-MB-04 | Short text files from claude.ai | designed (ADR-0035 d4, d6) | designed: `attachments: [{path, media_type, text}]`; forms on `oml_workarea_info` | `write {attachments: [{path, text}]}`; `media_type` derived from the path (D7) |
| UC-DL-03 | [Read and download](../product/use-cases.md#read-and-download-a-deliverable) | designed (ADR-0035 d3, d5) | designed: `oml_get {include_text}`; `GET /v1/attachments/{digest}?artifact_id=`; `oml attach get` | `read {artifact_id, include_text}`; the download stays HTTP |
| UC-DL-04 | List deliverables | designed (ADR-0035 d7) | partly `GET /v1/memories?kind=`; designed `oml_my_work {kind}`, `oml_recall {kinds}` | `read {kind: deliverable, mine: false}` |
| UC-DL-05 | Trace a result | designed (ADR-0035 d8) | designed: `GET /v2/memories/{id}/lineage?direction&depth`; `oml_graph` | `read {artifact_id, lineage: ancestors\|descendants}` (merge M4) |
| UC-DL-06 | [Choose where files are kept](../product/use-cases.md#choose-where-files-are-kept) | designed (ADR-0035 d9, d10) | designed: boot setting `storage.url`; `oml storage copy` | unchanged; configuration, not a client call |

### Time

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-TT-01 | [Say when a memory is true](../product/use-cases.md#time) | designed (ADR-0036 d2) | designed: `valid_from`, `valid_to` on the three writes and `POST /v1/memories` | `write {text, valid_from?, valid_to?}` |
| UC-TT-02 | What OML held at a date | designed (ADR-0036 d3) | designed: `as_of` or `as_of_seq` on recall, get and both listings | `read {as_of}`, taking an instant or a seq (merge M2) |
| UC-TT-03 | True at a time or in a range | designed (ADR-0036 d4) | designed: `valid_at`, `valid_since`, `valid_until` | unchanged |
| UC-TT-04 | Read-only MCP resources | designed (ADR-0036 d5) | designed: `resources/list`, `resources/templates/list`, `resources/read` | unchanged; a second read path, kept K5 |
| UC-TT-05 | `oml fs` from a terminal | designed (ADR-0036 d6) | designed: `oml fs ls\|cat\|export` | unchanged; each verb is one `read` |

### Guardrails

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-GR-01 | [Set a team rule](../product/use-cases.md#team-rules) | designed (ADR-0037 d1–d3, d10; domain core on `feat/guardrails-domain`, #21) | designed: `POST /v1/memories {kind: guardrail, severity, rule?}`, not on MCP; then review | `write` over HTTP with `kind: guardrail` |
| UC-GR-02 | Load rules at start | designed (ADR-0037 d5–d7) | designed: `oml hook guardrails` → `GET /v1/guardrails` | unchanged (kept K6) |
| UC-GR-03 | Deny before the tool runs | designed (ADR-0037 d8, d9, d13) | designed: `oml hook pre-tool-use`, from the local cache | unchanged; no network call |
| UC-GR-04 | Rule scope per key | designed (ADR-0037 d4) | designed: `--guardrail-scope workarea\|ancestors` at issuance and rotation | `keys issue … --guardrail-scope`, default `ancestors` |
| UC-GR-05 | Rules for harnesses without hooks | designed (ADR-0037 d11) | designed: `oml guardrails export --format agents-md\|claude-md`; MCP prompt `guardrails` | unchanged |
| UC-GR-06 | A change freeze | designed (ADR-0037 d12, needs ADR-0036) | designed: a guardrail with `valid_from`, `valid_to` | `write {kind: guardrail, valid_from, valid_to}` |

### Operations

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-FL-03 | Start when the server is down | built (ADR-0029 d5) | the hook exits 0 and prints nothing | unchanged |
| UC-FL-05 | [Check health](../product/use-cases.md#check-health) | built in part | `oml doctor`; `GET /healthz`, `GET /readyz`; designed `oml status` | `doctor` (merge M11) |
| UC-FL-06 | Diagnose a named condition | designed | partly `/readyz` and `oml doctor`; designed runbook verbs, a dead-letter re-enqueue with no verb (FR-28) | `doctor` names the runbook |
| UC-DP-02 | [Install on one host](../product/use-cases.md#install-or-upgrade-on-one-host) | built, unreleased | `cosign verify-blob` on `oml-server-lead-X.Y.Z-<target>.tar.gz` | unchanged |
| UC-DP-03 | [Install on Kubernetes](../product/use-cases.md#install-on-kubernetes) | built (ADR-0033) | `kubectl create secret`; `helm install`; `kubectl logs`; `kubectl port-forward`; `oml bootstrap claim`; `helm upgrade --set ingress.enabled=true` | unchanged |
| UC-DP-04 | [Upgrade without losing identity](../product/use-cases.md#upgrade-without-losing-identity) | built (ADR-0033 d2, d3) | `helm upgrade`; `--set keyring.existingClaim` | unchanged |
| UC-DP-05 | [Least privilege](../product/use-cases.md#run-the-server-with-least-privilege) | built, unreleased (ADR-0034) | owner role, one `ALTER … OWNER`, `OML_POSTGRES__OWNER_URL` beside `OML_POSTGRES__URL` | unchanged |
| UC-OP-01 | [Apply settings](../product/use-cases.md#apply-settings-and-see-what-is-in-effect) | designed | designed: `oml settings show [--effective]\|validate\|apply`; three routes | `settings show\|apply [--dry-run]` (merge M10) |
| UC-SIM-01 | [Rehearse with invented facts](../product/use-cases.md#rehearse-with-invented-facts) | designed (ADR-0011) | designed: `oml simulations open\|list\|show\|end`; `simulation_id` on writes, refused | unchanged at build; the write fields leave until then (cut C3) |
| UC-DS-01 | [Import a data set](../product/use-cases.md#import-an-existing-data-set) | designed (ADR-0020) | none | unchanged at build |
| UC-DP-07 | [Run in production](../product/use-cases.md#run-in-production) | designed (ADR-0016, ADR-0023) | none | unchanged |
| UC-DP-08 | Back up and restore | designed | the operator's own database backups | unchanged |
| UC-DP-09 | Rotate keys, respond to compromise | designed | none | unchanged |
| UC-DP-10 | Move between profiles | designed (ADR-0023 d10) | none | unchanged |
| UC-DP-11 | Monitor | designed; partly `/healthz`, `/readyz`, plain-text boot lines | designed: metrics, JSON logs, SLOs | unchanged |

### Audit

| UC | Use case | Status | Calls today | Minimal |
|---|---|---|---|---|
| UC-AU-01 | [Follow the record](../product/use-cases.md#follow-the-record) | built | `GET /v2/events?after_seq=&limit=`; `oml_events_tail` | `events {}` on HTTP and the SDKs (cut C1) |
| UC-AU-02 | [Check history](../product/use-cases.md#check-that-history-was-not-changed) | designed; `oml audit verify-chain` built, refused live | designed: `POST /v2/exports {kind: chain}` | `audit verify-chain` |
| UC-AU-03 | [One bundle of evidence](../product/use-cases.md#one-bundle-of-evidence) | designed | designed: `oml audit export --out`; three export routes | `audit export` |
| UC-AU-06 | Who read what | designed | designed: the read log in the evidence export | `audit export`; no call of its own |
| UC-AU-04 | [Erase](../product/use-cases.md#withdraw-or-erase-a-memory) | designed (ADR-0005) | designed: `oml erase <artifact> [--dry-run]`; `POST /v2/memories/{id}/erase` | unchanged, under one preview convention (merge M10) |
| UC-AU-05 | Withdraw (demote) | designed | designed: `oml claims demote <artifact>@<v> --reason`; `POST /v2/memories/{id}/demote {version, reason}` | `claims demote <artifact> --reason`; `version` derived (D6) |
| UC-AU-07 | [Retention and legal hold](../product/use-cases.md#retention-and-legal-hold) | decided, not designed (FR-26) | none | settings and an admin verb, shape open |
| UC-AU-08 | [Offboard an organization](../product/use-cases.md#offboard-an-organization) | designed (FR-27) | designed: a ceremony of revocations, exports and erasures | the composed verbs |
| UC-AU-09 | [Analytics lake](../product/use-cases.md#analyze-memory-in-a-data-lake) | designed (ADR-0003) | none | not a client call of the gate |

## The minimal call set

These calls, with these arguments, serve every use case above. Names follow ADR-0039's draft
where the map agrees with it, so the map judges shape, not names. Each surface carries the same
call under its own spelling: MCP `oml_<call>`, SDK `<call>`, CLI `oml <call>`.

### The agent: three tools

| Call | Required | Optional, and the default that covers the common case | Rides | Serves |
|---|---|---|---|---|
| `write` | `text`; `idempotency_key` on MCP, generated by the SDKs and the CLI | `workarea` (the key's default); `claim_key` (derived, D1); `kind` (`memory`, D3); `derives_from` (none; non-empty journals `learn`); `replaces` (none; present, a supersession); `retire` (`false`, needs `replaces`). Designed: `attachments`, `valid_from`, `valid_to` | `POST /v1/memories`, or `…/{id}/supersede` with `replaces` | LC-01, LC-02, LC-06, LC-07, ST-01, SS-02, BR-02, BR-03, DL-01, MB-04, ON-02, TT-01, GR-01, GR-06 |
| `read` | none | `artifact_id` (get); `query` (recall); neither, the listing with `mine` (`true`). `workarea` (the key's default); `k`, `max_tokens`; `include_held` (merge M3); `kind`, `state`, `claim_key`, `limit`, `cursor`. Designed: `lineage` and `depth` (M4), `include_text`, `as_of` (M2), `valid_at`, `valid_since`, `valid_until`, `terms`, `expand_terms` | `POST /v1/memories/recall`, `GET /v1/memories/{id}`, `GET /v1/memories`, and the designed lineage read | LC-03, LC-04, LC-05, LC-08, LC-10, ST-02, SS-03, IP-02, DL-03, DL-04, DL-05, ON-01, ON-06, TT-02, TT-03 |
| `workarea` | none | `workarea` (the key's default) | `GET /v2/workareas/{w}` | LC-09, MB-04 (the attachment forms), IP-01 (the in-progress root) |

A fourth agent call, `relate {from, to, label}`, is designed for LC-10, ON-03 and WA-06 and
waits on claim-edge support; it carries `kind` only if workarea edges are proposed by agents,
which no use case above needs (the workarea edge is an admin's).

### The reviewer: never MCP

| Call | Required | Optional | Serves |
|---|---|---|---|
| `review next` | none | `workarea` | RV-01, RV-04, SDK-02, ON-04 |
| `review approve` | none | `serve_id` (D5), `note` | RV-01, RV-06, SDK-02 |
| `review reject` | `reason` | `serve_id` (D5) | RV-01, SDK-02 |
| `review queue` | none | `workarea` | RV-02 (designed) |
| `review reopen` | `artifact_id`, `reason` | `version` (D6) | RV-03 (designed) |

### The auditor

| Call | Required | Optional | Serves |
|---|---|---|---|
| `events` (HTTP, SDK) | none | `after_seq` (0), `workarea`, `limit` | AU-01 |
| `audit export`, `audit verify-chain` | as the [CLI reference](../cli/reference.md#oml-audit-evidence-export-and-chain-verification) declares | | AU-02, AU-03, AU-06 |

### The admin and the operator

The admin surface is the CLI over `/v2`, and its verbs are the ones the tables above name in
their Minimal column. Against today's built and designed verbs it changes in four places: `keys
issue` absorbs `keys issue-agent` (M9), `keys show` absorbs `grants list` and `grants effective`
(M6), `doctor` absorbs `status` (M11), and every previewing verb takes one convention (M10).

### The hooks

`oml hook session-start`, and the designed `tool-failure`, `session-end`, `guardrails` and
`pre-tool-use`, are calls the harness makes, not the agent. Each reads with the agent's key
through the same `read` routes and adds no route of its own, except `GET /v1/guardrails` (K6).

## Cut candidates

| # | What | Why no use case needs it | Bearing on the records |
|---|---|---|---|
| C1 | `oml_events_tail` on the MCP catalog (ADR-0039's `oml_events`) | Its one agent use case, LC-08, is served by `read {state}` with no cursor to keep; the cursor-polled tail serves AU-01, an auditor's use case, on HTTP. It also costs one more `permissions.allow` entry (SS-06) | contradicts ADR-0039 d2 in part: keep `GET /v2/events`, drop the tool, or keep it with a use case that the listing cannot serve |
| C2 | `oml_attach` as an MCP tool | MCP carries no file bytes, so an agent that opens an upload session still needs a terminal or HTTP to send the parts. `oml attach put <file>` does the whole upload and prints the session the write names | for ADR-0035: no file tool returns when attachments are built; supports ADR-0039 d8 |
| C3 | `simulation_id`, `simulated_time` on the served writes | No simulation can be opened, and the server refuses every `simulation_id` | supports ADR-0039 d9 |
| C4 | kind `derived_fact` in the write's input | No use case writes a conclusion with no nameable source; a conclusion with sources is `derives_from`, and one without is a memory. Stored rows keep the kind | open for ADR-0039; not in its draft |
| C5 | `reason` beside `text` on a retirement | A retirement's `text` already states why (ADR-0039 d3); a second field for the same sentence is a choice the agent must make | narrows ADR-0039 d3, which keeps `reason` optional on every supersession |
| C6 | `GET /v2/workareas?path=` (`resolveWorkarea`) | Names are keys and no reference field takes a path ([ADR-0026](../adr/ADR-0026-names-are-keys.md)); no use case starts from a path | extends ADR-0039 d9: drop it from the designed annex |
| C7 | `oml deny set --reason` | The server accepts the reason and journals none (`main.rs`, `DenyCommand::Set`), so nothing reads it | journal it, which UC-AU-03 would use, or cut it |
| C8 | `--write-only` on `keys issue-agent` | No use case names an agent that writes and may not read its own workarea ([ADR-0028](../adr/ADR-0028-agent-key-issuance.md) d2 gives the flag) | open: name the use case, or cut |
| C9 | `label` on a key | A key has a name, a principal and a label ([roles and keys](../architecture/roles-and-keys.md#key-names-principals-and-labels)); no use case reads the label | weak: a free-text note costs nothing at the common case, since it is optional |
| C10 | `since`, `until` on `GET /v1/memories` | No use case names a submission-time window on the listing; the time use cases are ADR-0036's as-of and valid-time filters | for ADR-0036: one set of time filters |

## Merge candidates

| # | Two that do one job | The one | Bearing on the records |
|---|---|---|---|
| M1 | `oml_remember`, `oml_learn`, `oml_supersede`; `oml_recall`, `oml_get`, `oml_my_work` | `write`, `read` | supports ADR-0039 d2–d4 |
| M2 | `as_of` and `as_of_seq` | one `as_of` taking an RFC 3339 instant or an integer seq | for ADR-0036 d3, which refuses both together already |
| M3 | `include_own_held` (ADR-0025) and `include_unreviewed` (ADR-0030 d8) | one `include_held`, whose reach the server derives: the key's own held rows everywhere, other keys' held rows only inside an in-progress subtree; default `true` inside one, `false` outside | for ADR-0030 and ADR-0039; ADR-0030 d8 says the two fields already never serve a row twice |
| M4 | `oml_graph`, `GET /v2/memories/{id}/lineage` and `read {artifact_id}` | `read {artifact_id, lineage: ancestors\|descendants, depth}`, returning claim items | supports ADR-0039 d4's one-shape test: lineage returns claim items, so it passes it. ADR-0039 d2 leaves the fold to the record that builds lineage; the map recommends it |
| M5 | `oml_vocab {suggest\|get}` and `read` | `read {query, kind: term}`, `read {artifact_id}`; `hierarchy` and `mappings` become `read` over `term_mapping` claims and the lineage mode | for ADR-0027 d13 and ADR-0039 d2; `suggest` may need a prefix match that recall's keyword search does not do, which is the case for keeping it |
| M6 | `GET /v2/keys/{key}`, `GET /v2/keys/{key}/effective-access` and `oml grants list` | `keys show`, carrying grants, denials and effective access | new; not in ADR-0039 |
| M7 | `X-OML-Workarea` on submit, recall and supersede, and the `workarea` field on the listing and the events tail | one place for the workarea on HTTP | contradicts ADR-0039 d7, which keeps the header unchanged and so keeps two forms of one argument on HTTP |
| M8 | `oml workareas create --in-progress` and `oml workareas in-progress <w>` | the one verb, after a create | weak, for ADR-0030 d11: the flag saves one call at creation |
| M9 | `oml keys issue-agent` and `oml keys issue` | one `keys issue` whose defaults are the agent key: role `writer`, a writer and a reader grant on `--workarea`, which is also the default | new; `issue-agent` stays a name the `oml-admin` skill can call |
| M10 | previews by `--apply` (`bans add`, `re-parent`, `keys revoke --matching`) and by `--dry-run` (`erase`, `settings validate`, `reassign`, simulation end) | one convention across every previewing verb, and `settings validate` as `settings apply --dry-run` | new |
| M11 | `oml status` and `oml doctor`, which emit the same signals | `doctor`, carrying the snapshot fields | new |
| M12 | claim edges and workarea edges, three routes each; the workarea status read and the workarea read | one edge family; the workarea read | supports ADR-0039 d9 |

## Default candidates

| # | Required today | Derived from | Bearing on the records |
|---|---|---|---|
| D1 | `claim_key` on every MCP write and on `POST /v1/memories` | the text, as `auto-` plus a digest prefix | supports ADR-0039 d6 |
| D2 | `idempotency_key` on MCP writes | could be the arguments' digest | the razor's case for a default; the maintainer kept it required on 2026-11-03, because two deliberate identical writes would collapse into one (ADR-0039 d6). The map records the cost: the agent invents one string per write |
| D3 | `kind` on `POST /v1/memories` | `memory`, MCP's default already | new: ADR-0039 d10 makes `verb` and `claim_key` optional on the route and leaves `kind` required |
| D4 | `verb` on `POST /v1/memories` | `derives_from`: empty `remember`, non-empty `learn` | supports ADR-0039 d3 |
| D5 | `serve_id` on a verdict | the reviewer key's one open serve, when it holds exactly one | contradicts ADR-0039 d10's `review approve <serve_id>`, which makes the reviewer copy what `review next` printed; with more than one open serve, the field stays required |
| D6 | `version` on reopen and on demote | the version the artifact id already names | for the designed review and claim-administration routes |
| D7 | `media_type` on an inline text attachment | the path's extension, `text/plain` otherwise | for ADR-0035 d4 |
| D8 | the workarea on every agent call | the key's default workarea | already the rule (ADR-0025 d14) |

### Kept, with the reason

| # | Looks like a candidate | Kept because |
|---|---|---|
| K1 | the review verbs on MCP, beside `write` and `read` | R1: no gate act over MCP ([ADR-0001](../adr/ADR-0001-the-constitution.md)) |
| K2 | `--principal` on key issuance, derived from the key's name | the name never stands in for the principal ([ADR-0026](../adr/ADR-0026-names-are-keys.md) d9) |
| K3 | `parent` on workarea create, defaulting to `universe` | a wrong parent costs a re-parent ceremony; the common case is not one parent |
| K4 | `keys revoke` replaced by `deny set <key> universe` | a revoked key is dead to every check, and a denial leaves it live within the tree |
| K5 | MCP resources beside `read` | clients show resources to the user without a tool call, which no tool does (ADR-0036 d5) |
| K6 | `GET /v1/guardrails` beside the listing | the hook needs the set's digest and its cap in one answer, and recall never serves kind `guardrail` (ADR-0037 d5) |
| K7 | `OML_URL`/`OML_API_KEY` beside `OML_CLI_SERVER`/`OML_CLI_KEY` | an admin key must never feed an agent's context ([ADR-0029](../adr/ADR-0029-session-start-hook.md) d4) |
| K8 | `workarea` and `events` as tools of their own, not modes of `read` | neither returns claim items (ADR-0039 d2); the map cuts `events` from MCP for another reason (C1) |

## ADR-0039: what the map supports and contradicts

| ADR-0039 (draft) | The map |
|---|---|
| d1 — the contract may be rewritten before FR-21, with a mapping, a break list and a migration | supports; this page's Minimal column is a mapping of the same kind, by use case |
| d2 — four served tools | supports `oml_write`, `oml_read` and `oml_workarea`; contradicts `oml_events` on MCP (C1); supports moving `oml_attach`, `oml_relate_propose`, `oml_graph` and `oml_vocab` out, and adds that `oml_attach` should not return (C2), `oml_graph` should fold into `oml_read` (M4) and `oml_vocab` mostly can (M5) |
| d3 — one flat write; the endpoint and verb follow from the fields | supports (D4); narrows `reason` on a retirement (C5); adds the kind cut C4 |
| d4 — read modes by `artifact_id`, `query` or neither | supports; adds the lineage mode (M4) and the one `include_held` flag (M3) |
| d5 — annotations | supports; with C1, one `permissions.allow` entry fewer for SS-06 |
| d6 — derived `claim_key`, required `idempotency_key` | supports the first (D1); records the second as the one place the maintainer's decision outweighs the razor (D2) |
| d7 — one spelling per concept | supports; contradicts keeping `X-OML-Workarea` beside a `workarea` field (M7) |
| d8 — a capability is a parameter, a resource or a hook before a tool | supports; every designed use case on this page fits the three agent tools, `relate` aside, or a hook |
| d9 — served-only contract, designed annex, duplicates removed | supports (M12, C3); adds C6 and C10 to what leaves the annex |
| d10 — the old-to-new mapping | supports; adds `kind`'s default on HTTP (D3) and contradicts the required `serve_id` (D5) |
| d11 — one error shape | supports (UC-FL-01) |
| d12, d13 — aliases for one minor release; the break list | no use case bears on them; C1 adds one break, `oml_events_tail` without a successor tool, whose successor is `read {state}` or `GET /v2/events` |
| d14 — the build plan | no bearing |
| ADR-0040 (draft) — plugin setup through `userConfig` | supports UC-SS-05 |

The map adds four candidates ADR-0039's draft does not name: M6, M9, M10 and M11, all on the
admin CLI. They change no agent call.

## What this page does not decide

It opens and closes no roadmap row and changes no contract. Each candidate goes to the record
named in its row; a candidate that no record takes stays on this page as a candidate. When a
use case is added to [use cases](../product/use-cases.md), it gets a `UC-` id and a row here in
the same change.
