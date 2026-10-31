<!--
id: api/http-api
tier: engineering
status: design draft for review · decided, not built
answers: what the two-ring JSON surface commits — /v1 provider and /v2 lab routes, authentication, idempotency records, the pagination convention, and the verb ceiling per endpoint; which events a key sees on the events tail; the /mcp challenge, the discovery documents, the OAuth endpoints and the authorization page of connections
-->

# HTTP API

**Status:** design draft for review · decided, not built

This page owns decision **D13** as contract: the two-ring JSON surface — the provider ring
(`/v1`) and the lab ring (`/v2`) — authentication, idempotency
records, the pagination convention, and the verb ceiling each endpoint enforces. The
machine-readable form is [openapi.yaml](openapi.yaml) — the two describe the same contract, and a
divergence between them is a documentation bug, not an ambiguity to interpret. Every refusal cited
below is a typed error declared in [errors.md](errors.md). The MCP agent plugin is a stateless
facade over the agent-safe subset of this API ([mcp-plugin.md](mcp-plugin.md)); the `oml` CLI
drives the administrative and review verbs over this same API
([cli/reference.md](../cli/reference.md)) — there is no second surface and no private RPC. The
one-transport posture is deliberate: a second surface would mean a second authentication path, a
second audit story, and a place for the two contracts to drift apart.

Field names and endpoint spellings are design identifiers: normative in shape, adjustable in
spelling at implementation without a design change (the same caveat the
[event catalog](../data/event-catalog.md) carries).

## Two rings

One keyed listener carries two **stability rings**
([ADR-0022](../adr/ADR-0022-two-ring-api.md)); this page remains the one contract document for
both, and `/healthz` and `/readyz` stay unversioned.

- **The provider ring (`/v1`)** is the memory provider foundation: submit (and the reserved
  `POST /v1/memories:batch`, [FR-12](../product/roadmap.md)), read own and promoted, recall,
  supersede, and the attachment upload/fetch flow. **The covenant:** once
  [FR-21](../product/roadmap.md) declares v1 stable, v1 changes are additive only — no field
  removed, no semantic changed, no status code repurposed; anything that would break a v1 client
  is, by definition, a v2 capability. A provider-ring request names no workarea: the target
  resolves from the key's journalled `default_workarea`, with the `X-OML-Workarea` header (a node
  id, honored only within the key's grants — plain-passable because it is authorization-checked
  data, never a secret) as the explicit override; a submit, recall or non-own listing from a key
  with neither answers `request.workarea_unresolved` ([errors.md](errors.md)). Get, the own-work
  listing (`own=true`) and supersede never answer it: get and `own=true` resolve no workarea, and
  supersede takes the target's workarea.
- **The lab ring (`/v2`)** is the evolving memory lab: review and ratification ceremonies, claim
  edges, workarea, key and claim administration, bans, simulations, exports, settings, tenants,
  bootstrap, the events tail and vocabularies. It prefers additive change but may re-version to
  `/v3` without ever touching `/v1`. Lab requests keep explicit workarea ids in paths and
  bodies — an administration surface names what it administers.

The split is a stability contract, not a trust boundary: the verb ceiling (R1), admission, RLS
and every guarantee in [commitments](../product/commitments.md) apply identically on both rings,
and a `/v1` write is held until review like any write — what `/v1` hides is the machinery, not
the constitution.

## Conventions

| Convention | Rule |
|---|---|
| Base path | Two rings on one listener ([Two rings](#two-rings)): `/v1` the provider ring (covenant-bound once FR-21 closes), `/v2` the lab ring (evolving; may re-version to `/v3` without touching `/v1`). Additive fields are not breaking, and clients must ignore unknown response fields. |
| Content type | `application/json; charset=utf-8`, except attachment part upload (`application/octet-stream`) and the export bundle download. A request body in any other media type answers `415 request.unsupported_media` ([errors.md](errors.md)). Responses are `application/json`; `Accept` is not negotiated in v1. The OAuth endpoints of [connections](#connections-oauth-for-hosted-mcp-clients) are outside both rings and take their RFCs' media types: the token request and the page's form are `application/x-www-form-urlencoded`, and the authorization page is `text/html`. |
| Rate and quota | `429 limit.rate` / `limit.quota` may be answered by any operation; it is declared explicitly only on the v1 ring and high-traffic v2 surfaces ([errors.md](errors.md)). |
| Timestamps | RFC 3339, UTC, e.g. `2026-10-29T14:03:17Z`. |
| Digests | `sha256:<64 hex>`. An attachment is named by the SHA-256 of its plaintext ([storage.md](../architecture/storage.md)). |
| Identifiers | Opaque strings (`artifact_id`, `workarea_id`, `key_id`, `serve_id`, …). Clients never parse them. |
| Request id | Every response carries `X-Request-Id`; error bodies repeat it as `request_id`. |
| Deletion | No endpoint is `DELETE`. Every removal is a journalled `POST` act — demote keeps bytes, erase is crypto-shredding (constitution R4; [audit-and-lineage.md](../architecture/audit-and-lineage.md)). |

## Versioning and deprecation

The rings version independently ([Two rings](#two-rings)); this section fixes what *additive*
means, because the v1 covenant is only as strong as that definition.

**Additive** — permitted on a frozen ring without a version change:

- a new endpoint;
- a new **optional** request field (absent means the prior behavior, exactly);
- a new response field — clients must ignore unknown response fields
  ([Conventions](#conventions)), which is what makes this additive;
- a new error code under an existing HTTP status (the taxonomy is closed per release, and a
  client branches on codes it knows — an unknown code under a known status degrades to the
  status's generic handling).

**Not additive for v1, settled exactly this way: extending a response enum.** A client that
matches exhaustively on `state`, `verdict` or any other enumerated response field breaks on a
member it has never seen, so a new enum member is a breaking change on `/v1` — it ships as a
new field, or as a `/v2` capability, never as a quiet widening. Request-side enums may gain
members (a client that never sends the new value never sees a difference).

**The `/v2` evolution process.** The lab ring prefers additive change under the same
definition. Every surface change, additive or breaking, is announced in the release changelog
(no release exists yet — the process binds from the first one). A breaking `/v2` change mints
`/v3`; the retiring ring keeps being served alongside its successor for a stated minimum
overlap window of **one minor release cycle**, and the overlap is announced with the change.
Deprecated endpoints will carry `Deprecation` and `Sunset` headers once implemented — decided,
not built, like everything on this page.

**Freezing v1.** [FR-21](../product/roadmap.md) is the freeze act: a recorded v1-stable
declaration pinning the provider ring at a named revision of [openapi.yaml](openapi.yaml)
(currently `0.3.0-design`), from which moment the covenant — and this section's definition of
additive — is in force.

## Authentication and authorization

Every request on either ring carries `Authorization: Bearer <api-key>`. `/mcp` takes the same
header with either a key value or, where the deployment sets `oauth.issuer`, a connection's
access token, which resolves to its key ([connections](#connections-oauth-for-hosted-mcp-clients);
[roles and keys](../architecture/roles-and-keys.md#connections)). On `/v1` and `/v2` an access
token is not a credential and answers `auth.invalid` like any non-key. Only `/healthz`, `/readyz`,
the two discovery documents and the OAuth endpoints are unauthenticated.

- **Tenant is derived server-side from the key, never from the request** (D6;
  [security.md](../architecture/security.md)). No endpoint takes a tenant parameter, with one
  exception: an operator key may name `tenant_id` when creating a tenant's first admin key —
  operator keys are provider-level and have no tenant of their own.
- **Roles do not nest.** A key is issued with one role — `reader`, `writer`, `dreamer`, `admin`,
  `operator` — which names the grant roles it may carry (the
  [admissibility table](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order));
  each grant confers a role over one workarea subtree
  ([roles-and-keys.md](../architecture/roles-and-keys.md)). Where an endpoint
  below says *requires reader*, it means reader **effective access** on the workarea the request
  names, resolved from grants and denies at call time ([workareas.md](../architecture/workareas.md));
  where it says *requires admin* or *requires operator*, it means the key's role itself, because
  the verb governs tenant- or provider-level objects.
- **The verb ceiling is structural** (constitution R1): no agent-facing role reaches promote, sign,
  demote, delete, or any admin verb, and the refusal is the typed error
  [`access.verb_ceiling`](errors.md) naming the rule. The ceiling per endpoint is stated in
  the [surface table](#the-surface) and repeated per section.
- **Reads fail closed as absence.** A resource outside the key's effective read scope — including
  every simulation row for a key outside that simulation's audience (D10) — answers
  `access.not_found`, indistinguishable from a resource that does not exist. A resource the key can
  see but lacks the verb for answers `access.grant_refused`.
- **One refusal order.** Every workarea-scoped verb resolves its refusal in the fixed four-step
  order declared on
  [roles-and-keys.md](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order),
  which also names the role each verb needs; this page does not restate it.
- **Every read is logged** (D12): key, role, workarea, the memory ids served, timestamp, and for a
  recall the canonical request digest — in the retention-bounded read log whose batch digests
  anchor into the journal ([retrieval.md](../architecture/retrieval.md#the-read-log)).

Bearer keys are the shipped mechanism on every route; `/mcp` also accepts a connection's access
token, decided, not built ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md),
[RS-2](../product/roadmap.md)). The role vocabulary and key authorization semantics are owned by
[roles-and-keys.md](../architecture/roles-and-keys.md).

## Idempotency

Every state-creating `POST` requires an `Idempotency-Key` header (missing →
`request.idempotency_key_required`). Read-shaped `POST`s — `recall`, `settings/validate` — do not
take one, and attachment part `PUT`s need none: a part is addressed by `(session, part_number)`
and verified by digest at completion, so replaying a part is harmless.

The semantics are D13's **idempotency record**: `(key_id, workarea, idem_key) → request digest +
state + stored response`, claimed in its own transaction **before any side effect** (the row lives
in Postgres; [postgres-schema.md](../data/postgres-schema.md)). On the provider ring the
workarea component is the **resolved** target — the key's `default_workarea` or the
`X-OML-Workarea` override (ADR-0022 d4), never a body field. For supersede, and for lab
endpoints that name no workarea (keys, bans, simulations, exports, settings, tenants), the
workarea component of the scope is a fixed nil value, so no supersede target's workarea ever
enters a record ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d10).

A supersede record is looked up after step 1 of the
[supersede rule](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) and
before step 2, and the replay table below answers from it whatever the target's state or the
key's access now: a retry of a committed supersession replays its stored response even after
the target was erased or reassigned, or the key lost its reader grant, because that response
names only the caller's own successor and discloses nothing new. With no record, step 2 runs,
and the record is claimed only after it passes, so a refusal at step 1 or 2 claims no record.

The request digest is SHA-256 over the RFC 8785 JSON of `{method, route, path_params, body}`:
the HTTP method, the route template (`/v1/memories/{artifact_id}/supersede`, say), the path
parameters by name, and the parsed request body (`null` when there is none). Two requests to
different targets therefore never share a digest, whatever their bodies.

```mermaid
sequenceDiagram
    participant C as client
    participant S as oml-server
    C->>S: POST /v1/memories (Idempotency-Key: k1)
    S->>S: claim record (key, workarea, k1) = in_progress, digest(request)
    S->>S: side effects: scan, quarantine, journal
    S->>S: record := committed + stored response
    S-->>C: 202 (response stored)
    C->>S: retry, same k1, same body
    S-->>C: 202 stored response, Idempotency-Replay: true
    C->>S: same k1, different body
    S-->>C: 409 idempotency.digest_mismatch
```

| Case | Answer |
|---|---|
| First request | Processed; response stored against the record. |
| Replay, same request digest, record `committed` | The stored response, byte for byte, with header `Idempotency-Replay: true`. A replayed submit does not create a second version. |
| Replay while the record is `in_progress` | `409 idempotency.in_progress`, retryable, with `Retry-After`. The first attempt is still running — crashed attempts leave `in_progress` records that expire. |
| Same key, different request digest | `409 idempotency.digest_mismatch`. The key names exactly one request; reusing it for another is refused, never silently honored. |

**A connect-code response is stored without the code.** The idempotency record of
`POST /v2/keys` with `connect_code: true` and of `POST /v2/keys/{key_id}/connect-codes` stores its
response without the code, so no idempotency record holds a connect code. A replay returns that body
with `Idempotency-Replay: true`; the first answer is the only one that carries the code, and a
caller that lost it mints a new one
([CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation);
[connections](../architecture/roles-and-keys.md#connections)). This is the one exception to the
byte-for-byte replay above, decided, not built
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d7, amending ADR-0014 d3). A
bearer key's issuance or rotation replays byte for byte, value included
([FR-36](../product/roadmap.md)).

A `committed` record is retained for at least **24 hours** — a committed floor, not an
estimate. The `idempotency.retention` setting
([configuration](../operations/configuration.md)) may only **raise** that floor, never lower
it. When retention expires the record is gone, and a later retry with the same key is
processed as a fresh request — expiry-then-reprocess is documented behavior, not an anomaly.
A client whose retry horizon can exceed the deployment's retention must treat a retry past it
as possibly-second work; retry loops belong inside the window.

## Pagination

List endpoints share one convention:

- Request: `limit` (1–200, default 50 — the ceiling is the `api.page_limit_max` setting,
  [Limits](#limits)) and `cursor` (opaque, from the previous page; absent for the first page).
- Response: `{"items": [...], "next_cursor": "..."}` — `next_cursor` is `null` when the listing is
  exhausted. Cursors are scoped to the key that obtained them and expire; a stale cursor answers
  `request.validation`.
- **Ordering** is `created_at` **descending** — newest first — on every list endpoint, so the
  first page is the most recent work. The one exception is below.
- **Cursors are positional, not snapshots.** A cursor marks a position in the live ordering;
  it freezes nothing. An item promoted, demoted or reassigned between pages may appear on a
  later page or vanish from one, and two walks of the same listing can disagree about rows that
  changed in between. A consumer that needs a consistent, replayable view reads the
  [events tail](#events-tail) instead — that is what the journal is for.
- Cursor lifetime is the `api.cursor_ttl` setting
  ([configuration](../operations/configuration.md)); an expired cursor answers
  `request.validation`, never a silent restart from page one.

One deliberate exception: the [events tail](#events-tail) paginates by `after_seq` and is
ordered `seq` **ascending**, because the per-tenant journal sequence is meaningful to clients
and is the resume point a consumer stores.

## The surface

Grouped by ring ([Two rings](#two-rings)). **The provider ring (`/v1`)** — submit, recall, the
listing without `own` and supersede take the `X-OML-Workarea` override header, which supersede
accepts only when it names the target's workarea; get and the own-work listing (`own=true`)
refuse it with `request.validation`. Get, the own-work listing and supersede never answer
`request.workarea_unresolved`: get and `own=true` resolve no workarea, and supersede takes the
target's workarea. The reserved `POST /v1/memories:batch` ([FR-12](../product/roadmap.md)) will
join this table:

| Endpoint | Verb ceiling (effective access unless noted) |
|---|---|
| `POST /v1/memories` | writer — and every `derives_from` ancestor readable by the key; lineage is attribution, not a connection proposal |
| `POST /v1/memories/recall` | reader |
| `GET /v1/memories` | reader; `own=true` admits any tenant role, own rows only |
| `GET /v1/memories/{artifact_id}` | reader; the calling key fetches its own rows in any state |
| `POST /v1/memories/{artifact_id}/supersede` | writer (own claims) or dreamer(propose); includes retirement |
| `POST /v1/attachments/uploads` | writer |
| `PUT /v1/attachments/uploads/{session_id}/parts/{part_number}` | writer (session owner) |
| `POST /v1/attachments/uploads/{session_id}/complete` | writer (session owner) |
| `GET /v1/attachments/{digest}` | reader; reviewer under a valid `serve_id`; the author for own held |

**The lab ring (`/v2`):**

| Endpoint | Verb ceiling (effective access unless noted) |
|---|---|
| `GET /v2/memories/{artifact_id}/lineage` | reader |
| `GET /v2/claim-edges` | reader |
| `POST /v2/claim-edges` | dreamer(propose) |
| `POST /v2/claim-edges/{edge_id}/ratify` | dreamer(review); never the proposer (R2) |
| `GET /v2/workareas` · `GET /v2/workareas/{id}` · `/tree` · `/edges` · `/status` | reader |
| `POST /v2/workareas` | admin (key role) |
| `POST /v2/workareas/{id}/reparent` | admin (key role) |
| `POST /v2/workareas/{id}/crew` | admin (key role) |
| `POST /v2/workareas/{id}/edges` | dreamer(propose) on ≥1 endpoint, reader on both (edge-authority rule, [workareas.md](../architecture/workareas.md)) |
| `POST /v2/workarea-edges/{edge_id}/ratify` | dreamer(review) on both endpoints; never the proposer |
| `POST /v2/workareas/{id}/transitions` | dreamer or admin |
| `POST /v2/workareas/{id}/transitions/{transition_id}/ratify` | a second, non-proposing dreamer or admin |
| `GET /v2/events` | reader |
| `GET /v2/vocab/suggest` · `GET /v2/vocab/terms/{curie}` · `GET /v2/vocab/terms/{curie}/hierarchy` · `GET /v2/vocab/mappings` | reader |
| `GET /v2/review/queue` · `POST /v2/review/next` · `POST /v2/review/verdicts` · `POST /v2/review/reopen` | dreamer(review); refused per row by lineage-closure exclusion |
| `POST /v2/keys` and every key, grant, denial verb | admin (key role); operator only for a tenant's first admin key |
| `GET /v2/bans` · `POST /v2/bans` · `POST /v2/bans/{ban_id}/lift` | admin (key role) |
| `GET /v2/simulations` · `GET /v2/simulations/{id}` · `POST /v2/simulations` · `POST /v2/simulations/{id}/audience` · `POST /v2/simulations/{id}/end` | admin (key role) |
| `POST /v2/memories/{artifact_id}/reassign` · `/demote` · `/erase`, `POST /v2/claims/tip-override` | admin (key role) |
| `POST /v2/exports` · `GET /v2/exports` · `GET /v2/exports/{export_id}` · `/bundle` | admin (key role) |
| `GET /v2/settings` · `POST /v2/settings/validate` · `POST /v2/settings/apply` | admin (key role) |
| `POST /v2/tenants` | operator (key role) |
| `POST /v2/bootstrap/claim` | single-use enrollment token (bootstrap ceremony only) |

A `held_by: service` key is refused every review-class verb above — the review endpoints,
claim-edge ratify and workarea-edge ratify — and status-transition ratify, with
`access.verb_ceiling` before any lookup, whatever its grants
([roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)).

`GET /healthz` · `GET /readyz` are unversioned and unauthenticated ([Health](#health)).
`POST /mcp`, the [agent plugin](mcp-plugin.md)'s endpoint, is unversioned and authenticated by a
key or a connection's access token, under the verb ceiling of the key either resolves to. The
discovery documents and the OAuth endpoints are unversioned and unauthenticated
([Connections](#connections-oauth-for-hosted-mcp-clients)).

Promote, sign and delete appear nowhere in these tables on purpose: promotion is an effect of
ratification inside the admission pipeline, signing is `oml-server`'s alone, and deletion does not
exist (R1, R4).

## Memories

### Submit — `POST /v1/memories`

One endpoint for the two submission verbs; `verb` selects the journal event
(`submission.remember` / `submission.learn`, [event catalog](../data/event-catalog.md)). The
request names no workarea — the target resolves from the key
([Two rings](#two-rings), ADR-0022 d4): the key's journalled `default_workarea`, or the
`X-OML-Workarea` header as the explicit per-request override, honored only within the key's
grants. A key with neither is refused with `request.workarea_unresolved` (422).

```json
{
  "verb": "remember",
  "claim_key": "deploy-runbook",
  "kind": "memory",
  "body": "The canary stage must drain before …",
  "attachments": ["sha256:9c1e…"],
  "derives_from": [],
  "simulation_id": null,
  "simulated_time": null
}
```

- `verb: "learn"` asserts a derived claim: `derives_from` must be non-empty, and an empty or
  omitted `derives_from` is refused (`submission.lineage_required`) — on every surface that fronts
  this verb, the MCP plugin's `oml_learn` included. A derived conclusion whose author cannot or
  will not name lineage is submitted as `verb: "remember"` with `kind: derived_fact` (journalled
  `submission.remember`), where the honesty rule carries the caveat: an undeclared derivation is
  undetectable by the service — the lineage closure is as good as what writers declare
  (the constitution's honesty rule, [ADR-0001](../adr/ADR-0001-the-constitution.md), stated
  plainly on [admission.md](../architecture/admission.md)).
- Submitting with `derives_from` requires writer effective access on the target and no other
  role, even when the lineage names another author's claim — declaring an ancestor is
  attribution, not an edit of it. Every `derives_from` ancestor must still be readable by the
  key — promoted within its reader effective access, or its own row in any state — else
  `access.not_found`, indistinguishable from a nonexistent id
  ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d3).
  The declaration has review consequences, not authorization ones: every named ancestor expands
  the row's reviewer-exclusion set (R2 over the lineage closure), and a false lineage is grounds
  for rejection at review ([ADR-0012](../adr/ADR-0012-admission-and-review.md)).
- When a new claim's `claim_key` collides with an existing version of it
  (`submission.claim_key_conflict`) is declared in
  [audit-and-lineage.md](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).
- `kind` ∈ `memory` | `derived_fact` | `steering` | `term` | `term_mapping` | `vocab_release`. A
  steering row is held and reviewed like any claim and is never ranked at recall (D12). The three
  vocabulary kinds ride this same endpoint and pipeline — proposing a term, a mapping or a release
  pin is an ordinary submission into a vocabulary workarea
  ([Vocabularies](#vocabularies) below) — and a vocabulary workarea refuses
  `provenance: simulation` submissions of these kinds (deterministic scan-class veto): a demoted
  simulation row must never squat a permanent CURIE.
- Every attachment digest must name a **completed** upload session owned by this key, or the
  submit is refused (`submission.attachment_missing`).
- A `derives_from` that touches a simulation row forces `simulation_id` to that row's simulation
  (`submission.simulation_id_required` otherwise); writing with a `simulation_id` requires the key
  to be in that simulation's audience ([simulations.md](../architecture/simulations.md)).
- `universe` refuses direct claim writes (`submission.universe_refused`); a `validated` workarea
  refuses all new writes (`workarea.frozen`).
- The deterministic scan runs in the same call and can veto (`submission.scan_veto` — a refusal
  that journals nothing, reproducible from the journalled rule-set pin; nothing waits in the
  queue; [admission.md](../architecture/admission.md)). Banned authors and sources are refused at this
  same admission check (`access.banned`).

Success is `202 Accepted` — **held, visible to no one else until ratified** (R3):

```json
{
  "artifact_id": "art_01J…",
  "claim_key": "deploy-runbook",
  "version": 3,
  "state": "held",
  "plaintext_digest": "sha256:ab12…",
  "scan": {"result": "pass", "scanner_version": "…"},
  "journal_seq": 4182
}
```

### Recall — `POST /v1/memories/recall`

Ranked retrieval over **promoted** rows in scope: FTS + vector, served from Postgres, never from
the lake (D12; [retrieval.md](../architecture/retrieval.md)). The request names no workarea: the
default scope is the **key's default-workarea subtree** — the key's `default_workarea`, or the
`X-OML-Workarea` header as the override (within grants; neither present answers
`request.workarea_unresolved` — there is no tenant-wide fallback). Request: `query`, `k`,
optional `kinds` filter (hits only), `include_bodies`, and:

- `include_own_held` (default `false`) — the calling key's own held rows that match join the
  hits, ranked in the same order with `state: held`
  ([retrieval.md — own held rows in recall](../architecture/retrieval.md#own-held-rows-in-recall)).
- `max_tokens` — a budget in budget tokens; steering is never cut, and the hits served are the
  longest rank prefix that fits. Refused with `include_bodies: false`
  ([retrieval.md — token budgets](../architecture/retrieval.md#token-budgets)).

The body refuses unknown fields (`request.validation`), and so does an empty `kinds` or `terms`
array: an absent filter narrows nothing, and a sent one names at least one value. Two optional
vocabulary fields complete it; a server that does not yet serve them refuses them by name, never
ignores them:

- `terms` — CURIEs; filters to rows whose `terms` annotation cites any of them. A CURIE the key
  cannot read matches nothing (fail closed, as `derives_from` does).
- `expand_terms` ∈ `none` (default) | `descendants` — widens the `terms` filter before retrieval:
  each CURIE expands to its `is_a` descendants, resolved **per CURIE against the release pinned by
  the vocabulary that owns the expanded prefix** (a per-release closure lookup,
  [Vocabularies](#vocabularies)). Expansion changes the filter, never the ranking: steering order
  and `k` are untouched.

The `kinds` filter deliberately excludes the vocabulary kinds — term, mapping and release rows are
excluded from the shared recall indexes by default; where a vocabulary workarea's journalled
policy opts them in, they are hits of a recall that omits `kinds`
([retrieval.md](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked),
ADR-0021). Response separates what is ranked from what is not:

```json
{
  "steering": [ {"artifact_id": "…", "kind": "steering", "tokens": 120, "body": "…"} ],
  "hits": [ {"memory": {"artifact_id": "…", "claim_key": "…", "version": 4, "tip": true, "state": "promoted", "tokens": 300, "body": "…"}, "score": 0.83} ],
  "budget": {"max_tokens": 500, "counter": "oml-tc/1", "steering_tokens": 120, "used_tokens": 420,
             "truncated": true, "hits_omitted": 2, "next_hit_tokens": 250},
  "query_digest": "sha256:…"
}
```

Steering rows precede hits and carry no score — the recall-ordering contract: the gate changes
visibility, not ranking (R3/D12). Which steering applies, the total order of hits and the budget
cut are declared on [retrieval.md](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked);
every served row carries `tokens`, and `budget` is present if and only if `max_tokens` was sent.
The read is logged with the ids served after truncation and the canonical request digest
([retrieval.md](../architecture/retrieval.md#the-read-log)). Recall quality and latency are
unmeasured — [FR-4](../product/roadmap.md).

### Search — `GET /v1/memories`

Structured listing, paginated. Filters: `workarea_id` (a subtree filter), `claim_key`, `state`,
`kind`, `since`, `until`, `own`. Returns memory summaries (no bodies), each carrying `tokens`,
`retirement`, `replaces` and `held_expires_at`. With `own=true` the listing is the **own-work
status** read: any tenant role except operator lists every row the calling key authored
([own](../architecture/roles-and-keys.md#own)), in every state — held, promoted, demoted,
rejected — across all workareas minus its globally denied subtrees, with no grant needed; an
operator key answers `access.verb_ceiling`. It resolves no workarea, and an `X-OML-Workarea`
header sent with `own=true` is `request.validation`. An own row
on a node where the key holds no role carries `workarea_id` and `workarea_path` as null
([naming a node](../architecture/roles-and-keys.md#own)). Without `own`, scope defaults to the
key's default-workarea subtree, with `X-OML-Workarea` as the override, and the listing returns promoted
served tips within reader effective access only — a `self_signed` version only when the scope
node is its own workarea ([retrieval](../architecture/retrieval.md#the-rule-postgres-serves-the-lake-never-does)):
held rows belonging to others are never returned, whatever the filter says (R3).

`workarea_id` must name a node the key may be told about, else `access.not_found`,
indistinguishable from a node that does not exist, so the filter never confirms that a node
exists ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d13). With `own=true` that
is a node the key holds some role on, under the
[naming rule](../architecture/roles-and-keys.md#own), and the filter narrows the listing to the
key's rows in that node's subtree, whatever the key holds below it. Without `own` it is a node
where reader is effective, and the listing is the intersection of its subtree with the scope's:
the scope still resolves from the key, so a key with neither a default workarea nor the header
answers `request.workarea_unresolved` whatever `workarea_id` says.

A retired claim's served tip is its retirement version,
so the listing includes it, flagged `retirement: true`; recall, by contrast, serves nothing of a
retired claim. Ordering is `created_at` descending, then `artifact_id` descending; the cursor is
an opaque position bound to the calling key and expiring under `api.cursor_ttl`, and every page
re-applies the filters ([Pagination](#pagination)). On every listing, as on get and recall,
`derives_from` lists only the ancestors the caller can read ([Get](#get-get-v1memoriesartifact_id)).

### Get — `GET /v1/memories/{artifact_id}`

One memory with its body (decrypted on the serving path only), its `tokens` and attachment
descriptors. The calling key may fetch its own rows in any state, with no grant needed (global
denials still mask, [own](../architecture/roles-and-keys.md#own)); anyone else needs reader
effective access on the row's workarea and sees promoted versions only — superseded versions and
every version of a retired claim stay fetchable as history. Get takes the row's own workarea as
its implicit scope, so a non-own `self_signed` version is served to a key with reader effective
access there ([retrieval](../architecture/retrieval.md#the-rule-postgres-serves-the-lake-never-does)).
Anything else answers `access.not_found`. An erased artifact answers `410 lifecycle.erased` —
attribution and journal trail remain, bytes do not (D4) — but only to a key that could read the
version immediately before its erasure: its author's key set, or, when the version was promoted
then, a key with reader effective access on its workarea. Readability is checked first, and
every other key gets `access.not_found`, so the erasure of another author's held version tells
no one it existed ([errors](errors.md)). Get resolves no workarea, so an `X-OML-Workarea`
header sent with it is `request.validation`. An own row on a node where the key holds no role
carries `workarea_id` and `workarea_path` as null
([naming a node](../architecture/roles-and-keys.md#own)).

`derives_from` lists only the ancestors the caller can read — its own rows in any state, and
promoted rows within its reader effective access — on get, recall and every listing alike, and
`replaces` obeys the same readability test on every read: it is null when the caller cannot
read the replaced version, as after an admin moves that version into another workarea or back
into held ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d3). A promoted
descendant can derive from its author's held row, and its readers never learn that row's id
from either field. The [lineage read](#lineage-get-v2memoriesartifact_idlineage) does name it:
the full closure appears there, with every node outside the caller's read scope elided to a
redacted bare id, so a reader learns that an unreadable ancestor exists and its id, never its
content — a stated residual, [FR-31](../product/roadmap.md).

### Lineage — `GET /v2/memories/{artifact_id}/lineage`

The derivation closure over `derives_from` and claim edges, **pending and ratified both**,
serialized as stored — the server serializes the graph, it does not compute analytics over it
(D13: serialize, don't compute). Nodes outside the key's read scope are elided to bare ids with
`redacted: true`, so the shape of the closure is visible without leaking content. A redacted
node still discloses that the version exists and its id — another author's held or rejected
ancestor of a promoted version included — which is the residual
[FR-31](../product/roadmap.md) tracks.

### Supersede — `POST /v1/memories/{artifact_id}/supersede`

Proposes a replacement for the version `{artifact_id}` names: a new attributed submission of the
same `claim_key`, held like any write, whose `derives_from` includes the target and whose response
names it in `replaces`. Who may propose — a writer key on a target version it authored, a dreamer
key through edit-as-proposal (D9) — is the supersede rule declared on
[roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order), and
R2 keeps every proposer off the review. The target must be the current served tip, else
`submission.tip_conflict` with `details.current_tip` when the caller can read it; a held version
cannot be superseded ([RS-17](../product/roadmap.md)).

The tip moves only at ratification, by compare-and-set;
[audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)
owns the tip, version-ordinal, competing-successor, kind-change, retirement and retired-claim
rules. The scope is the target's workarea: an `X-OML-Workarea` that differs from it is
`request.validation`. Every refusal that depends on the target — that header, the tip, the kind
and retirement checks, `workarea.frozen` — is answered only after the target is known to be
readable, in the order step 5 of the supersede rule fixes, so none of them tells a caller that an
unreadable target exists. `Idempotency-Key` is required and honoured, scoped to the key with a
nil workarea; the record is looked up between steps 1 and 2 of that rule, so a committed
supersession always replays, and claimed only after step 2 passes
([Idempotency](#idempotency)). The body refuses unknown fields and carries `simulation_id`,
`simulated_time` and `mapping` as the submit body does. Of the target's fields only `kind` carries
over: an omitted `kind` makes the successor take the target's, and an omitted `attachments` or
`terms` leaves the successor with none — nothing else is inherited from the target.

**Retirement.** The same call with `retirement: true` proposes retiring the claim, open to the
same keys as any supersession ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md)
d11). `body` is required and states why; it is scanned, normalized, bounded and digested like
any body, and it is what the reviewer judges. Its refusals are [errors](errors.md)'
`request.validation` and `workarea.frozen` rows; what a retired claim serves is declared on
[audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip).

## Attachments

Resumable upload sessions, then content-addressed fetch (D3;
[storage.md](../architecture/storage.md)).

| Step | Endpoint | Notes |
|---|---|---|
| Create session | `POST /v1/attachments/uploads` | `{expected_digest, size_bytes, media_type}` → `201` session. The expected digest is recorded up front; the session is scoped to the uploading key and expires. |
| Upload part | `PUT /v1/attachments/uploads/{session_id}/parts/{part_number}` | Raw bytes, `application/octet-stream` → `204`. A retry re-puts the part; a lost session restarts (`upload.session_invalid` names which). |
| Complete | `POST /v1/attachments/uploads/{session_id}/complete` | Assembles, hashes, compares: `upload.digest_mismatch` when the bytes are not what the session promised; `200` with `{digest, ciphertext_digest, state: "complete"}` otherwise. |
| Fetch | `GET /v1/attachments/{digest}` | Bytes, decrypted on the serving path, plaintext digest verified before serving. Admits: reader effective access to a promoted claim binding the digest; the author of a held claim binding it; a reviewer under a valid `serve_id` query parameter. Logged as a read. |

A completed upload is inert until a submission binds its digest into a signed envelope; the claim
and its files are then one reviewed unit — a reviewer is served every attachment before a verdict
counts (D3).

### Fetch semantics

`GET /v1/attachments/{digest}` serves the whole object, once per fetch:

- `Content-Type` echoes the `media_type` recorded at session creation — the service stores it,
  it does not sniff bytes at serve time.
- `Content-Length` is the **plaintext** size: the served bytes are decrypted and digest-verified
  plaintext, so the length a client sees is `size_bytes` as declared at the session, never the
  ciphertext size.
- **`Range` requests are not supported in v1.** A `Range` header is ignored and the full object
  is served with `200`, never `206`. The cost is stated rather than hidden: an interrupted fetch
  restarts from byte zero, which at the attachment bound ([Limits](#limits)) means re-moving up
  to 64 MiB — tracked as [FR-29](../product/roadmap.md).

The export bundle download (`GET /v2/exports/{export_id}/bundle`,
[Evidence export](#evidence-export)) has the same fetch semantics: whole object, no
`Range`/`206`, `Content-Length` is the bundle size.

## Workareas

Reads require reader effective access on the node; nodes outside scope are absent
(`access.not_found`). The algebra — tree, universe, grants, denies — is owned by
[workareas.md](../architecture/workareas.md); this page only carries the verbs.

| Endpoint | Returns / does |
|---|---|
| `GET /v2/workareas?path=<ltree>` | Resolve a node by derived path. Grants bind to node ids, never paths; this is the lookup that bridges the two. |
| `GET /v2/workareas/{id}` | Node: id, parent, derived path, status, crew record — and, relative to the calling key, its readable immediate `children` (`[{id, path_leaf, status}]`, ordered by `path_leaf` ascending in byte order, then `id`), the `edges` touching it (`[{edge_id, from, to, label, state}]`, ordered by `edge_id` ascending), `my_effective_verbs` (the [effective-verbs table](../architecture/roles-and-keys.md#effective-verbs)) and `permission_epoch` ([workareas.md](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)), in one call. Children and edges outside read access are absent. |
| `GET /v2/workareas/{id}/tree` | The subtree, paginated; nodes outside read scope are omitted. |
| `GET /v2/workareas/{id}/edges` | Labelled `related` edges touching the node, `pending` and `ratified`, with proposer and ratifier. Edges confer zero access. |
| `GET /v2/workareas/{id}/status` | Status, the review policy object in force, pending transitions. |
| `POST /v2/workareas` | **admin.** Create a child in `development`: `{parent_id, name, crew?, prefix?}`. A `prefix` registers the node as a vocabulary workarea — tenant-unique and **permanent** (`vocab.prefix_conflict` on collision), and requires `crew` (a vocabulary needs a named curator crew, [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)). Refused under a `validated` parent (`workarea.frozen`). |
| `POST /v2/workareas/{id}/reparent` | **admin.** `{new_parent_id, dry_run}` — journalled single-transaction descendant path rewrite; `dry_run: true` returns the paths and grants affected without journalling. Re-parenting into a `validated` destination is refused (`workarea.frozen`). |
| `POST /v2/workareas/{id}/crew` | **admin.** Journalled crew-metadata update; allowed on `validated` areas — freeze covers claims and promotions, not metadata. |
| `POST /v2/workareas/{id}/edges` | Propose an edge `{to_workarea_id, label}`, label ∈ `relates_to` \| `continues` \| `derived_from` \| `mirrors`. Requires dreamer(propose) effective access on at least one endpoint and reader effective access on both (an admin who must act here holds a separate dreamer key — a key never combines admin with dreamer grants, `access.role_exclusive`). → `202`, pending. |
| `POST /v2/workarea-edges/{edge_id}/ratify` | Ratify under dreamer(review) effective access on **both** endpoints; the proposer is refused (`review.ratifier_is_proposer`, R2 extends to edges). |
| `POST /v2/workareas/{id}/transitions` | Propose a forward status transition `{to: "committed" \| "validated"}`. Backward does not exist; `validated` is refused while any child is not validated (`workarea.transition_refused`). → `202`, pending. |
| `POST /v2/workareas/{id}/transitions/{transition_id}/ratify` | A second, non-proposing dreamer or admin ratifies; journalled with the policy snapshot ([workareas.md](../architecture/workareas.md) transition matrix). |

## Claim edges

Claim↔claim connection proposals follow the same propose-then-ratify shape inside the admission
pipeline ([admission.md](../architecture/admission.md)).

- `GET /v2/claim-edges?workarea_id=…&artifact_id=…` — paginated graph read, pending and ratified,
  scoped to read access.
- `POST /v2/claim-edges` — `{from_artifact_id, to_artifact_id, label}` under dreamer(propose) →
  `202`, pending. The label vocabulary for claim edges is not fixed by the design (the closed
  label set exists for workarea edges only); fixing it is an open detail for the
  [roadmap](../product/roadmap.md).
- `POST /v2/claim-edges/{edge_id}/ratify` — dreamer(review), never the proposer, and refused when
  the ratifying key or principal appears in either endpoint's lineage closure
  (`review.author_in_lineage`).

## Vocabularies

The thin read surface over AI+Human vocabularies — terms, hierarchy, mappings and releases are
claims in vocabulary workareas, ridden through the ordinary pipeline; the model is owned by
ADR-0021 ([decision register](../adr/index.md)) and
[ontologies.md](../architecture/ontologies.md). Everything here is reader-ceiling, cursor-paginated
per the convention, and fails closed as absence: an unknown CURIE and one outside the key's read
scope are indistinguishable (`access.not_found` on the term get, an empty match elsewhere).

| Endpoint | Returns |
|---|---|
| `GET /v2/vocab/suggest?q=…&workarea_id=…` | Deterministic prefix+label typeahead over granted vocabulary workareas: `curie`, `label`, `term_status` per suggestion. Matching, never interpretation — no model runs here. |
| `GET /v2/vocab/terms/{curie}` | The term's served tip: owning workarea, version, `term_status` (`active` \| `deprecated` — a structural payload field that survives shredding), definition presence (the CURIE is permanent, the body erasable), ratified `successors` with their relation (`replaced_by` \| `consider`), and the releases whose pin includes it. |
| `GET /v2/vocab/terms/{curie}/hierarchy?direction=…&release=…` | `parents` \| `children` \| `ancestors` \| `descendants` over ratified `is_a` — served from the per-release closure materialized at release ratification (a lookup, never a traversal: serialize, don't compute). `release` is optional and defaults to the latest ratified release of the owning vocabulary. |
| `GET /v2/vocab/mappings?term=…` | Ratified SSSOM-shaped mappings touching the term: `subject`, `object`, `predicate` (`exactMatch` \| `closeMatch` \| `broadMatch` \| `narrowMatch` \| `relatedMatch`), `justification`, `ratified_at`. Both endpoints persist — a mapping never merges. |

There are no vocabulary write endpoints: `term`, `term_mapping` and `vocab_release` submissions
ride [`POST /v1/memories`](#submit-post-v1memories) with those `kind` values — held, scanned,
reviewed and ratified like any claim, with the shipped review policy defaulting
`replaced_by` deprecations, `exactMatch` mappings and `vocab_release` pins to human-required
verdicts (tenant-tunable). Vocabulary workareas refuse `provenance: simulation` submissions of
these kinds. The typed refusals this surface adds — `vocab.cycle_refused`,
`vocab.prefix_conflict`, `vocab.unknown_curie`, `vocab.inactive_curie` (whose `details` carry the
successors for one-hop self-healing) — are declared in [errors.md](errors.md).

## Events tail

`GET /v2/events?after_seq=<n>&limit=<m>&workarea_id=<node>` — the per-tenant journal, served as
the [event catalog](../data/event-catalog.md)'s envelopes with **digest-only payloads** (D1: no
body plaintext ever rides a journal event). Requires reader effective access on `workarea_id`,
else `access.not_found`; omitted, `workarea_id` is the key's default workarea, and a key with none
answers `request.workarea_unresolved`. The tail filters to that subtree within reader effective
access, and events outside it are **omitted**, so a scoped tail has gaps by design.

This page is the single declaration site for which events a key sees
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d15). Visibility depends only on
the journal up to the event and on the key's current access, never on later row state:

| Event family | Who sees it |
|---|---|
| `lifecycle.*` | a key for which **(a)** or **(b)** holds, and no other. **(a) Reader clause:** the key has reader access on the subject's workarea, and the subject passes three conditions. *Promoted:* in the journal up to and including the event, the subject version's latest transition between held and promoted is a promotion (`lifecycle.promote` or `lifecycle.supersede`), not a `lifecycle.reassign` with `re_enters_held: true` or a `review.reopen`; a version never promoted fails it; a `lifecycle.reassign` is judged on the journal before it, so the readers it takes a promoted version from see it leave. *Audience:* the subject is not a simulation row or sim-derived version outside the key's audience ([simulations](../architecture/simulations.md)). *Self-signed:* the subject is not `self_signed` unless the tail's `workarea_id` is the subject's own workarea ([retrieval](../architecture/retrieval.md)). **(b) Author clause:** the key's [own](../architecture/roles-and-keys.md#own) key set holds the subject version's authoring key, read from the journal as the version's submission event (`submission.*` or `simulation.write`) says; the subject's workarea lies within the tail's subtree minus the key's global denials; and the subject is not a simulation row or sim-derived version outside the key's audience. So the erase of another author's held or never-promoted version reaches no reader, and an author sees every `lifecycle.*` event of its own versions in the tail's subtree |
| `workarea.*` | keys with reader access on the subject workarea, per the table below |
| `submission.*`, `simulation.write` | only a key whose [own](../architecture/roles-and-keys.md#own) key set holds the subject version's authoring key, `memories.author_key_id` — for `submission.*` and `simulation.write` the envelope's `api_key_id`, for `lifecycle.*` the `api_key_id` of the subject's submission event, read from the journal — within the tail's subtree minus its global denials, and only while the subject is not a simulation row or sim-derived version outside the key's audience, with no reader grant needed on the event's own workarea: the own-work rule |
| `review.*`, `claim_edge.*`, `key.*`, `grant.*`, `connection.*`, `tenant.*`, `system.*` | no key on this tail; they belong to the [evidence export](#evidence-export) |

The subject and its workarea come from the event's own payload, plus the subject's earlier events
— its submission, and the `lifecycle.promote` or `lifecycle.supersede` that promoted it — for the
simulation, taint and `self_signed` marks:

| Event types | Subject | Subject workarea |
|---|---|---|
| `submission.remember`, `submission.learn`, `simulation.write`, `lifecycle.promote`, `lifecycle.demote`, `lifecycle.erase` | the version the payload names | its `workarea_id` |
| `submission.attach` | the version the attachment binds into | that version's `workarea_id`, from its submission event |
| `lifecycle.supersede` | the new version | its `workarea_id` |
| `lifecycle.tip_override` | the version that becomes the tip (`to_artifact_id`) | its `workarea_id` |
| `lifecycle.reassign` | the moved version | `from_workarea_id`, and `to_workarea_id` when `re_enters_held` is false. The reader clause holds when the key has reader on either listed workarea and that workarea lies within the tail's subtree, and its self-signed condition compares the tail's `workarea_id` with that same workarea; under the author clause, either listed workarea within the tail's subtree suffices. The reader clause's promoted, audience and self-signed conditions apply to it as to every `lifecycle.*` event, so a reassign that fails them — a held version's move, say — is shown only under the author clause |
| `workarea.create`, `workarea.reparent`, `workarea.status_proposed`, `workarea.status_transition`, `workarea.crew_updated` | the node | the node itself |
| `workarea.relate_proposed`, `workarea.relate_ratified` | the edge | both endpoints: shown when the key has reader on both and at least one lies within the tail's subtree |

An event type the tables do not map is not shown. Two keys with equal access therefore see the
same `lifecycle.*` and `workarea.*` events in the same order; each additionally sees only its own
`submission.*` and `simulation.write` events and, under the author clause, the `lifecycle.*`
events of its own versions.

**What a served event names.** A claim-subject workarea id in a served payload —
`workarea_id`, `from_workarea_id` or `to_workarea_id` — that the
[naming rule](../architecture/roles-and-keys.md#own) withholds from the key, a node on which it
holds no role, is replaced by null. `workarea.*` payloads serve `parent_id`, `old_parent_id` and
`new_parent_id` as the [workarea read](#workareas) serves `parent_id`: the ancestors of a node the
key can read, and a re-parented node's former parent, are the node ids the naming rule states it
discloses, so the tail and the workarea read agree about them. An
event served to a key outside its actor's own key set, the actor being the envelope's
`api_key_id`, carries `principal` and `api_key_id` as null, and every payload field that names a
key — `proposer_key_id`, `ratifier_key_id` and each `*_by_key_id` — as null: the tail tells a
key what happened to what it may read, never which reviewer or admin did it. The
[evidence export](#evidence-export) carries the identities.

**What a served event counts.** `seq`, `head_seq` and `next_after_seq` are tenant-wide journal
positions, and so is `permission_epoch` on the [workarea read](#workareas), so a key learns how
many events it cannot see occurred between two it can, and when — a count, never content. It is
a stated residual, tracked with the other counts a key cannot read as
[FR-31](../product/roadmap.md).

One call scans at most `limit` raw events from `after_seq + 1`; the response carries
`next_after_seq`, which the client passes back verbatim as its next `after_seq`, and `head_seq`,
so a consumer can measure its lag. `head_seq` is read in the snapshot the scan reads. When no raw
event follows `after_seq`, `next_after_seq` = `after_seq`; otherwise it is
min(`after_seq` + `limit`, `head_seq`) — the last raw seq scanned, since a tenant's seqs have no
gaps. A page may hold fewer than `limit` events, or none, while `next_after_seq` still advances.
An `after_seq` greater than `head_seq` is `request.validation`.

The honest consequence: a scoped tail is a feed, not a verification surface — omitted events break
naive hash-chain recomputation. Full per-tenant chain verification is served by the
[evidence export](#evidence-export) (`kind: "chain"`), which is how `oml audit verify-chain` reads
the chain live ([cli/reference.md](../cli/reference.md)).

## Review

The review ceremony over HTTP — external crews holding dreamer keys are the designed review
capacity; the CLI is the human exception path (D11, D14). Review throughput is unmeasured —
[FR-2](../product/roadmap.md).

| Endpoint | Semantics |
|---|---|
| `GET /v2/review/queue?workarea=…` | Held-item counts and oldest ages per workarea in the reviewer's scope — the same numbers `oml status` reports as queue depth. |
| `POST /v2/review/next` | Serve exactly one held item: body, **all** attachments (as fetch descriptors honored by `GET /v1/attachments/{digest}?serve_id=…`), and for a supersession the version it replaces. Returns a `serve_id` bound server-side to (reviewer key, artifact version, attachment digest set), expiring, consumed by its first verdict, invalidated by any change to the served content (D14). Serves only rows within the key's dreamer effective access on the row's workarea. `204` when nothing in scope waits. A `held_by: service` key is refused here, as on every review-class verb and on status-transition ratify, with `access.verb_ceiling` before any lookup ([roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)). Refused per item with `review.author_in_lineage` when the reviewer's key or principal appears in the item's lineage closure, or authored a rival held successor of the same target ([rival successors](../architecture/roles-and-keys.md#the-dreamer-split)) — the server never serves what this key may not judge. |
| `POST /v2/review/verdicts` | `{serve_id, verdict: "approve" \| "reject", reason}` (`reason` required to reject). Accepted only against a valid, unexpired, unconsumed serve bound to this key (`review.serve_invalid` names which condition failed). The journalled verdict records the served digests. Response states the resulting state: `promoted` (policy satisfied, ratification journalled with its policy snapshot), `held` (more verdicts required by the policy's `k`), or `rejected`. Requires dreamer effective access on the row's workarea, resolved by the [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) like every other verb: `access.grant_refused` with `needed_role: dreamer` where reader is effective there, `access.not_found` otherwise. An approve that would ratify a supersession whose target is no longer the tip of the successor's own workarea and `claim_key` is refused with `409 submission.tip_conflict` (`details.current_tip` when readable): nothing is journalled and the `serve_id` is not consumed, so the same serve can still carry a reject ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d1, d10). |
| `POST /v2/review/reopen` | `{artifact_id, version, reason}` — non-author reopen of a rejection, a journalled verb in its own right, not a resubmission. The original author is refused (`review.author_in_lineage`). A reopen that would put a second live version under one `claim_key` and ordinal is refused with `409 submission.claim_key_conflict` ([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)). |

The independence checks — reviewer ≠ author, resolved against the full lineage closure at
promotion time, plus the per-workarea independence axis — are owned by
[roles-and-keys.md](../architecture/roles-and-keys.md) and
[admission.md](../architecture/admission.md); this surface only reports their refusals.

## Keys, grants, denials

Admin key role throughout; a grant's role must be admissible under the key's role, and an issued
key's role never exceeds the issuer's (`access.issuer_ceiling`; the
[admissibility table and issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)).
One combination is refused outright: a key holding the admin role never carries dreamer-class grants — minting or amending
one answers `access.role_exclusive` (the two-key rule: admin ≠ reviewer,
[roles-and-keys.md](../architecture/roles-and-keys.md)). Key values appear exactly once,
in the issuance response; the journal records `key.issued` with `value_disclosed: false`
(show-once ceremony, D14).

| Endpoint | Does |
|---|---|
| `POST /v2/keys` | Issue: `{role, grants: [{workarea_node_id, role, deny: [{subtree_node_id}]}], default_workarea_id?, held_by?, expiry?, label?, principal?, tenant_id?, connect_code?}` → `201` with the key **value, shown once** — or, with `connect_code: true`, a connector key with no value and its `connect_code` `{code_id, code, expires_at, connector_url}` in the value's place ([connections](../architecture/roles-and-keys.md#connections)) — each stored grant echoed with its `grant_id` and `epoch` (its `grant.issued` journal seq), and the `default_workarea_id` and `held_by` the key was issued with, so the issuer sees the default the server chose and whether its attestation took effect. Unknown fields are refused; the request is validated in the [issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order), which also fixes the default workarea, when the value is disclosed and what a part-way failure leaves. An operator key must add `tenant_id`, an admin key must not send it, and an operator key may issue only a tenant's first admin key. |
| `GET /v2/keys` · `GET /v2/keys/{key_id}` | Listing and detail: role, principal, `default_workarea_id`, `held_by`, grants, denials, epoch, issuance/revocation events. Never values. |
| `POST /v2/keys/{key_id}/revoke` | Journalled revocation; effective at the key's next request served by the lead, and after replication lag on a follower ([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)). |
| `POST /v2/keys/{key_id}/rotate` | Mint a successor key — new `key_id`, linked by `rotated_from`, grants reproduced from the journalled records — with `{grace}` bounding the old key's remaining life. One journalled act (`key.rotated`) carries both key ids and the grace deadline; at the deadline the old key is revoked (journalled, epoch bump). A connector key is not rotated: `422 oauth.connect_refused` (`details.reason: connector_key`); a fresh connection is a new code, a fresh identity a new connector key ([connections](../architecture/roles-and-keys.md#connections)). Bulk crew rotation is open — [FR-6](../product/roadmap.md). |
| `POST /v2/keys/{key_id}/grants` | Add a grant bound to a workarea **node id**, optionally with per-grant deny masks → `201`, epoch bump. |
| `POST /v2/keys/{key_id}/grants/{grant_id}/revoke` | Journalled grant removal; epoch bump. |
| `POST /v2/keys/{key_id}/denials` | Create or extend the key's **global** deny object — deny-wins across all the key's grants, distinct from a per-grant deny. Journalled as `key.denied`, whose seq is the denial's epoch. |
| `POST /v2/keys/{key_id}/denials/{denial_id}/lift` | Journalled lift. |
| `POST /v2/keys/{key_id}/connect-codes` | Mint a fresh connect code for a connector key → `201` `{code_id, code, expires_at, connector_url}`, the code shown once; the same commit (`connection.code_issued`) voids the key's earlier codes as [connections](../architecture/roles-and-keys.md#connections) declares. A bearer key, a revoked or expired key, or a deployment without `oauth.issuer` answers `422 oauth.connect_refused`. |
| `POST /v2/keys/{key_id}/connections/{connection_id}/revoke` | `{reason?}` → journalled `connection.revoked`, which ends the connection and voids the key's codes as [connections](../architecture/roles-and-keys.md#connections) declares. An `authorized` connection is not listed, and naming one answers `access.not_found`, as an unknown id does. `GET /v2/keys/{key_id}` lists a key's open and revoked connections. Both are served whether or not the deployment sets `oauth.issuer`. |
| `GET /v2/keys/{key_id}/effective-access?workarea=…` | The resolved answer: what this key may do where, and which deny produced each hole — reproducible from journalled grants by construction (permission epoch). |

`POST /v2/tenants` (**operator**) creates a tenant; the operator then issues its first admin key
and nothing else — operator keys never read, write or review content (`access.verb_ceiling`).

`POST /v2/bootstrap/claim` is the one endpoint authenticated by something other than an API key:
the single-use, expiring **enrollment token** the Helm bootstrap job minted
([deployment.md](../operations/deployment.md)). `{enrollment_token}` → `201` with the root
operator key, value shown once; the token is consumed on first use, and a consumed, expired or
unknown token answers `auth.invalid` with `details.reason` naming which. The `oml bootstrap claim`
ceremony fronts exactly this call ([cli/reference.md](../cli/reference.md)).

## Connections: OAuth for hosted MCP clients

This section is the single declaration site of the HTTP side of connections — the `/mcp`
challenge, the two discovery documents, the OAuth endpoints and the authorization page
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d1–d6, d9, d14). What a
connection, a connect code and a token are, and why a token is never more than its key, is
declared on [roles and keys](../architecture/roles-and-keys.md#connections); the refusals on
[errors](errors.md#oauth-connections). Everything here exists only where the deployment sets the
boot key `oauth.issuer` ([configuration](../operations/configuration.md)), the public origin, for
example `https://oml.example.com`; the protected resource is `<issuer>/mcp`. Unset, the documents
and endpoints below answer `404`, `/mcp` is bearer-only, and a connect code is refused; listing and
revoking a key's connections on `/v2` are still served. Decided, not built
([FR-34](../product/roadmap.md)).

**The challenge.** A `401` from `/mcp` keeps its typed body (`auth.missing` with no header,
`auth.invalid` for a credential that failed) and carries:

```http
WWW-Authenticate: Bearer resource_metadata="https://oml.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp"
```

with `error="invalid_token"` added on `auth.invalid` (RFC 6750 §3.1), so a client refreshes or
reconnects. Without `oauth.issuer` the header is `Bearer realm="oml"`. No other route's `401`
changes, and no `200` carries the challenge.

**The discovery documents.** Both are unauthenticated JSON, built from `oauth.issuer` alone,
never from the request's `Host`:

- `GET /.well-known/oauth-protected-resource/mcp` — RFC 9728: `resource` exactly `<issuer>/mcp`
  (RFC 9728 §3.3; a connector URL entered with another spelling, a trailing slash included, does
  not match), `authorization_servers` `[<issuer>]` (one entry), `scopes_supported` `["mcp"]`,
  `bearer_methods_supported` `["header"]`, `resource_name`. The root form,
  `/.well-known/oauth-protected-resource`, is not served.
- `GET /.well-known/oauth-authorization-server` — RFC 8414: `issuer`, `authorization_endpoint`,
  `token_endpoint`, `registration_endpoint`, `response_types_supported` `["code"]`,
  `grant_types_supported` `["authorization_code", "refresh_token"]`,
  `code_challenge_methods_supported` `["S256"]`, `token_endpoint_auth_methods_supported`
  `["none"]`, `scopes_supported` `["mcp"]`, `client_id_metadata_document_supported` `true`,
  `authorization_response_iss_parameter_supported` `true` (RFC 9207 §3: every authorization
  response, success or error, carries `iss`). No OpenID Connect discovery document,
  `/.well-known/openid-configuration`, is served.

**The endpoints.**

| Endpoint | Does |
|---|---|
| `GET /oauth/authorize` | Checks the parameter bounds first — `state` at most 512 bytes, `client_id` and `redirect_uri` at most 2 048 bytes each, a `code_challenge` that is present matching `^[A-Za-z0-9_-]{43}$`, and no parameter sent more than once (RFC 6749 §3.1) — then validates the client (a metadata document fetched under the bounds below, or a registered client), the redirect URI (the allowlist match below), `response_type=code`, PKCE, `resource` and `scope`, then renders the connect-code page for an authorization request alive 10 minutes. A parameter past its bound, a repeated one and a present `code_challenge` that does not match the pattern included (`request.validation`), and a refused client or redirect URI (`oauth.client_invalid`, `oauth.redirect_refused`) are a `400` page and are never redirected to (RFC 6749 §4.1.2.1). After the client and the redirect URI are accepted, an absent `code_challenge`, or a `code_challenge_method` that is absent (RFC 7636 §4.3 reads that as `plain`) or not `S256`, is `oauth.pkce_required`; it and every other later refusal — `oauth.resource_mismatch`, `request.validation` — redirect with `error`, `error_description`, `state` and `iss`. The authorization request writes no row: the page's form carries it as a handle sealed by an AEAD under a key the lead generates at its start and holds only in memory, and a restart or takeover ends every page in progress ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d9). |
| `POST /oauth/authorize` | The page's own form, in two submissions and no others. Every submission first needs either `Sec-Fetch-Site: same-origin` or, when that header is absent, an `Origin` equal to the issuer origin; `Origin: null` and anything else answer `403 oauth.origin_refused` on the page, never redirected, and consume no attempt. Step 2: exactly `handle`, the sealed handle the first page carries, and `connect_code`. Step 3: exactly `handle` and `action` (`connect` \| `cancel`), where the handle names the request row a verified code wrote and whose code is not voided. Any other combination — both fields, an action before a verified code, a second code after one verified — is `request.validation` on the page. A correct code of a key that is neither revoked nor expired is spent, the authorization request's row is written with the code's id under a fresh 256-bit handle, and the confirmation page shows what the connection will act as (below); a wrong one — unknown, expired, voided, spent, or a revoked or expired key's — answers the form again, `400 oauth.connect_code_invalid`, with the attempts left and a new sealed handle carrying one attempt fewer, and the fifth ends the request with `access_denied` (`oauth.authorization_expired`, which also answers a handle past its 10 minutes, one that does not open under the lead's key, and a step-3 handle that names no row or whose code is voided, [connections](../architecture/roles-and-keys.md#connections)). `connect` creates the connection, `authorized`, and redirects (`303`) with a 60-second, single-use authorization code, `state` verbatim, percent-encoded as a query parameter, and `iss`; `cancel` with `access_denied`, `state` and `iss`. Connect and Cancel each delete the authorization request's row in their own commit, so a later step-3 `POST` with that handle names no row and answers `oauth.authorization_expired` on the page. |
| `POST /oauth/token` | `application/x-www-form-urlencoded`. `grant_type=authorization_code` with `code`, `client_id`, `redirect_uri`, `code_verifier` and optional `resource`, or `grant_type=refresh_token` with `refresh_token`, `client_id` and optional `resource` → `200` `{access_token, token_type: "Bearer", expires_in, refresh_token, scope: "mcp"}` with `Cache-Control: no-store`. `redirect_uri` is required exactly when the authorize request carried it, and must then equal it byte for byte; when the authorize request omitted it, `redirect_uri` may be omitted or must equal the client's one registered URI; anything else is `oauth.authorization_code_invalid`. The first exchange opens the connection and each refresh rotates the refresh token; which tokens a refresh accepts, and when a reused refresh token or authorization code revokes the connection, are declared on [roles and keys — connections](../architecture/roles-and-keys.md#connections). The client is the one stored on the connection the code or the refresh token belongs to (`oauth_connections.client_id`), and the client tables are never read here, so a document client whose cached document lapsed still refreshes; a `client_id` that differs from the connection's is `invalid_grant` (`oauth.authorization_code_invalid` or `oauth.refresh_invalid`). An absent `code` or `refresh_token`, or any parameter sent more than once (RFC 6749 §3.2), is `request.validation` (`invalid_request`), and an unknown `code` or `refresh_token` `invalid_grant`, whatever its `client_id`. Rotation and redemption are conditional updates declared on the [Postgres schema](../data/postgres-schema.md#connections-adr-0031). A revoked or expired key, a voided code, or a connection whose resource is not the current `<issuer>/mcp` answers `invalid_grant` ([connections](../architecture/roles-and-keys.md#connections)). Refusals are RFC 6749 §5.2 bodies: `oauth.authorization_code_invalid` and `oauth.refresh_invalid` as `invalid_grant` — a code or refresh token issued to another client included (RFC 6749 §5.2) — and `oauth.resource_mismatch` as `invalid_target`. `oauth.client_invalid` is `invalid_client` and answers only a request that sends client authentication, since every client is public: one that sends an `Authorization` header gets `401` with `WWW-Authenticate` naming the scheme it used, for example `Basic realm="oml"` (RFC 6749 §5.2), and one that sends a `client_secret` in the body gets `400`. The path makes no outbound call. |
| `POST /oauth/register` | RFC 7591, `application/json`: `redirect_uris` (each inside the allowlist), `client_name`, `token_endpoint_auth_method: "none"`, `grant_types` within `authorization_code` and `refresh_token`, `response_types` `["code"]` → `201` with a server-minted `client_id`, no secret, no registration access token; a redirect URI outside the allowlist is `oauth.redirect_refused` as `invalid_redirect_uri`. `client_name` is required, because the page shows it, and its absence is `request.validation` as `invalid_client_metadata`. An absent `token_endpoint_auth_method`, which RFC 7591 §2 reads as `client_secret_basic`, is registered as `none` and echoed as `none`; any other value is `request.validation` as `invalid_client_metadata`. A client that has not connected within 24 hours is removed; past 10 000 unused registrations the endpoint answers `429 limit.rate`. |

**Client identity.** A `client_id` that is an `https` URL with a path names a client ID metadata
document. Its form is checked first: no userinfo, no fragment, no query, no `.` or `..` path
segment, no percent-encoded `/` or `.`, no port but the default, a host that is a domain name and
never an IP literal, and exactly its own WHATWG URL serialization, else `oauth.client_invalid`
(`reason: client_id_form`). `oml-server` then fetches it at `GET /oauth/authorize`, under these
bounds:

- every A and AAAA answer for the host is inside an allow-list, and the fetch is refused if any
  answer is not. An AAAA answer is admitted only inside `2000::/3`, the global unicast space, and
  never inside `2001::/23`, `2001:db8::/32`, `2002::/16` or `3fff::/20`, the blocks inside it that
  the IANA IPv6 Special-Purpose Address Registry (fetched 2026-10-31, last updated 2025-10-09)
  marks not globally reachable or that embed an IPv4 address. An A answer is admitted only when it
  is unicast, outside `224.0.0.0/4`, outside the deprecated 6to4 relay block `192.88.99.0/24`, and
  inside no block the IANA IPv4 Special-Purpose Address Registry (fetched 2026-10-31, last updated
  2025-10-09) marks "Globally Reachable: False" — as fetched, `0.0.0.0/8`, `10.0.0.0/8`,
  `100.64.0.0/10`, `127.0.0.0/8`, `169.254.0.0/16`, `172.16.0.0/12`, `192.0.0.0/24`,
  `192.0.2.0/24`, `192.168.0.0/16`, `198.18.0.0/15`, `198.51.100.0/24`, `203.0.113.0/24`,
  `240.0.0.0/4` and `255.255.255.255/32`. Every IPv6 form that embeds an IPv4 address — `::/96`,
  `::ffff:0:0/96`, `::ffff:0:0:0/96`, `64:ff9b::/96`, `64:ff9b:1::/48`, `2002::/16`, Teredo's
  `2001::/32` — is therefore refused by construction, never unwrapped, as are `100::/64`,
  `5f00::/16`, `fc00::/7`, `fec0::/10`, `fe80::/10` and `ff00::/8`, all outside `2000::/3`. A
  network-specific NAT64 prefix (RFC 6052) on the lead's own network lies inside `2000::/3` and
  cannot be recognized; the deployment's egress policy refuses it
  ([security](../architecture/security.md#connections-the-oauth-surface));
- the TCP connection is made to the checked address with no second resolution, and the HTTP
  client ignores proxy environment variables;
- TLS verifies the certificate against the web PKI for the URL's host, which is also the SNI;
- no redirect is followed; the request sends `Accept-Encoding: identity`; the fetch is cut at 5
  seconds and at 16 KiB of bytes received;
- at most 16 fetches run at once on the lead, at most one per host name and at most one per
  resolved address; past any cap, with no cached copy to serve, the request is
  `oauth.client_invalid` (`reason: fetch_busy`).

The body is JSON whose `client_id` equals the URL, carrying `client_name` and `redirect_uris`,
with `token_endpoint_auth_method` absent or `none`. It is cached as its `Cache-Control` allows and
kept at most 24 hours from its fetch; within those 24 hours a copy past its `Cache-Control`
lifetime is served when a fresh fetch fails or a fetch cap is full. At most 10 000 documents are
cached; a full cache never refuses: it evicts the least recently used document whose client has
not connected — a client has connected once a Connect on the page names it, which stamps
`oauth_clients.connected_at` ([Postgres schema](../data/postgres-schema.md#connections-adr-0031))
— and when every cached document's client has connected, the fetched document serves its request
uncached. Nothing after `GET /oauth/authorize` reads the cache — the sealed handle carries what
the page needs, and the token endpoint identifies the client by the connection — so an eviction
affects no page in progress and no connection. Any other `client_id` must be one
`POST /oauth/register` minted. No client is pre-registered and none holds a secret.

**The redirect allowlist.** Exactly `https://claude.ai/api/mcp/auth_callback`, matched byte for
byte on the parameter's value after exactly one `application/x-www-form-urlencoded` decoding (in
an authorize query or a token body), or on the JSON string value (in a registration or a
document), and never after URL parsing or any further decoding. Nothing else, at registration, in
a document or in an authorize request.

**The authorization page.** It sets no cookie and runs no script. Every page carries
`Content-Security-Policy: default-src 'none'; style-src 'sha256-…'; form-action 'self' <redirect
origin>; frame-ancestors 'none'; base-uri 'none'`, `Cache-Control: no-store`,
`Referrer-Policy: same-origin` and `X-Content-Type-Options: nosniff`; `same-origin`, not
`no-referrer`, because `no-referrer` makes a browser send `Origin: null` on the page's own `POST`
(the Fetch Standard, "append a request `Origin` header", fetched 2026-10-31). It names the client —
a document client's `client_name` "verified by" the document URL's whole host, in lower-case ASCII
(the punycode A-label form), never shortened and rendered apart from `client_name`; a registered
client's `client_name` marked "name not verified" — and the redirect URI's host. Its sentences
are fixed, word for word:

- Above the code field: "Enter the code only if you opened this page yourself,
  by adding the connector in Claude, in the last few minutes. If someone sent you this link, close
  it."
- A wrong code — unknown, expired, voided, spent, or a revoked or expired key's alike — with the attempts left: "This code
  is not valid. Check it, or ask your administrator for a new one."

After a correct code, the confirmation shows the tenant the key belongs to, by the name the
operator gave it at `POST /v2/tenants`, which no tenant admin can set; the key's principal and
role; every workarea it holds a grant on, with the grant's role; and one sentence per kind of verb
the key's role and effective access give it:

- where it holds writer: "Claude can remember and learn as this agent in `<workareas>`."
- where it holds reader: "Claude can recall and read as this agent in `<workareas>`."
- where it holds dreamer(propose): "Claude can propose edits and edges as this agent in
  `<workareas>`."
- where it holds writer or dreamer(propose): "Everything it writes is held until someone other
  than its author reviews it."
- for each granted workarea whose tenant policy permits `self_signed` rows: "In `<workarea>`, its
  writes may be self-signed and readable there without review."
- last, always: "It cannot approve, demote, delete or administer anything."

The buttons are Connect and Cancel. Client-supplied strings are HTML-escaped and shortened;
`logo_uri` is never loaded
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d9).

**Behind a proxy.** A reverse proxy in front of `oml-server` — the hosted deployment's Caddy
included — forwards `/mcp`, `/.well-known/oauth-protected-resource/mcp`,
`/.well-known/oauth-authorization-server` and `/oauth/*` unchanged, with no path rewriting and no
caching; the page's headers are `oml-server`'s own. A proxy that writes access logs must not
record the `Location` response header of `POST /oauth/authorize`, whose query carries the
authorization code, nor the request bodies of `POST /oauth/authorize` and `POST /oauth/token`,
which carry the connect code, the authorization code, the verifier and the refresh token. Caddy's
access-log fields include response headers under `resp_headers` (Caddy's `log` directive
documentation, fetched 2026-10-31), so the hosted Caddy's `log` directive carries
`format filter` with `resp_headers>Location delete`, whatever Caddy records by default, and adds
nothing that logs a request body. Doctor cannot read a proxy's configuration: the rule is stated,
not checkable ([observability](../operations/observability.md#per-profile-checks)). Claude's hosted
apps call from `160.79.104.0/21` (Anthropic's connector documentation, fetched 2026-10-31); a
firewall that admits by address admits that range on `/mcp`, the two documents, `/oauth/token`
and `/oauth/register`, while `/oauth/authorize` is reached by the person's own browser. In cluster
mode a follower has no `oauth.issuer` of its own: it serves the challenge, the discovery documents
and token validation from the value the lead records in the `lead_lease` row
([configuration](../operations/configuration.md),
[Postgres schema](../data/postgres-schema.md#outbox-dead_letter-idempotency)), so a follower never
honors tokens or serves documents for an issuer the lead does not, beyond replication lag. A
lead-eligible node reads its own value only when it takes the lease, so a takeover by one
configured with another value is an issuer change
([roles and keys](../architecture/roles-and-keys.md#connections)). A
follower forwards the OAuth endpoints to the lead, which writes, and validates access
tokens from its replica for the reads it serves, with the revocation lag a key has there. A
follower forwards an `/mcp` request that carries an access token and reaches a write verb as the
whole `/mcp` `POST` to the lead's `/mcp`, over the enrolled hop, with the token unchanged in
`Authorization`; the lead re-resolves token → connection → key and authorizes against that key.
This is the MCP request forwarded to the same endpoint, not a token passed to a ring route.

## Bans

- `POST /v2/bans` — `{subject_kind: "principal" | "source", subject, reason?, dry_run}`. With
  `dry_run: true` (the CLI's default) the response is the reach preview — keys affected, held
  submissions that would be refused, workareas touched — and nothing is journalled. Otherwise
  `201` and the ban is checked at every admission from then on (`access.banned`).
- `POST /v2/bans/{ban_id}/lift` — journalled lift; the ban row stays as history (R4 in spirit).
- `GET /v2/bans` — active and lifted, with who imposed and who lifted each.

## Simulations

The fenced provenance channel (D10; [simulations.md](../architecture/simulations.md)). Admin key
role for the channel objects; the writes inside a simulation are ordinary submissions carrying
`simulation_id`, made by audience keys through `POST /v1/memories`.

- `POST /v2/simulations` — `{audience: [{"key_id": …} | {"grant_ref": …}], label?}` → `201`. A
  simulation whose audience holds no dreamer key can promote nothing — it fails closed, and the
  response says so in a `warnings` field rather than refusing.
- `GET /v2/simulations` · `GET /v2/simulations/{id}` — state, audience, rows written, sim-derived
  taints outside it.
- `POST /v2/simulations/{id}/audience` — `{add: [{"key_id": …} | {"grant_ref": …}], remove: [...],
  reason?}`, at least one of `add`/`remove` non-empty, `Idempotency-Key` required → `200` with the
  resulting audience. Corrects a simulation's audience — the only recovery from the fail-closed
  no-dreamer state short of ending it; journalled as `simulation.audience_changed`, and the same
  `warnings` field as creation fires when the corrected audience still holds no dreamer key.
  Removal takes effect on the next read; past reads stand in the read log. Refusals:
  `access.verb_ceiling` (non-admin), `simulation.ended`, `request.validation` (empty `add`+`remove`),
  `access.not_found`.
- `POST /v2/simulations/{id}/end` — `{dry_run}`: preview lists every row and taint the end would
  touch; the real act is journalled, demotes the simulation's rows and triggers re-review of
  tainted claims → `202` (the demotion closure proceeds through the pipeline).

Refusals this channel generates on *other* endpoints: non-audience keys never see simulation rows
(`access.not_found` on reads — the predicate is applied on every read path); non-audience writes
and verdicts naming a simulation answer `simulation.audience_refused`; acts naming an ended
simulation answer `simulation.ended`; promotion or reassignment that would put a simulation row or
sim-derived taint into a `validated` area answers `simulation.taint_refused`; a promotion,
supersession ratification or tip-override that would make a tainted version the *default* served
tip answers `simulation.tip_refused` — taint shadows the tip only for the audience, as an overlay
([simulations.md](../architecture/simulations.md)).

## Claim administration

Admin verbs over claims — the CLI's `oml claims` and `oml erase` ride these
([cli/reference.md](../cli/reference.md)):

| Endpoint | Does |
|---|---|
| `POST /v2/memories/{artifact_id}/reassign` | `{workarea_id, re_key?, dry_run}` — journalled move recording both policy snapshots; a claim moved to a stricter-policy destination re-enters `held`. Refused into `validated` (`workarea.frozen`); a `claim_key` collision at the destination is refused unless the move records a re-key (`submission.claim_key_conflict`). |
| `POST /v2/memories/{artifact_id}/demote` | `{version, reason}` — journalled demotion; bytes kept (R4). |
| `POST /v2/memories/{artifact_id}/erase` | `{dry_run}` — journalled crypto-shred: erase event plus DEK destruction, scoped to body and attachment bytes. The dry run returns the erasure inventory for this artifact — every copy class and its disposition ([audit-and-lineage.md](../architecture/audit-and-lineage.md)). Attribution metadata survives; closing that is [FR-1](../product/roadmap.md). → `202`. |
| `POST /v2/claims/tip-override` | `{workarea_id, claim_key, to_version, reason}` — designate an earlier promoted version as the served tip without demoting the later one: explicit and journalled (`lifecycle.tip_override`), never a silent re-rank. |

## Evidence export

An export is an **async job** — bundles are large and chain verification is linear in journal
length.

1. `POST /v2/exports` — `{kind: "evidence" | "chain", from_seq?, include_simulations?}` → `202`
   with `export_id`. `evidence` is the signed bundle: per-memory decision trail, per-tenant chain
   verification, grants history, read log scoped to its retention window, reciprocal-approval
   statistics per key pair, and — only when asked — simulation rows listed separately by
   `simulation_id` (D14). `chain` streams the per-tenant journal envelopes, digest-only, for
   client-side verification. The export act is itself journalled.
2. `GET /v2/exports` · `GET /v2/exports/{export_id}` — job state: `running` | `done` | `failed`.
3. `GET /v2/exports/{export_id}/bundle` — the signed bundle, `application/octet-stream`;
   `export.not_ready` before the job is `done`.

The sanctioned description of what this produces: *the evidence an auditor asks for* — never a
compliance claim (D17; [audit-and-lineage.md](../architecture/audit-and-lineage.md)).

## Settings

Configured-versus-effective, over the API so `oml settings` can show the divergence (D14):

- `GET /v2/settings` — the configured document as last applied; `?effective=true` adds what the
  running deployment actually uses, every divergence marked per key (defaulted value,
  not-yet-picked-up change, per-replica variance).
- `POST /v2/settings/validate` — server-side validation without applying: `200` with findings. A
  document naming a **retired settings key is refused by that key's name**, with its replacement —
  the finding carries code `settings.retired_key`.
- `POST /v2/settings/apply` — validate, journal (`system.settings_applied`), apply. Any validation
  error refuses the whole apply (`settings.invalid` / `settings.retired_key`); there is no force
  flag.

The settings schema itself is not fixed by this design; the contract fixed here is the
show/validate/apply shape and the retired-key refusal.

## Health

- `GET /healthz` — liveness: `200 {"status": "ok"}` while the process runs.
- `GET /readyz` — readiness: `200` only when the replica can serve — Postgres reachable and the
  startup fork check passed (the stored chain head verifies against the last signed head;
  [storage.md](../architecture/storage.md)). Otherwise `503` with per-check detail. While a
  replica refuses writes after a failed fork check, API writes on either ring answer
  `system.unavailable`. Ready or not, the body names the build that answered, in a `build`
  object:

  | Field | Value |
  |---|---|
  | `version` | the workspace version the binary was built at |
  | `commit` | the full 40-hex commit it was built from |
  | `commit_time` | that commit's committer time |
  | `build_time` | when the build captured these fields; `SOURCE_DATE_EPOCH`, when the build sets it, fixes this value so a pinned build stays reproducible |
  | `rustc` | the compiler's `rustc -V` line |
  | `target` | the target triple |
  | `profile` | `release` or `debug` |

  Times are UTC, `YYYY-MM-DDTHH:MM:SSZ`. A field nothing could supply reads `unknown`, never a
  guess: a build from a source tree with no git metadata and no `OML_BUILD_COMMIT` /
  `OML_BUILD_COMMIT_TIME` build arguments reports both as `unknown`.

Both are unauthenticated, carry no tenant data, and are the probe targets the Helm chart wires up
([deployment.md](../operations/deployment.md)). The cost of `build`: anyone who can reach
`/readyz` learns the exact build a replica runs. For a released version, that is already public
on the release page; a deployment that wants it hidden keeps `/readyz` off its public route.

## Limits

Two kinds of bounds govern requests. **Size and count bounds** are deployment settings with
shipped defaults — the named bounds below. **Rate and quota bounds** are journalled grant
attributes per key and tenant (D6), a different mechanism with its own refusals.

| Bound | Shipped default | Setting ([configuration](../operations/configuration.md)) | Refusal |
|---|---|---|---|
| Claim body (text) size | 1 MiB | `submission.max_body_size` | `413 request.payload_too_large` |
| Attachment size, each (via [upload sessions](#attachments)) | 64 MiB | `uploads.max_attachment_size` | `413 request.payload_too_large` at session creation |
| Upload part size | 5–64 MiB | `uploads.part_size_min` / `uploads.part_size_max` | `request.validation` at session creation; `413` on an oversize part |
| Attachments per memory | 64 | `submission.max_attachments` | `request.validation` at submit |
| List page `limit` | max 200 (default 50) | `api.page_limit_max` | `request.validation` above the max |

The claim body bound counts UTF-8 bytes of the NFC-normalized body, on every surface that
carries a body — submit, supersede, and the agent plugin's `text` inputs: at most
`submission.max_body_size` (shipped 1 MiB) bytes, and a larger body is
`413 request.payload_too_large`. No schema states a character count for it, because a character
bound neither measures bytes nor follows the setting.

Every value is a **shipped default, deployment-tunable** through the named setting
(`oml settings apply`, journalled). None is tenant-tunable in v1: a tenant that needs a tighter
budget gets one through quota grant attributes (`limit.quota`), not by moving these bounds. And
none is a measured capacity — a default is a policy choice, and the performance consequence of
raising one is unmeasured ([FR-25](../product/roadmap.md)).

Quota and rate-limit enforcement in v1 is per-replica
token buckets at limit/N — a documented approximation whose error grows with replica churn
([FR-8](../product/roadmap.md)) — and the refusal is typed: `429 limit.rate` or `429 limit.quota`
with `Retry-After` ([errors.md](errors.md)). Nothing here is measured; no latency or
throughput number on this page describes OML.
