<!--
id: api/mcp-plugin
tier: engineering
status: design draft for review · decided, not built
answers: which MCP tools the agent plugin exposes over streamable HTTP and how each translates to one call on the two-ring HTTP API; the one claim-item shape; how a tool's workarea argument takes an id or a name; how an agent authenticates, by key or by a connection, and how Claude's hosted apps are wired in
-->

# The agent plugin: MCP over streamable HTTP

**Status:** design draft for review · decided, not built

**Release:** the three task-shaped tools since v0.4.0; the eight they replace, four since v0.1.0 and four since v0.2.0, answered unlisted through v0.5.0 and were removed in v0.5.1 ([releases](../product/releases.md)).

This page declares the surface an AI agent wires into its tool loop: an MCP
server speaking the streamable HTTP transport, mounted on `oml-server` as a
facade over the two-ring [JSON HTTP API](http-api.md). Every tool below
translates to one call on its ring of the HTTP API
([ADR-0022](../adr/ADR-0022-two-ring-api.md)): provider tools ride `/v1`, lab
reads and proposals ride `/v2` — invisible at the tool level. The ring split
changes no tool shape, and the verb ceiling is unchanged: which ring a tool
rides is an implementation mapping noted per tool below, never a tool
property. Nothing exists here that the HTTP API does not
also offer, and much exists there that this surface deliberately does not
(D13). The tool catalog is the **agent-safe subset**: the verbs a writing or
recalling agent needs, and none of the verbs that change what others may see.

The catalog is three task-shaped tools, [`oml_write`](#oml_write),
[`oml_read`](#oml_read) and [`oml_workarea`](#oml_workarea)
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d2). Its
machine-readable form is [mcp-tools.json](mcp-tools.json): exactly the
`tools/list` result payload, every served tool's name, description,
annotations and input schema. It is normative for the wire shape in the same
relationship [http-api.md](http-api.md) has to [openapi.yaml](openapi.yaml):
this page and that file describe the same three tools, and a divergence
between them that `_meta.pending_design` does not list is a documentation
bug, not an ambiguity to interpret. The file declares only what is served,
so a served tool's entry changes only in the same change as the code that
serves it, and the server never advertises a field it refuses. What this
page decides and the server does not serve, the tools
[`oml_relate_propose`](#oml_relate_propose) and [`oml_vocab`](#oml_vocab),
`oml_read`'s lineage mode and `oml_write`'s `attachments`, is in
[mcp-tools.designed.json](mcp-tools.designed.json), which is never served;
`scripts/check_contract_sync.py` fails when the two files name a tool in
common or mcp-tools.json differs from the served `tools/list`.

## Transport: streamable HTTP, stateless

*Available since v0.1.0; the `instructions` naming `oml_read`, and the events tail off MCP, since v0.4.0 · [releases](../product/releases.md)*

The plugin runs the MCP streamable HTTP transport in stateless mode, declared
up front:

- **No server session id.** The server issues no `Mcp-Session-Id`; a client
  that sends one is ignored, not refused. `initialize` is answered honestly
  (protocol version, capabilities, server name) and nothing is stored. Its
  `instructions`, under the 4,096 characters Claude Code keeps, tell the agent
  when to call [`oml_read`](#oml_read) with a query and with which words; the text is
  declared once, as `SERVER_INSTRUCTIONS` in
  `crates/transports/oml-transport-http/src/mcp.rs`
  ([ADR-0038](../adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md) d1).
- **Every tool call is independent.** Each HTTP POST carries one complete
  JSON-RPC request and receives one complete JSON response
  (`Content-Type: application/json`). The server opens no SSE stream in
  response to a POST, and `GET` on the MCP endpoint — the server-initiated
  stream — is refused.
- **SSE push, resumability and server-initiated messages are out of scope for
  v1.** There are no subscriptions, no progress notifications, no
  server-to-client requests.

Why this fits, stated with its cost:

- `oml-server` runs as N stateless replicas behind one service
  ([deployment](../operations/deployment.md)). A session id would demand
  sticky routing or shared session state for a surface that needs neither;
  stateless mode lets any replica answer any call.
- Agents crash, restart and retry. A session to resume is a failure mode to
  handle; an independent call retried with the same
  [idempotency key](#oml_write) is a no-op by construction
  ([idempotency records](http-api.md), D13).
- No tool in the catalog needs push. The one read shaped like a stream, the
  events tail, is not on MCP: it is `GET /v2/events`, a poll from a
  client-held journal sequence, and an agent learns what became of its writes
  with [`oml_read`](#oml_read) and `state` (ADR-0039 d2).
- The cost is real and accepted: an agent that wants to react to events polls
  and pays the latency of its polling interval, and long-running server-side
  work cannot stream progress. If a v1 use case surfaces that needs push, that
  is a new decision, not a quiet extension.

The MCP specification revision the implementation pins, and the transport
behavior verified against it, are recorded with a fetch date at implementation
time — the same sourcing discipline the auth question below already follows.

## Authentication: bearer keys and connections

*Available since v0.1.0; one key per agent since v0.2.0; OAuth by connect code since v0.4.0 · [releases](../product/releases.md)*

Every HTTP request carrying an MCP message presents one credential in the same header: the
agent's API key, or, for a client that cannot hold a key, a connection's access token:

```
Authorization: Bearer oml_k_01JB2X8F0YQZ6W4N7R9T3VH5MD
```

A **connection** binds one OAuth client — Claude's hosted apps, through a claude.ai custom
connector — to one connector key, opened by a one-time connect code an admin issues; its access
token resolves to that key on every call, so the tools below answer exactly as they would to the
key, and never more, with one exception: [`oml_attach`](#oml_attach) `open` is refused under a
connection ([roles and keys — connections](../architecture/roles-and-keys.md#connections),
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)); as built no served tool opens an
upload, since `oml_attach` left the catalog. A refused call keeps its typed `401` and carries the
OAuth challenge that starts a client's sign-in
([HTTP API — connections](http-api.md#connections-oauth-for-hosted-mcp-clients)), or `Bearer
realm="oml"` where the deployment sets no `oauth.issuer`.

The mapping is entirely server-side (D6): the key resolves to a tenant, a
role, and a set of grants over workarea subtrees
([roles and keys](../architecture/roles-and-keys.md),
[workareas](../architecture/workareas.md)). No request field names a tenant,
and a tenant named in a request body would be ignored in favor of the key's.
A call without a valid key is refused before any tool dispatch. There is no
anonymous tool, including `tools/list`.

One key per agent identity is not a convenience but a correctness input:
attribution, the reviewer-is-never-the-author check (R2) and the
reciprocal-approval statistics in the evidence export all key off the
(key, principal) pair. A key shared across crew members makes their work
mutually indistinguishable to every one of those mechanisms.

Bearer keys stay the credential of every client that can hold one — the Claude
Code plugin, scripts, reviewer crews — and nothing about them changes. The
MCP authorization specification, revision 2025-11-25 (fetched 2026-10-31),
and the clients that need it are recorded in the context of
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md).

## The verb ceiling: what this surface can never do (R1)

**No promote, sign, demote, delete or admin verb exists on this surface.**
Not refused — absent. The `tools/list` result is the entire surface, and the
catalog holds three read-and-propose tools
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d2; twelve
before it, nine at ADR-0014's writing). There is no tool whose success changes
what *other* keys can read, except by entering the admission pipeline as a
held submission that an independent reviewer must ratify
([admission](../architecture/admission.md), R3).

Concretely:

- Promotion is an effect of ratification inside the admission pipeline, never
  a callable verb — on any surface
  ([permission matrix](../architecture/roles-and-keys.md#permission-matrix)).
- Signing is done by `oml-server` with the deployment key
  ([storage](../architecture/storage.md)); no client signs anything.
- Review verdicts and the ratification of connection proposals are
  dreamer(review) verbs on the [HTTP API](http-api.md), bound to the serve
  ceremony — a server-issued, expiring `serve_id` consumed by its first verdict (D14,
  [review guide](../review-guide.md)). They are deliberately not MCP tools:
  the catalog stays writer/reader/propose-safe, so handing an agent this
  plugin config hands it no path to a verdict, whatever role its key carries.
  The crew-side verdict flow is worked end to end in
  [reviewer-crews](reviewer-crews.md).
- Demote, erase, key issuance, workarea creation, reassignment, bans and
  simulations are admin or operator verbs on the HTTP API and
  [CLI](../cli/reference.md).

An agent that tries anyway gets the MCP-level refusal for an unknown tool:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "error": {
    "code": -32602,
    "message": "Unknown tool: oml_promote. This surface carries no promote, sign, demote, delete or admin verb (R1); see tools/list for the complete catalog."
  }
}
```

## Catalog conventions

- **Who may call** states the role class whose grants carry the verb, per the
  [permission matrix](../architecture/roles-and-keys.md#permission-matrix).
  Authorization is resolved from the key's *effective access* in the named
  workarea at call time — union of grants, attenuated by denies, deny-wins
  ([workareas](../architecture/workareas.md#effective-access-union-of-attenuated-grants-then-deny-wins)) —
  never from the role name alone. A writer key recalls only where it also
  holds reader-scope grants.
- **Errors** are the typed, fail-closed taxonomy of the HTTP API, one shape
  on every surface ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md)
  d11). A served tool that refuses answers a JSON-RPC *result* with
  `isError: true`, carrying the [errors.md envelope](errors.md#the-envelope)
  — `{error: {code, message, rule, details?, retryable}, request_id}` — both
  as `structuredContent` and, serialized, as its one text content, so a
  client that reads only text sees the same refusal. `request_id` mirrors the
  `X-Request-Id` of the `/mcp` response. A JSON-RPC *error* is a protocol
  fault only: a malformed message, an unknown method, `tools/call` params
  that are not `{name, arguments}`, and -32602 for a tool the server does not
  serve, whether unknown, designed, or a v0.3.0 tool with no successor tool. Every error names
  the rule that refused the call. The taxonomy — spellings, retryability,
  HTTP status mapping including the 429 quota family — is declared in
  [errors.md](errors.md); the examples below use its codes, and a retryable
  refusal carries a `Retry-After` header in seconds. Designed, with
  [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d7's renames
  and not yet served: a field sent under a spelling v0.4.0 renamed is
  `request.validation` with `details.field` and `details.renamed_to`; no
  code is added for it. Until then the v0.3.0 argument names are the served
  ones. One shared example, the
  per-replica-approximated quota limit (D6, [FR-8](../product/roadmap.md)),
  as the `result` of a `tools/call`:

```json
{
  "content": [{"type": "text", "text": "{\"error\":{\"code\":\"limit.quota\",…},\"request_id\":\"req_01J…\"}"}],
  "structuredContent": {
    "error": {
      "code": "limit.quota",
      "rule": "tenant quota (journalled grant attribute; enforced per replica at limit/N)",
      "message": "write quota exhausted for this key's tenant",
      "retryable": true
    },
    "request_id": "req_01J…"
  },
  "isError": true
}
```

- **Out-of-scope reads fail closed without leaking existence**: asking about
  a workarea or artifact the key cannot read returns the same typed error as
  asking about one that does not exist.
- **A workarea by id or name.** Every argument that names a workarea —
  `workarea` on the three tools, and a workarea edge's `from` and `to` on the
  designed `oml_relate_propose` —
  takes the node's id or its [name](http-api.md#names-and-ids)
  ([ADR-0026](../adr/ADR-0026-names-are-keys.md)). The facade passes the value
  through as the ring's `workarea` field
  ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d7), whose
  JSON Pointer, `/workarea`, a refusal's `details.field` names, and the server
  resolves it, so a name the key may not see answers
  `access.not_found` exactly as its id would. A derived path is neither, and
  is refused with `request.validation`.
- **Pagination** is cursor-based everywhere a list can grow; the client holds
  the cursor (stateless), and the cursor contract — including the events
  tail's deliberate `after_seq` exception — is owned by
  [http-api.md](http-api.md).
- **Simulation fencing** applies to every read below: rows with
  `provenance: simulation` are served only to keys in that simulation's
  audience ([simulations](../architecture/simulations.md)). This is a
  predicate, not an error — fenced rows are simply absent.
- All identifiers in the examples are illustrative.

| Tool | One line | Annotations | Role class |
|---|---|---|---|
| [`oml_write`](#oml_write) | Store a claim, a derived claim, a new version or a retire-proposal — held on arrival | `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: false` | writer; with `replaces`, writer (own claims) or dreamer (propose) |
| [`oml_read`](#oml_read) | Fetch one version, recall in a subtree, or list own work or a workarea's tips | `readOnlyHint: true`, `idempotentHint: true`, `openWorldHint: false` | reader, or the calling key for its own rows |
| [`oml_workarea`](#oml_workarea) | One workarea: status, crew, the caller's effective verbs, pending transitions and the brief | `readOnlyHint: true`, `idempotentHint: true`, `openWorldHint: false` | reader |

The annotations are [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md)
d5's: Claude Code runs read-only calls in parallel and may repeat a pending
idempotent one ([MCP in Claude Code](https://code.claude.com/docs/en/mcp),
fetched 2026-11-03), and a write here is held for review, never destructive.

## The claim item

One shape describes a claim version wherever a tool returns one — every mode
of [`oml_read`](#oml_read): get's item, recall's steering items and hits, and
the listing's items — so the three are interchangeable to an agent
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d12). Every field
comes from the ring's `MemorySummary` and `Memory`
([openapi.yaml](openapi.yaml)), whose `text` it keeps, and the facade adds
nothing else.

| Field | Meaning |
|---|---|
| `artifact_id` | This version's id; each version of a claim has its own |
| `claim_key` | The claim the version belongs to |
| `version` | Its ordinal within the `claim_key` |
| `kind` | `memory`, `derived_fact`, `steering`, or a vocabulary kind |
| `state` | `held`, `promoted`, `demoted`, `rejected`, `erased` |
| `tip` | Whether it is the claim's served tip for the calling key, audience-scoped: for a key in a simulation's audience the overlay tip, which shadows the default tip, and for every other key the default tip ([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)); a superseded version is `promoted` with `tip: false` |
| `retirement` | True on a retirement version ([http-api](http-api.md#supersede-post-v1memoriesartifact_idsupersede)) |
| `replaces` | `{artifact_id, version}` of the version a supersession replaces; null for version 1, and null when the calling key cannot read the replaced version, under the same test as `derives_from` ([http-api](http-api.md#get-get-v1memoriesartifact_id)) |
| `workarea` | The workarea node id; null when the calling key holds no role on the node, which only an own row an admin moved there can show ([naming a node](../architecture/roles-and-keys.md#own)) |
| `workarea_path` | The derived path of that node; null wherever `workarea` is |
| `tokens` | Budget tokens of the body under counter `oml-tc/1` — not any model's count ([retrieval](../architecture/retrieval.md#token-budgets)) |
| `created_at` | When the version was written |
| `promoted_at` | When it was ratified; null otherwise |

Every `oml_read` item carries `text`. Per mode, the item adds: `score` on
recall hits only; `held_expires_at` in the listing; `derives_from` and
`attachments` in get. The v0.3.0 sections below state what each old tool's
item carried. `derives_from` lists only the ancestors the calling
key can read, as on the ring
([http-api](http-api.md#get-get-v1memoriesartifact_id)). Recall steering
items carry `text` and `tokens` like every other item, never a score.

---

## `oml_write`

*Available since v0.4.0 · [releases](../product/releases.md)*

Store a claim, a claim derived from others, a new version of one, or a
proposal to retire one. Every write is **held on arrival**: scanned, then
reviewed by an identity that did not write it, and visible to others only
after ratification (R2, R3; [admission](../architecture/admission.md)).
Success means *accepted for review*, never *published*.
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d3.)

**Annotations:** `readOnlyHint: false`, `destructiveHint: false`,
`idempotentHint: true`, `openWorldHint: false`. A write is held for review
and a retirement is a proposal, so no call destroys anything, and the
`idempotency_key` makes a repeat a replay (d5).

**Who may call:** keys with writer effective access on the target workarea;
with `replaces`, the target's author, or dreamer(propose) otherwise, as on
the ring ([supersede](http-api.md#supersede-post-v1memoriesartifact_idsupersede)).

**The mode is chosen by the fields present.** One flat argument set; `text`
and `idempotency_key` are required:

| Mode | Chosen by | Takes | Refused, each `request.validation` naming it in `details.field` | Rides |
|---|---|---|---|---|
| `submit` | no `replaces` | `claim_key`, `kind` (default `memory`), `derives_from`, `workarea` | `retirement`, `reason` | `POST /v1/memories`, journalled `submission.learn` when `derives_from` is non-empty and `submission.remember` otherwise |
| `supersede` | `replaces` | `kind`, `derives_from`, `workarea`, `retirement`, `reason` | `claim_key` (the new version takes the target's); a `kind` other than `memory`, `derived_fact` or `steering`; a `workarea` not the target's; a target of a vocabulary kind | `POST /v1/memories/{replaces}/supersede`; `retirement: true` is ADR-0025 d11's retire-proposal, `text` stating why |

```json
{
  "text": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery, valid until 2026-11-15. Quote ref AM-7741.",
  "derives_from": ["8f0c2d4e-1b7a-4c39-9e55-3a6d2f81b0c4"],
  "idempotency_key": "crew-procure/run-4412/step-9"
}
```

- `claim_key` is **optional, and an agent leaves it out**: the server derives
  `auto-` plus 16 hex digits from the authoring key and the NFC text, and a
  sent key starting `auto-` is refused (d6). An agent invents no key.
- `workarea` takes an id or a name; omitted, a submission targets the key's
  default workarea (`request.workarea_unresolved` for a key with none).
- `idempotency_key` is scoped to (key, resolved workarea) without `replaces`
  and to the calling key alone with it, as the two endpoints scope it; the
  same key reused across the two modes is a separate write (D13).
- Designed, in [mcp-tools.designed.json](mcp-tools.designed.json):
  `attachments`, the upload sessions a file was put through outside MCP, and
  `simulation_id` with `simulated_time`, an audience key's write inside a
  simulation, in submit and supersede mode alike, as the v0.3.0 writes
  declared them. Each is refused as outside the input schema until it is
  built; `x-oml-designed-fields` in
  [openapi-designed.yaml](openapi-designed.yaml) keeps the same
  fields for HTTP.

**Output**

```json
{
  "mode": "submit",
  "artifact_id": "0b7e5a1c-6d2f-4e83-9a41-c5f8d3b2e907",
  "claim_key": "auto-3f9a1c0e7b2d4a68",
  "version": 1,
  "state": "held",
  "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "retirement": false,
  "replaces": null,
  "idempotent_replay": false
}
```

The ring's own refusals pass through unchanged: a frozen workarea, a tip
conflict (`submission.tip_conflict` naming `details.current_tip`), a
`claim_key` conflict, an idempotency key reused with another request, a
banned author.

## `oml_read`

*Available since v0.4.0 · [releases](../product/releases.md)*

Find, fetch or list claims. Every mode answers
[claim items](#the-claim-item) with their `text`, under a `mode` field
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d4).

**Annotations:** `readOnlyHint: true`, `idempotentHint: true`,
`openWorldHint: false` (d5): Claude Code may run it beside other read-only
calls without asking, once a rule allows it.

**Who may call:** reader effective access on the subtree read, or the
calling key for its own rows.

**The mode is chosen by the fields present.** `artifact_id` fetches, `query`
recalls, and neither lists. An argument outside the chosen mode, or
`artifact_id` and `query` together, is refused with `request.validation`
naming it in `details.field`:

| Mode | Arguments | Output | Rides |
|---|---|---|---|
| `get` | `artifact_id`; designed: `lineage` (`ancestors` \| `descendants`), `depth` | `{mode, items}`, one item with `derives_from` and `attachments` | `GET /v1/memories/{artifact_id}`; the designed lineage read |
| `recall` | `query`, `workarea`, `k`, `kinds`, `include_unreviewed`, `max_tokens`, `search`, `match`, `include_expired`, `now` | `{mode, steering, hits, search, budget?}` | `POST /v1/memories/recall` |
| `listing` | `own` (default `true`), `workarea`, `claim_key`, `state`, `kind`, `since`, `until`, `limit`, `cursor` | `{mode, items, next_cursor}` | `GET /v1/memories` |

- *Proposed ([ADR-0058](../adr/ADR-0058-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model.md)):*
  `oml_write` takes `valid_from` and `valid_until`, when the claim holds, each a period
  (`2015`, `2015-03`, `2015-03-14`) or an instant; every claim item carries both. Recall
  hides a claim whose `valid_until` has passed unless `include_expired` is true, reads at
  `now` (the server's clock when omitted), and names in `search.window` the date window it
  read from the query, which the `bm25-postings-time` pipelines search.
- **get** answers the calling key's own rows in any state and others'
  promoted versions within read access; anything else is
  `access.not_found`, as a nonexistent id is.
- **recall** searches promoted claims in the subtree, the key's default
  workarea when `workarea` is omitted. Steering comes first, never ranked
  and never cut by `max_tokens`; the hits served are the longest rank prefix
  that fits, and `budget` reports the cut. `include_unreviewed: true`
  (default `false`) adds the key's own held rows that match, flagged
  `state: "held"`; it is the one flag for unreviewed rows (d4).
- **recall's search** *(since v0.5.5, [ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md))*:
  `search` names the pipeline (`keyword-all`, `keyword-any`, `bm25`,
  `bm25-trigram`, and, since v0.5.6, `bm25-postings` and the opt-in
  `bm25-rerank`) among those the server
  serves; omitted, the
  organisation's default runs, `bm25` as v0.5.5 shipped and, since v0.5.6,
  `bm25-postings`. Any word may match: an
  identifier matches by its parts and a word by its stem; English stop words
  are not read when the query has other words; `bm25-trigram` also matches a
  misspelling by its letters. `match: "all"` keeps only memories holding
  every word, and when none does serves the any-word hits with
  `search.fallback: "any_term"`. The answer's `search` names the pipeline,
  the query's words as read and, when fewer than three hits are found,
  `did_you_mean` spellings for the words no hit holds, from the words of
  promoted memories the key may read, and, since v0.5.6, `access` (`postings`
  or `scan`) and `pruned` (the frequent words `bm25-postings` left out of
  what it read, still scored in each hit); each hit carries
  `matched` (`terms`, `of`, `via`). A pipeline the server does not serve is
  `request.validation` naming `/search`. The three tools stay three.
- **listing** with `own: true`, the default, is everything the key wrote,
  newest first, in every state and every workarea it wrote in; with
  `own: false` it is the promoted tips of the workarea's subtree. `since`
  takes a version written at its instant and `until` leaves it out.
- **An agent learns what became of its writes here**: `oml_read` with
  `state: "promoted"` or `state: "rejected"` over its own work, with no
  cursor to hold and no event tool (d2).
- The lineage mode, the successor of `oml_graph`, is designed in
  [mcp-tools.designed.json](mcp-tools.designed.json): `lineage` and `depth`
  are refused with `request.validation` naming the field until the lineage
  read is routed.

## `oml_workarea`

*Available since v0.4.0 · [releases](../product/releases.md)*

One workarea as the calling key may see it: its identity, derived path,
status, crew record, readable immediate children, and the verbs the key's
effective access carries there, `my_effective_verbs`, with the permission
epoch. It returns a node, not claim items, which is why it is not a mode of
`oml_read`. `workarea` is optional, an id or a name; omitted, it is the
key's default workarea (`request.workarea_unresolved` for a key with none).
(Rides `GET /v2/workareas/{workarea}`, one call.)

**Annotations:** `readOnlyHint: true`, `idempotentHint: true`,
`openWorldHint: false` (d5).

**Who may call:** keys with reader effective access on the workarea, or an
admin key for its structure read. The output is
[`oml_workarea_info`](#oml_workarea_info)'s, under the v0.4.0 name, and three
fields more (*since v0.5.2*;
[ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) d3, d9): `policy`, the review policy in force;
`pending_transitions`, the status proposals waiting for a second identity;
and `brief`, `{items, budget}`, the claims the workarea's reviewers agreed an
agent entering it reads first, each the claim item every tool returns, with
its text, cut by the server's `brief.max_tokens`. No tool proposes or ratifies a
transition or an edge: those run on the HTTP API and the CLI.

## The prompt `guardrails`

*Since v0.5.1 · [ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md) d11 · [releases](../product/releases.md)*

Beside its three tools the server declares the `prompts` capability and serves one prompt,
`guardrails`, with no arguments. `prompts/get` answers one user message holding the text
`oml hook guardrails` prints for the key's default workarea, from the same read and the same
renderer, with no line marked checked, since no rule cache stands behind it. Prompts are run by
a person, so this reaches Claude's apps only when someone invokes it; a client with the plugin's
hooks loads the set at every session start instead. A guardrail is never written through MCP:
`oml_write` refuses `kind: guardrail` and a `replaces` naming one.

## What left the catalog

- **The events tail is not on MCP.** An agent's one use for it, learning
  whether a write was approved or rejected, is `oml_read` with `state`. The
  tail stays on `GET /v2/events`, the SDKs' `events` and the CLI, for
  auditors, a poll from a client-held journal sequence
  ([http-api.md](http-api.md)).
- **`oml_attach` has no successor tool.** MCP carries no file bytes, so an
  agent would still need a terminal for the parts. Files go through the
  designed `oml attach put <file>` and the SDKs' upload, whose session
  `oml_write`'s designed `attachments` names. A call to `oml_attach` is
  JSON-RPC -32602 saying so.
- **`oml_graph` is `oml_read`'s designed lineage mode**, since lineage
  returns claim items. A call to `oml_graph` is -32602 saying so.
- **`oml_relate_propose` and `oml_vocab` are designed**, in
  [mcp-tools.designed.json](mcp-tools.designed.json), which is never served;
  a call to either is -32602 naming that file.
- **The eight v0.3.0 tool names were removed in v0.5.1.** Through v0.5.0
  they were hidden aliases, unlisted but answering with their v0.3.0 inputs,
  output shapes and refusal codes
  ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d12). Since
  v0.5.1 a `tools/call` naming one is JSON-RPC -32602 with the
  `Unknown tool` message, as for any name the server does not serve. Their
  sections [below](#the-v030-tools-removed-in-v051) keep their contracts as
  a historical record. Each maps to its successor:

| v0.3.0 | v0.4.0 |
|---|---|
| `oml_remember`, `oml_learn` | `oml_write`; `derives_from` journals learn |
| `oml_supersede` | `oml_write` with `replaces` |
| `oml_recall` | `oml_read` with `query`; `include_unreviewed` for `include_own_held`, `workarea` for `scope` |
| `oml_get` | `oml_read` with `artifact_id` |
| `oml_my_work` | `oml_read` with neither, `own` defaulting to `true` |
| `oml_workarea_info` | `oml_workarea` |
| `oml_events_tail` | `oml_read` with `state` for an agent's own writes; `GET /v2/events` to audit |

**Migration.** A `permissions.allow` rule naming an old read tool allows
nothing: allow `mcp__plugin_oml_oml__oml_read` and
`mcp__plugin_oml_oml__oml_workarea`, or the server-wide
`mcp__plugin_oml_oml`, which allows `oml_write` too
([the plugin README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md);
amends [ADR-0029](../adr/ADR-0029-session-start-hook.md) d8). Run
`claude plugin update oml@open-memory-lab` and the v0.4.0 CLI, so the cached
skill and the session-start hook name the three tools (d13), then set the
plugin's `url` and `api_key` settings once, since its entry no longer reads
`OML_URL` and `OML_API_KEY`
([ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md) d2,
[below](#wiring-it-into-your-agent)).

## A new capability is a parameter, a resource or a hook first

A new agent capability arrives as a parameter of one of the three tools, an
MCP resource or a hook before it is a tool. A fourth served tool needs a
record saying why no parameter of the three, no resource and no hook
carries it ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d8).
Each tool costs every session its catalog entry and every user a permission
rule.

## What the catalog costs

The three served entries of [mcp-tools.json](mcp-tools.json) are 8,488
characters, against 18,893 for the twelve entries of v0.3.0, each entry
measured the same way: serialized as JSON with the default separators,
summed over the entries. That is the text every session's `tools/list` puts
in front of the model.

## Designed tools, never served

These two tools are decided and not served; their entries are in
[mcp-tools.designed.json](mcp-tools.designed.json), and each moves to
mcp-tools.json in the change that serves it, under d8's test below. Their
workarea arguments are `workarea`, as on the served tools
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d7).

### `oml_relate_propose`

*Not built; links between memories arrived in v0.5.2 on HTTP and the CLI, and this tool is not
part of them ([ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) d4) · [releases](../product/releases.md)*

Propose a labelled edge — claim↔claim or workarea↔workarea. The proposal is
journalled and **pending**; it confers nothing and changes no access until a
*different* dreamer ratifies it (R2 extends to edges). Ratification is not on
this surface — it is a dreamer(review) verb behind the serve ceremony on the
[HTTP API](http-api.md). (Rides the designed `POST /v2/edges`, `kind` `claim` or
`workarea` — the lab ring.)

**Who may call:** keys with dreamer(propose) effective access. For a
workarea edge: propose access on at least one endpoint **and** reader access
on both. For a claim edge: both claims readable by the key. The
edge-authority rule is declared in
[workareas](../architecture/workareas.md#edge-authority).

**Input** — a workarea edge (closed label set: `relates_to`, `continues`,
`derived_from`, `mirrors`):

```json
{
  "kind": "workarea",
  "label": "continues",
  "from": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "to": "a4497daf-cfc8-4ddc-a0e5-4817efdb56c8",
  "note": "metals-crew Q1 planning continues the Q4 negotiation workarea"
}
```

A claim edge uses `kind: "claim"` with artifact ids as endpoints; claim-edge
proposals are part of the admission pipeline and enter the lineage closure
computed at promotion time, pending and ratified alike
([audit and lineage](../architecture/audit-and-lineage.md)). The claim-edge
label vocabulary is not fixed by the design — an open detail tracked in the
[roadmap](../product/roadmap.md) ([http-api.md](http-api.md)); the proposal
event's payload shape is owned by the
[event catalog](../data/event-catalog.md).

**Output**

```json
{
  "edge_id": "we_01JB2Y6T8V1X3Z5B7D9FH2KM4P",
  "kind": "workarea",
  "label": "continues",
  "state": "pending",
  "proposer_key": "d405428e-4c99-426d-8826-76d28436e423",
  "awaiting": "ratification by a dreamer(review) key on both endpoints, ratifier ≠ proposer (R2)"
}
```

**Refusals** — missing propose access on every endpoint; an endpoint outside
read scope (fail closed, indistinguishable from nonexistent); a label outside
the closed set for workarea edges. Example:

```json
{
  "error": {
    "code": "access.not_found",
    "rule": "edge proposal requires dreamer(propose) on ≥1 endpoint and reader on both (D7)",
    "message": "endpoint 'to' names no workarea readable by this key",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_vocab`

*Parked · [releases](../product/releases.md)*

Read the tenant's vocabularies: term lookup, deterministic typeahead,
hierarchy over the per-release closure, and SSSOM-shaped mappings. One tool,
four actions — each translates to one `GET /v2/vocab/*` call (the lab ring;
[http-api.md](http-api.md#vocabularies)). Strictly read-only at the reader
ceiling: proposing a term or mapping is [`oml_write`](#oml_write) with
the matching `kind`, and nothing on this surface deprecates, ratifies or cuts
a release.

**Who may call:** keys with reader effective access on the vocabulary
workarea(s) involved. Vocabularies outside scope are absent, fail closed.

**Input** — `action` ∈ `suggest` | `get` | `hierarchy` | `mappings`:

- `suggest` — `{q, workarea?, limit?, cursor?}`: prefix+label typeahead
  over granted vocabulary workareas, deterministic matching only. Returns
  `{curie, label, term_status}` per suggestion.
- `get` — `{curie}`: the term's served tip.
- `hierarchy` — `{curie, direction: parents | children | ancestors |
  descendants, release?, limit?, cursor?}`: a lookup over the closure
  materialized at release ratification; `release` defaults to the owning
  vocabulary's latest ratified release.
- `mappings` — `{term, limit?, cursor?}`: ratified mappings touching the
  CURIE — subject, object, predicate, justification, `ratified_at`.

```json
{ "action": "get", "curie": "proc:supplier-quote" }
```

**Output** — `get` returns `term_status` and the ratified successor pointers,
so an agent whose citation is stale self-heals in one hop:

```json
{
  "curie": "proc:supplier-quote",
  "workarea": "408324c0-55dd-4eb7-b39f-2d7f7147dc66",
  "version": 4,
  "term_status": "deprecated",
  "definition_state": "present",
  "successors": [
    { "curie": "proc:price-quote", "relation": "replaced_by" }
  ],
  "releases": ["rel_01JB2Z2D4F6H8K1M3P5R7TVXZB"]
}
```

`replaced_by` is safe for automated rewiring; `consider` requires judgment.
The same two codes an agent meets when *citing* terms on a submit apply here
in read form: an unknown or out-of-scope CURIE is absence
(`access.not_found` — the read twin of `vocab.unknown_curie`, which fails a
citation closed exactly as `derives_from` does), and a submission citing a
deprecated term under `enforcement: require` is refused with
`vocab.inactive_curie`, whose `details` carry the successors and relation —
under `annotate`, the citation records a finding and never blocks
([errors.md](errors.md)).

**Refusals** — a CURIE or vocabulary outside read scope, indistinguishable
from one that does not exist. Example:

```json
{
  "error": {
    "code": "access.not_found",
    "rule": "CURIE resolution fails closed within the key's effective read scope (D7)",
    "message": "no readable term proc:supplier-quote for this key",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

## The v0.3.0 tools, removed in v0.5.1

None of these is in `tools/list`
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d2). The eight
served in v0.3.0 were removed in v0.5.1: each now answers JSON-RPC -32602
with the `Unknown tool` message, and its successor is in
[What left the catalog](#what-left-the-catalog). `oml_attach` and
`oml_graph`, listed in v0.3.0 and never served, answer -32602 naming their
successors. The sections below are the v0.3.0 contracts, kept as the
historical mapping; where one names a sibling old tool, read its successor.

**The alias window, closed in v0.5.1.** Through v0.5.0 a `tools/call` naming one of the eight
took the v0.3.0 input below, `scope` and `include_own_held` included and
`claim_key` required, and answered the v0.3.0 output below with the v0.3.0
refusal codes: the facade translated `scope` to the ring's `workarea` and
`include_own_held` to its `include_unreviewed`, and a v0.4.0 spelling such
as `include_unreviewed` was outside the old input schema, so it was
`request.validation`. `oml_events_tail` rode `GET /v2/events`. Every call,
refused or not, logged one line with `event` `mcp_alias_called` naming the
tool, its successor (`oml_write`, `oml_read`, `oml_workarea`, or
`GET /v2/events` for the tail), the calling key's `key_id` and `tenant_id`,
and the call's `request_id`: ids only, never the key's value. **v0.5.1
removed the eight**: a call to one is -32602
like any tool not served, and no call logs `mcp_alias_called` (d12).

### `oml_remember`

*Available since v0.1.0 · [releases](../product/releases.md)*

Submit a memory. The text, any attachments (by upload-session reference) and
the declared lineage enter the admission pipeline as one claim, **held on
arrival** — scanned, then reviewed by an identity that did not write it,
and visible to others only after ratification (R2, R3;
[admission](../architecture/admission.md)). Success here means *accepted for
review*, never *published*. (Rides `POST /v1/memories` — the provider ring.)

**Who may call:** keys with writer effective access on the target workarea.

**Input**

```json
{
  "workarea": "metals-crew",
  "claim_key": "supplier-quote-am-7741",
  "text": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery, valid until 2026-11-15. Quote ref AM-7741.",
  "attachments": [
    { "upload_session": "ups_01JB2XD9W6E4T2Y8U1O3PA5SKF" }
  ],
  "derives_from": [
    "art_01JB2WYT5H8K3N6Q9S2VX4ZC7B"
  ],
  "idempotency_key": "crew-procure/run-4412/step-9"
}
```

- `workarea` — **optional** (ADR-0022 d4): omitted, the submission targets
  the key's journalled default workarea; present, it is an override honored
  only within the key's grants. A key with neither a default nor a value here
  is refused (`request.workarea_unresolved`). The value is the target node's
  id or name, which the server resolves to the id grants bind to; a derived
  path is not accepted
  ([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)).
- `attachments` — upload sessions previously completed via
  [`oml_attach`](#oml_attach). The session's recorded digest is bound into
  the signed submission envelope (D3).
- `kind` — `memory` (the default) | `derived_fact` | `steering` |
  `term` | `term_mapping` | `vocab_release`, as on the HTTP submit
  ([http-api.md](http-api.md)). `kind: derived_fact` here is the
  path for a derived conclusion whose lineage cannot or will not be named —
  [`oml_learn`](#oml_learn) requires non-empty `derives_from`. Proposing
  vocabulary — a `term` or `term_mapping` — is this same ordinary submit into
  a vocabulary workarea the key holds writer grants on; `vocab_release` is
  also a claim and rides the same path, but ratifying one is a
  human-required verdict under the shipped review policy (as are
  `replaced_by` deprecations and `exactMatch` mappings) — no release is ever
  cut by an agent tool. Vocabulary workareas refuse `provenance: simulation`
  submissions of these kinds ([http-api.md](http-api.md#vocabularies)).
- `derives_from` — artifact ids this memory derives from. Lineage is
  **declared**, and the honesty rule applies in full: a writer that omits
  `derives_from` is not detectable by the service, so lineage-based review
  exclusion is exactly as good as the declared graph (R2 scope,
  [audit and lineage](../architecture/audit-and-lineage.md)).
- `simulation_id` / `simulated_time` — **optional** pass-through of the HTTP
  submit body's simulation fields ([http-api.md](http-api.md)), for audience
  keys writing inside a simulation — and required whenever `derives_from`
  touches a simulation row; `simulated_time` is declared data, never
  backdating.
- `idempotency_key` — scoped to (key, workarea), where the workarea component
  is the resolved target — the key's default or the explicit override
  (ADR-0022 d4). A retry with the same key
  and an identical request returns the stored response; the same key with a
  *different* request is a typed error, never a second write (D13,
  [http-api.md](http-api.md)).

**Output**

```json
{
  "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
  "claim_key": "supplier-quote-am-7741",
  "version": 1,
  "state": "held",
  "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "attachments_bound": 1,
  "idempotent_replay": false
}
```

`claim_key` groups the versions of one claim; the caller supplies it, and
when a new claim collides with an existing version of it
(`submission.claim_key_conflict`) is declared on
[audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).
Proposing a *new version* of an existing claim, or its retirement, is
[`oml_supersede`](#oml_supersede).

**Refusals** — writing without writer effective access on the node; writing
into a `validated` (frozen) workarea or directly into `universe`; an unknown
or incomplete upload session; a digest mismatch between session and envelope;
an idempotency-key reuse with a different request; a banned author or source
(checked at admission). Example — frozen target:

```json
{
  "error": {
    "code": "workarea.frozen",
    "rule": "status ladder: a validated workarea refuses new writes (D8)",
    "message": "workarea fbbb41c5… is validated; writes are refused. Demote, erasure and crew-metadata update are its only admissible mutations, none on this surface.",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_recall`

*Available since v0.1.0; own held rows, budgets and steering first since v0.2.0 · [releases](../product/releases.md)*

Search promoted memories within a workarea subtree — and, on request, the
caller's own held rows — under an optional budget. FTS and vector search are
served from the Postgres serving tier, never the lake
([retrieval](../architecture/retrieval.md), D12). Every recall is logged to
the read log: key, workarea, the request digest, the ids served after
truncation, timestamp ([retrieval](../architecture/retrieval.md#the-read-log)).
(Rides `POST /v1/memories/recall` — the provider ring.)

**Who may call:** keys with reader effective access on the scope.

**Input**

```json
{
  "query": "current aluminium quote Accra Metals validity",
  "scope": "metals-crew",
  "k": 8,
  "include_own_held": true,
  "max_tokens": 500
}
```

`scope` is **optional** (ADR-0022 d4): omitted, the search covers the key's
default workarea's subtree; present, it names one node, by id or name, as an
override within the key's grants. Either way the search covers that node's subtree intersected
with the key's effective read access. Subtree scoping rides the derived-path
filter column of the serving indexes
([retrieval](../architecture/retrieval.md#the-serving-tier)); hybrid
FTS + vector is the only search mode. `k` is at most 100, as on the ring.
The tool keeps its v0.3.0 argument names until `oml_read` replaces it:
`scope` rides the ring's `workarea` and `include_own_held` its
`include_unreviewed` ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md)
d7).

- `include_own_held` — **optional**, default `false`: the calling key's own
  held rows that match join the hits, ranked in the same order and flagged
  `state: held`; they count against `k` and the budget
  ([retrieval](../architecture/retrieval.md#own-held-rows-in-recall)).
- `max_tokens` — **optional**: a budget in budget tokens (counter `oml-tc/1`,
  not a model's count), at most 2⁵³ − 1, as on the ring. Steering is never
  cut; the hits served are the longest rank prefix that fits; the cut is
  reported in `budget`
  ([retrieval](../architecture/retrieval.md#token-budgets)).

**Output** — the steering-first contract, visible in the shape itself:

```json
{
  "steering": [
    {
      "artifact_id": "art_01JB2V8K3M6P9R2T5W8Z1CF4HQ",
      "claim_key": "quote-validity-steering",
      "version": 1,
      "kind": "steering",
      "state": "promoted",
      "tip": true,
      "retirement": false,
      "replaces": null,
      "workarea": "1351b2b6-5c7c-492d-9aff-630bc95035db",
      "workarea_path": "universe.procurement",
      "tokens": 20,
      "created_at": "2026-10-02T09:20:41Z",
      "promoted_at": "2026-10-02T11:05:12Z",
      "text": "Prefer quotes with an explicit validity date; treat undated quotes as expired."
    }
  ],
  "hits": [
    {
      "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
      "claim_key": "supplier-quote-am-7741",
      "version": 1,
      "kind": "memory",
      "state": "promoted",
      "tip": true,
      "retirement": false,
      "replaces": null,
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
      "workarea_path": "universe.procurement.metals-crew",
      "tokens": 26,
      "created_at": "2026-10-27T13:58:20Z",
      "promoted_at": "2026-10-27T14:11:08Z",
      "score": 0.83,
      "text": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery, valid until 2026-11-15. Quote ref AM-7741."
    },
    {
      "artifact_id": "art_01JB2YN4P6R8TA1CW3EY5G7J9L",
      "claim_key": "supplier-quote-am-7741",
      "version": 2,
      "kind": "memory",
      "state": "held",
      "tip": false,
      "retirement": false,
      "replaces": { "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "version": 1 },
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
      "workarea_path": "universe.procurement.metals-crew",
      "tokens": 30,
      "created_at": "2026-10-29T15:40:02Z",
      "promoted_at": null,
      "score": 0.79,
      "text": "Supplier Accra Metals revised the quote to 408 USD/t for Q1-2027 delivery, valid until 2026-12-01. Quote ref AM-7741-r2."
    }
  ],
  "budget": {
    "max_tokens": 500,
    "counter": "oml-tc/1",
    "steering_tokens": 20,
    "used_tokens": 76,
    "truncated": false,
    "hits_omitted": 0,
    "next_hit_tokens": null
  }
}
```

The contract (D12, declared in
[retrieval](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked)):
**steering rows precede hits and are never ranked** — they carry no score and
no position among the hits, however well or badly they match the query, and a
budget never cuts them. Hits are promoted served tips plus, with
`include_own_held`, the caller's own held rows — here the caller's held
version 2, listed beside the promoted version 1 it would replace; the two are
never merged. Every item is a [claim item](#the-claim-item) with `text` and
`tokens`; `budget` is present if and only if `max_tokens` was sent. Recall
quality and latency are unmeasured — [FR-4](../product/roadmap.md).

**Refusals** — scope outside the key's read access (same error as a
nonexistent node, fail closed); `max_tokens` with `include_text: false` on
the ring, or any field the schema does not declare (`request.validation`).
Example:

```json
{
  "error": {
    "code": "access.not_found",
    "rule": "effective access (D7); unknown and out-of-scope are indistinguishable",
    "message": "no readable workarea 821e1387… for this key",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_learn`

*Available since v0.1.0 · [releases](../product/releases.md)*

Submit a **derived fact**: a conclusion distilled from memories the agent has
already read, carrying the lineage that produced it. Mechanically it is one
`POST /v1/memories` (the provider ring) with `verb: "learn"` and
`kind: derived_fact` — the same
admission pipeline as [`oml_remember`](#oml_remember), held on arrival, same
non-author review, journalled as `submission.learn` — but `derives_from` is
the point of the verb: it must be non-empty, and a call with empty or omitted
lineage is refused with `submission.lineage_required`
([http-api.md](http-api.md), [errors.md](errors.md)). A derived conclusion
whose author cannot or will not name its lineage is submitted through
[`oml_remember`](#oml_remember) with `kind: derived_fact` instead (journalled
`submission.remember`), where the undeclared-derivation caveat applies: the
service cannot know what the writer really read, so the lineage closure is
only as good as what writers declare. `workarea` is optional here exactly as
on [`oml_remember`](#oml_remember): omitted, the key's default workarea is
the target (ADR-0022 d4).

**Who may call:** keys with writer effective access on the target workarea.

**Input**

```json
{
  "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "claim_key": "accra-metals-price-anchor",
  "text": "Accra Metals has undercut the next-best aluminium quote in three consecutive quarters; treat them as the price anchor for Q1-2027 negotiations.",
  "derives_from": [
    "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
    "art_01JB2M4N6P8R1T3V5X7Z9BD2FH",
    "art_01JB2A2C4E6G8J1L3N5Q7S9UWY"
  ],
  "idempotency_key": "crew-procure/run-4412/distill-2"
}
```

**Output**

```json
{
  "artifact_id": "art_01JB2XJH8K1M3P5R7T9W2YA4CE",
  "claim_key": "accra-metals-price-anchor",
  "version": 1,
  "kind": "derived_fact",
  "state": "held",
  "lineage_declared": 3,
  "idempotent_replay": false
}
```

**Refusals** — everything `oml_remember` refuses, plus the lineage checks:
an empty or omitted `derives_from` is refused (`submission.lineage_required`
— use `oml_remember` with `kind: derived_fact` for a conclusion without
nameable lineage); a `derives_from` target the key cannot read is refused as unknown (fail
closed, no existence leak), and a target inside a simulation forces the
simulation fence — a submission deriving from a simulation row must carry
that row's `simulation_id`, server-enforced, which this key can only do if it
is in the simulation's audience (D10,
[simulations](../architecture/simulations.md#no-nesting)). An audience key
supplies that id through the same optional `simulation_id` / `simulated_time`
pass-through fields as on [`oml_remember`](#oml_remember). Example:

```json
{
  "error": {
    "code": "access.not_found",
    "rule": "lineage targets must be readable by the submitting key (fail closed)",
    "message": "derives_from[1] names no artifact readable by this key",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_get`

*Available since v0.2.0 · [releases](../product/releases.md)*

Read one claim version by id: the caller's own rows in any state, anyone
else's promoted versions within read access. It is how an agent reads back
what it wrote before review, and how it follows a `replaces` or
`derives_from` pointer. (Rides `GET /v1/memories/{artifact_id}` — the
provider ring.)

**Who may call:** keys with reader effective access on the row's workarea; any
tenant key except operator for its [own](../architecture/roles-and-keys.md#own)
rows, with no grant needed (global denials still mask).

**Input**

```json
{ "artifact_id": "art_01JB2YN4P6R8TA1CW3EY5G7J9L" }
```

**Output** — the [claim item](#the-claim-item) with `text`, `derives_from`
and `attachments`:

```json
{
  "artifact_id": "art_01JB2YN4P6R8TA1CW3EY5G7J9L",
  "claim_key": "supplier-quote-am-7741",
  "version": 2,
  "kind": "memory",
  "state": "held",
  "tip": false,
  "retirement": false,
  "replaces": { "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "version": 1 },
  "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "workarea_path": "universe.procurement.metals-crew",
  "tokens": 30,
  "created_at": "2026-10-29T15:40:02Z",
  "promoted_at": null,
  "text": "Supplier Accra Metals revised the quote to 408 USD/t for Q1-2027 delivery, valid until 2026-12-01. Quote ref AM-7741-r2.",
  "derives_from": ["art_01JB2XF2R7M9W4K6T8Y1QZ3VH5"],
  "attachments": []
}
```

Superseded versions, and every version of a retired claim, stay readable as
history (`state: promoted`, `tip: false`). `derives_from` names only the
ancestors this key can read, and `replaces` is null when the key cannot read
the replaced version: a promoted version derived from its author's held row
names that row in neither field. The lineage read on the HTTP API does name
it, as a redacted bare id, so a reader learns that it exists and its id,
never its content ([FR-31](../product/roadmap.md)).

**Refusals** — `access.not_found` for anything the key may not read, a held
row of another key included: unknown and unreadable are indistinguishable;
`lifecycle.erased` for an erased version this key could read before its
erasure, and `access.not_found` for any other erased version
([errors](errors.md)).

### `oml_supersede`

*Available since v0.2.0 · [releases](../product/releases.md)*

Propose a new version of a claim, or with `retirement: true` propose retiring
it. The new version is a submission like any other — **held on arrival**,
scanned, served to a reviewer with the version it replaces — and the old
version stays the served tip, in every reader's recall, until a non-author
ratifies the new one
([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).
Success here means *accepted for review*, never *in force*. (Rides
`POST /v1/memories/{artifact_id}/supersede` — the provider ring.)

**Who may call:** the supersede rule of
[roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order),
retirement included; no proposer is then its reviewer (R2).

**Input**

```json
{
  "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
  "text": "Supplier Accra Metals revised the quote to 408 USD/t for Q1-2027 delivery, valid until 2026-12-01. Quote ref AM-7741-r2.",
  "reason": "supplier issued a revised quote",
  "idempotency_key": "crew-procure/run-4417/step-3"
}
```

- `artifact_id` — the claim's current served tip; the new version lands in
  that claim's workarea. A held version cannot be superseded
  ([RS-17](../product/roadmap.md)), and a target of a vocabulary kind
  (`term`, `term_mapping`, `vocab_release`) is refused on this tool with
  `request.validation`; supersede it on the HTTP API.
- `retirement` — **optional**, default `false`. `true` proposes retiring the
  claim, and `text` then states why; what a retired claim serves is declared
  on [audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip),
  and the refusals on [the HTTP API](http-api.md#supersede-post-v1memoriesartifact_idsupersede).
- `kind`, `derives_from`, `reason` — **optional** pass-through to the
  supersede body; the target is in `derives_from` implicitly, and every named
  id must be readable by the key. Omitted, `kind` is the target's; which kind
  changes a successor may make is declared on
  [audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).
  On this tool `kind` is limited to `memory`, `derived_fact` and `steering`,
  and the tool carries none of the vocabulary fields, which is why a
  vocabulary target is refused.
- The tool takes no `attachments` or `terms`, and the successor inherits
  neither from the target: only `kind` carries over, so a supersession made
  here leaves the new version with no attachments and no CURIE annotations
  ([http-api](http-api.md#supersede-post-v1memoriesartifact_idsupersede)).
- `simulation_id` / `simulated_time` — **optional** pass-through, exactly as
  on [`oml_learn`](#oml_learn): required when the target or `derives_from`
  touches a simulation row (`submission.simulation_id_required` otherwise),
  and honored only for keys in that simulation's audience. A tainted
  successor, once ratified, becomes the audience's overlay tip and never the
  default tip
  ([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).
- `idempotency_key` — scoped to the calling key alone, whatever the target's
  workarea; a retry returns the stored response and writes once, even after
  the target stops being readable (D13,
  [http-api](http-api.md#idempotency)).

**Output**

```json
{
  "artifact_id": "art_01JB2YN4P6R8TA1CW3EY5G7J9L",
  "claim_key": "supplier-quote-am-7741",
  "version": 2,
  "state": "held",
  "retirement": false,
  "replaces": { "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "version": 1 },
  "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "idempotent_replay": false
}
```

**Refusals** — `access.author_only` when a writer key targets a version
another key authored — including a claim the key started whose tip another
key's ratified version now holds; `access.not_found` for a target the key
cannot read; `submission.tip_conflict` when the target is not the current tip, with
`details.current_tip` when the key can read it — one hop to retry against;
`workarea.frozen` on a `validated` workarea; `request.validation` for a
`kind` that differs from the target's with `retirement: true`, a `term` on
retirement, or a target of a vocabulary kind; `submission.simulation_id_required`
when the target or `derives_from` touches a simulation row and no
`simulation_id` is sent. Every refusal that depends on
the target comes only after the target is known to be readable, in the order
of the supersede rule. Example:

```json
{
  "error": {
    "code": "submission.tip_conflict",
    "rule": "supersession targets the current served tip; the tip moves only at ratification",
    "message": "art_01JB2XF2R7M9… is no longer the tip of supplier-quote-am-7741",
    "details": { "current_tip": { "artifact_id": "art_01JB2Z3E5G7J9L2N4Q6S8UWA1C", "version": 3 } },
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_my_work`

*Available since v0.2.0 · [releases](../product/releases.md)*

The caller's own submissions and where each stands: held, promoted, demoted,
rejected, erased. It is the one view of its held rows a key has without a
reader grant — held content is invisible to everyone but its author and its
reviewers until ratified (R3) — and the honest answer to "did my memory make
it". (Rides the own-work listing, `GET /v1/memories?own=true` — the provider
ring.)

**Who may call:** any reader, writer, dreamer or admin key; the result is
scoped to the calling key's own submissions by construction. Operator keys
are refused — the operator never writes, reads or reviews content.

**Input**

```json
{
  "workarea": "metals-crew",
  "state": "held",
  "limit": 20
}
```

Every filter is optional. `workarea` is a subtree filter, by id or name; omitted, the listing
covers every workarea the key wrote to, minus its globally denied subtrees.
It must name a node the key holds some role on, else `access.not_found`,
indistinguishable from a node that does not exist
([http-api.md](http-api.md#search-get-v1memories)).
`claim_key` lists the versions of one claim. Omit them all for everything the
key wrote, newest first ([http-api.md](http-api.md#search-get-v1memories)).
`limit` is at most 200, as on the ring.

**Output**

```json
{
  "items": [
    {
      "artifact_id": "art_01JB2XJH8K1M3P5R7T9W2YA4CE",
      "claim_key": "accra-metals-price-anchor",
      "version": 1,
      "kind": "derived_fact",
      "state": "held",
      "tip": false,
      "retirement": false,
      "replaces": null,
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
      "workarea_path": "universe.procurement.metals-crew",
      "tokens": 36,
      "created_at": "2026-10-29T16:02:51Z",
      "promoted_at": null,
      "held_expires_at": "2026-11-05T16:02:51Z"
    },
    {
      "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
      "claim_key": "supplier-quote-am-7741",
      "version": 1,
      "kind": "memory",
      "state": "promoted",
      "tip": true,
      "retirement": false,
      "replaces": null,
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286",
      "workarea_path": "universe.procurement.metals-crew",
      "tokens": 26,
      "created_at": "2026-10-27T13:58:20Z",
      "promoted_at": "2026-10-27T14:11:08Z",
      "held_expires_at": null
    }
  ],
  "next_cursor": null
}
```

Items are [claim items](#the-claim-item) without `text`. `held_expires_at`
reflects the workarea's review policy object (held-item expiry,
[admission](../architecture/admission.md#review-policy-objects)): the gate
fails closed, so an item nobody reviews in time does not silently promote. It
comes from the ring's memory summary, never from the facade (ADR-0014 d1), and
a build without held-item expiry answers null. The tool returns no reviewer identity and no verdict reasoning —
the decision trail belongs to the evidence export, an admin surface.

**Refusals** — an operator key:

```json
{
  "error": {
    "code": "access.verb_ceiling",
    "rule": "operator never writes, reads or reviews content (D9)",
    "message": "operator keys have no own-work view",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_workarea_info`

*Available since v0.2.0 · [releases](../product/releases.md)*

One workarea as the calling key is allowed to see it: identity, derived path,
status, crew record, immediate structure, and — the part an agent actually
needs before writing — the verbs this key's effective access carries there.
(Rides `GET /v2/workareas/{workarea}`, one call — the lab ring.)

**Who may call:** keys with reader effective access on the node.

**Input**

```json
{ "workarea": "metals-crew" }
```

`workarea` is **optional**: omitted, the facade takes the key's journalled
default workarea from the authenticated key record (ADR-0022 d4) and makes the
same one call; a key with no default is refused with
`request.workarea_unresolved`.

**Output**

```json
{
  "id": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "name": "metals-crew",
  "path": "universe.procurement.metals-crew",
  "status": "committed",
  "crew": {
    "crew_id": "d10bcbe9-032d-4080-aa0c-cc06f8acd89f",
    "name": "metals-crew",
    "charter_uri": "https://crews.example.com/metals/charter.md",
    "external_manager": "crewd.example.com"
  },
  "parent": "1351b2b6-5c7c-492d-9aff-630bc95035db",
  "children": [
    { "id": "a4497daf-cfc8-4ddc-a0e5-4817efdb56c8", "path_leaf": "q1-2027", "status": "development" }
  ],
  "edges": [
    {
      "edge_id": "we_01JB2Y6T8V1X3Z5B7D9FH2KM4P",
      "from": "fbbb41c5-9630-4274-ae53-166f562f5286",
      "to": "7724e9d7-ddd6-4b94-8855-aad97c28224c",
      "label": "continues",
      "state": "ratified"
    }
  ],
  "my_effective_verbs": ["recall", "read", "graph_read", "events_tail", "remember", "learn", "attach", "supersede"],
  "permission_epoch": 815
}
```

`status` tells the agent what the node will refuse before it tries:
`development` may run relaxed, marked self-signed review by tenant policy;
`committed` requires full non-author review; `validated` refuses new writes
entirely ([the status ladder](../architecture/workareas.md#the-status-ladder-d8)).
The crew record is recorded, not operated — OML stores it and journals its
changes; the crew is managed elsewhere. It is the record of the crew the
workarea names, or null, and the only crew read on this surface
([crews](../architecture/workareas.md#crews-recorded-not-operated)). `my_effective_verbs` is a derived
answer — the union, over the grant roles effective on the node, of the
[effective-verbs table](../architecture/roles-and-keys.md#effective-verbs) —
reproducible from journalled grants at the stated permission epoch
([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)).
It states the access conferred, not what this server build serves;
`tools/list` states that. Children are ordered by `path_leaf` ascending in
byte order, then `id`; edges, in either direction, by `edge_id` ascending,
and an edge's `edge_id` is the one [`oml_events_tail`](#oml_events_tail)
reports. Children and edges outside the key's read scope are absent, not
marked.

**Refusals** — a node outside read scope, same `access.not_found` error as
[`oml_recall`](#oml_recall), fail closed.

### `oml_events_tail`

*Available since v0.1.0; filtered per key since v0.2.0 · [releases](../product/releases.md)*

Poll the journalled event stream for the key's scope, from the last journal
sequence the client processed. This is the stateless substitute for push: an
agent that wants to know "what happened since I last looked" calls with its
stored `after_seq` and gets the next page of events plus the current
`head_seq`. The server remembers nothing about the caller between calls;
losing the stored sequence means re-reading, never a gap the server must
repair. (Rides `GET /v2/events` — the lab ring.)

**Who may call:** keys with reader effective access; `scope` is optional
(omitted, the key's journalled default workarea — ADR-0022 d4 — and a key with
none is refused with `request.workarea_unresolved`), and the tail is filtered
to the events the key may see under the per-family visibility rule declared on
[http-api.md](http-api.md#events-tail). R3 applies to the stream exactly as to
reads: another writer's held submission never appears here — a key sees only
its own `submission.*` and `simulation.write` events — and no review or key-administration event
appears at all, so the tail cannot be used to watch the review queue from
outside.

**Input**

```json
{
  "scope": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "after_seq": 418,
  "limit": 50
}
```

Omit `after_seq` to start from the beginning (0), as on the ring; an agent
that wants to start from now passes the `head_seq` of a first call. Event
types and payload shapes are owned by the
[event catalog](../data/event-catalog.md).

**Output**

```json
{
  "events": [
    {
      "seq": 419,
      "event_type": "lifecycle.promote",
      "at": "2026-10-29T16:40:12Z",
      "subject": { "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "claim_key": "supplier-quote-am-7741", "version": 1 },
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286"
    },
    {
      "seq": 423,
      "event_type": "workarea.relate_ratified",
      "at": "2026-10-29T16:52:30Z",
      "subject": { "edge_id": "we_01JB2Y6T8V1X3Z5B7D9FH2KM4P", "from": "fbbb41c5-9630-4274-ae53-166f562f5286", "to": "7724e9d7-ddd6-4b94-8855-aad97c28224c" },
      "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286"
    }
  ],
  "head_seq": 431,
  "next_after_seq": 431,
  "idle": true
}
```

`next_after_seq` is the resume point declared on
[http-api.md](http-api.md#events-tail); pass it back verbatim as `after_seq`.
`idle` is true when it has reached `head_seq`. Each event carries `subject` —
`{artifact_id, claim_key, version}` for `lifecycle.*`, `submission.*` and
`simulation.write` (for `lifecycle.supersede` the new version, for
`lifecycle.tip_override` the version that becomes the tip), `{edge_id, from,
to}` for `workarea.relate_*`, `{workarea_id}` for every other `workarea.*` —
and `workarea`, always present, naming the in-scope workarea: the subject's
workarea; for `workarea.relate_*`, the edge's `from` endpoint when it lies
within the scope's subtree, else its `to` endpoint; for `lifecycle.reassign`,
`to_workarea_id` when `re_enters_held` is false and that workarea satisfies a
clause that makes the event visible (the reader clause, with reader on it
within the scope's subtree, or the author clause, with it within the scope's
subtree minus global denials), else
`from_workarea_id`. Any node the
[naming rule](../architecture/roles-and-keys.md#own) withholds from the key is
null, so an author-only event on a version an admin moved where the key holds
nothing carries `workarea: null`. Hashes, signatures, payloads and every
identity are dropped. Sequence numbers are the per-tenant journal sequence (D1): gaps in a
filtered tail are normal — they are the events this key may not see — and
order is the chain's order, so two keys with equal access see the same
`lifecycle.*` and `workarea.*` events in the same order; each additionally
sees only its own `submission.*` and `simulation.write` events and, under the
author clause, the `lifecycle.*` events of its own versions. `seq` and `head_seq` count every
event in the tenant's journal, so the gaps tell a key how many events it
cannot see occurred, and when — a count, never content
([FR-31](../product/roadmap.md)). The cost of poll-over-push is stated above under
[transport](#transport-streamable-http-stateless): reaction latency is the
polling interval, by design in v1.

**Refusals** — scope outside read access (`access.not_found`); a malformed
`after_seq`, or one beyond `head_seq`, is a typed `request.validation`
refusal, never a silent restart from zero.

### `oml_attach`

*Not scheduled · [releases](../product/releases.md)*

Manage a **resumable upload session** for a file, image or PDF (D3). The
session is the unit a memory references: open it with the expected plaintext
SHA-256 and size, move the bytes, then hand the session id to
[`oml_remember`](#oml_remember). (Rides `/v1/attachments/*` — the provider
ring.)

The chunk bytes themselves do **not** travel over MCP. Tool messages are
JSON; base64 would inflate every chunk by a third and push attachment-sized
payloads through the agent's context. The session's parts are PUT to the
versioned HTTP API under the same bearer key; this tool opens and inspects
sessions, nothing more. That split is the accepted cost of keeping the MCP
surface small; an agent whose harness cannot issue a plain HTTP PUT cannot
attach files through this plugin.

**Under a connection.** A connection has no key value, and a token is a
credential only at `/mcp`: it is never accepted on the parts route. Under a
connection, `open` is therefore refused before any session is created, so no
session counts against the key's upload quota, with `oauth.attach_refused`
([errors](errors.md#oauth-connections),
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d11);
`status` answers as it would to the key, which has no session to show. No
file's bytes travel through an upload session under a connection; ADR-0035
(proposed, in pull request #17) gives a connection small text files inline on
`oml_remember` instead.

**Who may call:** keys with writer effective access (the session binds to a
workarea only when a submission references it).

**Input** — open a session:

```json
{
  "action": "open",
  "media_type": "application/pdf",
  "size_bytes": 184233,
  "expected_digest": "sha256:9f2c41aa7d0e8b64c3f2a19e5d87b4610c3e9f2a84d7b1c6e0f5a382947d1c5b"
}
```

**Output**

```json
{
  "upload_session": "ups_01JB2XD9W6E4T2Y8U1O3PA5SKF",
  "state": "open",
  "part_size_bytes": 8388608,
  "parts_expected": 1,
  "upload_path": "/v1/attachments/uploads/ups_01JB2XD9W6E4T2Y8U1O3PA5SKF/parts/{part_number}",
  "expires_at": "2026-10-29T18:40:00Z"
}
```

**Input** — inspect after a crash or retry (`action: "status"` with the
session id) returns `state` (`open` | `complete` | `aborted` | `expired`),
the parts received, and the next part to send — this is what
"resumable" means: a retry resumes the recorded session rather than
improvising (D3). On the last part the server verifies the plaintext digest;
on match the session is `complete` and referenceable, the object is stored
content-addressed and encrypted per artifact
([storage](../architecture/storage.md)). A digest mismatch at completion
moves the session to `aborted` — terminal, open a new one; the refusal is
`upload.digest_mismatch`, and the mismatched bytes never bind to a claim.

Every attachment of a claim is served to its reviewer before a verdict
counts, and what review *means* per attachment type — extracted text where a
deterministic extractor exists, provenance + digest + scan result for opaque
binaries — is stated honestly in [admission](../architecture/admission.md).

**Refusals** — a key without writer effective access; a session quota or size
ceiling (typed 429/413 family, [errors.md](errors.md)); `open` under a
connection (`oauth.attach_refused`, above); referencing an incomplete or
mismatched session from `oml_remember`. Example:

```json
{
  "error": {
    "code": "upload.digest_mismatch",
    "rule": "a session records its expected digest; bytes that do not match never bind (D3)",
    "message": "session ups_01JB2XD9… received bytes with sha256 1c44…, expected 9f2c…; session is terminal, open a new one",
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

### `oml_graph`

*Not built; links between memories arrived in v0.5.2 on HTTP and the CLI · [releases](../product/releases.md)*

A scoped read of claims and their edges: the declared lineage
(`derives_from`), claim↔claim connection edges with their pending/ratified
state, and supersession chains within a subtree. The contract is
**serialize, don't compute**: the server returns the subgraph inside the
key's read scope; closure computation, path finding and ranking are the
client's problem. That keeps the server's work bounded and the output
auditable, at the cost of chattier clients on deep graphs. (Rides the lab
ring's designed graph reads — `GET /v2/edges` and
`GET /v2/memories/{id}/lineage`.)

**Who may call:** keys with reader effective access on the scope. `scope` is
optional: omitted, it is the key's default workarea (ADR-0022 d4). Reads are
logged like any read.

**Input**

```json
{
  "scope": "fbbb41c5-9630-4274-ae53-166f562f5286",
  "roots": ["art_01JB2XJH8K1M3P5R7T9W2YA4CE"],
  "depth": 2,
  "limit": 100
}
```

**Output**

```json
{
  "claims": [
    { "artifact_id": "art_01JB2XJH8K1M3P5R7T9W2YA4CE", "claim_key": "accra-metals-price-anchor", "version": 1, "kind": "derived_fact", "state": "promoted", "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286" },
    { "artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "claim_key": "supplier-quote-am-7741", "version": 1, "kind": "memory", "state": "promoted", "workarea": "fbbb41c5-9630-4274-ae53-166f562f5286" }
  ],
  "edges": [
    { "kind": "derives_from", "from": "art_01JB2XJH8K1M3P5R7T9W2YA4CE", "to": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "state": "declared" },
    { "kind": "connection", "label": "…", "from": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5", "to": "art_01JB2M4N6P8R1T3V5X7Z9BD2FH", "state": "pending", "edge_id": "ce_01JB2YA1C3E5G7J9L2N4Q6S8UW" }
  ],
  "truncated": false,
  "next_cursor": null
}
```

What the graph shows is the **declared** graph, and only that — undeclared
derivation is invisible to the service and therefore to this tool
([audit and lineage](../architecture/audit-and-lineage.md)). Nodes outside
the key's scope are absent; an edge whose far endpoint is unreadable appears
with the endpoint id only, body-less, so the shape is honest without leaking
content. The simulation predicate applies: simulation rows and their edges
are served only inside the audience.

**Refusals** — scope outside read access (`access.not_found`, fail closed);
a `depth`/`limit` above the served ceiling returns the typed limit error from
[errors.md](errors.md) rather than a truncated answer pretending to be whole.

---

## Wiring it into your agent

*Available since v0.1.0; setup through Claude Code's plugin settings since v0.4.0 · [releases](../product/releases.md)*

A generic MCP client configuration — the shape most harnesses accept, adjust
the key names to yours:

```json
{
  "mcpServers": {
    "oml": {
      "type": "http",
      "url": "https://oml.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${OML_API_KEY}"
      }
    }
  }
}
```

In Claude Code the `oml` plugin declares this entry for you, reading two
plugin settings and nothing from the environment
([ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md) d1, d2):

```json
{
  "mcpServers": {
    "oml": {
      "type": "http",
      "url": "${user_config.url}/mcp",
      "headers": {
        "Authorization": "Bearer ${user_config.api_key}"
      }
    }
  }
}
```

`url` is the server without `/mcp` and `api_key` the agent's key, marked
`sensitive`, so Claude Code keeps it in the system keychain and never in
`settings.json`. Since v0.4.1 `url` defaults to the maintainer's public server,
`https://memory.doublegate.io`, and is not required, and `api_key` has no default
([ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) d2).
`/plugin install` asks for both in a
masked dialog and `/plugin configure oml@open-memory-lab` changes them; from a
shell, Claude Code 2.1.285 or later reads them from the key file through
`claude plugin configure --values-stdin`, as the
[plugin README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md#install)
shows (ADR-0040 d4, d5). A v0.3.x install that only exported `OML_URL` and
`OML_API_KEY` serves no tools until the plugin is configured once.
`scripts/check_plugin.py` fails when the entry or the two options differ from
this shape (ADR-0040 d6, ADR-0043 d8).

### Claude on the web, on the desktop and on a phone

Claude's hosted apps — claude.ai, Claude Desktop, and the Claude iOS and Android
apps — cannot carry the header above, so they connect through a connection
instead (since v0.4.0, no live connection yet: [FR-34](../product/roadmap.md)).
The deployment must set `oauth.issuer` to its public origin, and its reverse
proxy must pass the discovery and OAuth paths through
([deployment](../operations/deployment.md#connections-behind-a-reverse-proxy)).

1. An admin issues the agent's connector key and its connect code in one call,
   one principal per agent as always:

    ```console
    $ oml keys issue-agent claude-mobile --principal crew/claude-mobile --workarea <workarea> --connect-code
    https://oml.example.com/mcp
    7KQ2-M9XD-4TPA
    ```

    `claude-mobile` is the key's [name](http-api.md#names-and-ids). The first
    line is the connector URL, the second the code: single use, alive 10 minutes
    by default, shown this once
    ([CLI](../cli/reference.md#oml-keys-issuance-revocation-rotation)).

2. On claude.ai, at Customize > Connectors, add a custom connector with exactly
   that URL — no trailing slash. Custom connectors are offered on the Free plan
   (one connector), Pro, Max, Ultra, Team and Enterprise
   ([Anthropic Help Center](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp),
   fetched 2026-10-31).
3. Claude opens OML's connect page; enter the code there and confirm. What
   the page says, word for word, is declared on the
   [HTTP API](http-api.md#connections-oauth-for-hosted-mcp-clients).
4. The connector is then usable from every Claude app signed in to that account,
   the mobile apps included. claude.ai holds only tokens bound to that one key;
   no key value exists to paste. It opens no upload session: the designed
   `oml_attach` `open` is refused under a connection ([`oml_attach`](#oml_attach)).

`oml keys disconnect claude-mobile <connection-id>` ends the connection, and
revoking the key ends it too. A code that lapsed before use is replaced with
`oml keys connect-code claude-mobile`. Claude Code keeps the header
configuration above.

### Operational notes

- **One key per agent.** Issue each crew member its own key
  ([key issuance](../architecture/roles-and-keys.md#key-lifecycle), via the
  [CLI](../cli/reference.md)); attribution, review independence and the
  collusion statistics depend on it. `oml keys issue-agent <key-name>
  --principal <principal> --workarea <workarea>` issues the one key this
  plugin needs, under the key's [name](http-api.md#names-and-ids)
  ([ADR-0026](../adr/ADR-0026-names-are-keys.md) d12)
  ([CLI](../cli/reference.md#oml-keys-issuance-revocation-rotation)), and the
  [`oml-admin` plugin](../cli/reference.md#the-oml-admin-plugin) runs it for an
  operator with `--out`, so the value lands in an owner-only file and never in
  a transcript, the file the plugin's shell configuration reads, and prints
  that configuration's `claude plugin configure` line (ADR-0040 d4); this plugin
  cannot issue keys, since key issuance is an admin verb (R1). Never put the
  key in a config file you commit.
- **One key does the agent's whole loop.** An agent that writes and recalls
  holds one writer-role key with a writer grant and a reader grant on its
  workarea, and that workarea as its default
  ([grant admissibility](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)):
  it writes, recalls with its own held rows, reads back, supersedes,
  proposes retirement and lists its own work, with the state that says what
  became of each write, through the one `oml` entry above. A recall-only agent gets a reader key; a proposing
  agent, dreamer(propose). A reviewing crew member does not use this plugin
  for its verdicts — verdicts live on the [HTTP API](http-api.md) behind the
  serve ceremony.
- **Retry with intent.** Put a stable `idempotency_key` on every
  [`oml_write`](#oml_write) your harness might retry; retries then cost
  nothing and duplicate nothing.
- **Allow the reads.** A plugin cannot allow its own tools; allow
  `mcp__plugin_oml_oml__oml_read` and `mcp__plugin_oml_oml__oml_workarea` in
  `permissions.allow` to stop the question on reads and keep it on every
  write, or `mcp__plugin_oml_oml` to allow all three (amends
  [ADR-0029](../adr/ADR-0029-session-start-hook.md) d8).
- **Expect `held`.** A successful write is a claim under test. List the
  key's own work with [`oml_read`](#oml_read) and `state` if the agent's plan
  depends on its memory becoming visible to others.
- **Start each session from memory.** In Claude Code the `oml` plugin's
  SessionStart hook runs `oml hook session-start` when the CLI is on `PATH`,
  which injects the workarea's steering, the key's recent submissions and the
  memories matching the working directory's name, under a token budget; it
  reads the plugin's two settings first and `OML_URL` and `OML_API_KEY`
  second, as one pair, never one value from each: either setting set and
  non-empty makes the settings the pair, one of them alone prints nothing
  and names the other, an empty value counts as unset, and the CLI's stored
  credentials are never read (ADR-0040 d3); it is
  declared, with its variables and how it fails open, on the
  [CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin)
  ([ADR-0029](../adr/ADR-0029-session-start-hook.md)).
- **Recall after a failed command, when opted in.** With
  `OML_FAILURE_RECALL=1`, the plugin's PostToolUseFailure hook runs
  `oml hook tool-failure` on a failed Bash command and injects at most three
  memories matching the error's distinctive words, declared on the
  [CLI reference](../cli/reference.md#oml-hook-tool-failure-recall-after-a-failed-command)
  ([ADR-0038](../adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md)).
- Liveness for your own infrastructure: `oml-server` exposes `/healthz` and
  `/readyz` ([deployment](../operations/deployment.md)); the MCP endpoint
  itself has no separate probe.

## Decision record

This surface is owned by ADR-0014 ([decision register](../adr/index.md)): one
versioned JSON HTTP API with MCP as a stateless streamable-HTTP facade, and
idempotency records. The two-ring split each tool rides, and the
key-derived target workarea that made the `workarea` arguments optional,
are [ADR-0022](../adr/ADR-0022-two-ring-api.md)'s. `oml_get`,
`oml_supersede`, the claim item, own held rows and budgets on recall, the
own-work and workarea reads, and the events-tail visibility rule are
the proposed [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md)'s,
which would amend ADR-0014 d2. Workarea arguments that take a name as well as
an id, and the named crew record the workarea read serves, are the proposed
[ADR-0026](../adr/ADR-0026-names-are-keys.md)'s. The three-tool catalog, its
modes and annotations, the designed annex, the events tail leaving MCP, and
the rule that a new capability is a parameter, a resource or a hook first are
[ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md)'s
(d2 to d5, d8). The Claude Code plugin's two settings and the entry that
reads them alone are
[ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md)'s (d1, d2, d6). Authentication by a connection,
for Claude's hosted apps, is
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)'s, which
amends ADR-0014 d1, d2 and d4.
