<!--
id: api/errors
tier: engineering
status: design draft for review · decided, not built
answers: which typed error codes exist, their family, HTTP status and retryability, and why the taxonomy is closed; how the oauth family travels inside RFC error shapes on the OAuth endpoints
-->

# Error taxonomy

**Status:** design draft for review · decided, not built

This page declares the typed, fail-closed error taxonomy of the [HTTP API](http-api.md) (D13).
Every refusal the service makes is one of the codes below — a closed vocabulary: a new refusal
reason is a design change, never an ad-hoc string. The machine-readable form is the
`ErrorCode` enum in [openapi.yaml](openapi.yaml); the two lists are identical by construction, and
a code appearing in one but not the other is a documentation bug.

Three properties hold for every error:

- **Typed.** `code` is machine-stable; clients branch on it, never on `message`.
- **It names its rule.** The `rule` field states the design rule that refused the request, in
  words — an agent that is told *why* can correct course; one that is told "403" retries forever.
- **Fail closed.** Where the service cannot establish that an act is permitted, the act does not
  happen and the refusal says which rule could not be satisfied. No refusal is ever downgraded to
  a silent no-op, and no read ever reveals what the key may not see — absence and refusal-to-admit
  are the two honest answers.

## The envelope

Every non-2xx response on either ring — the provider ring (`/v1`) and the lab ring (`/v2`) share
one taxonomy ([ADR-0022](../adr/ADR-0022-two-ring-api.md)):

```json
{
  "error": {
    "code": "access.verb_ceiling",
    "message": "a writer key cannot transition a workarea",
    "rule": "R1: the surface an agent can call never promotes, signs, demotes or deletes.",
    "details": {"verb": "POST /v2/workareas/{id}/transitions", "role": "writer"},
    "retryable": false
  },
  "request_id": "req_01J…"
}
```

| Field | Meaning |
|---|---|
| `code` | One of the codes below, `family.name`. Stable. |
| `message` | Human-readable instance description. Unstable; never parsed. |
| `rule` | The design rule the refusal names, quoted or paraphrased. Stable per code. |
| `details` | Code-specific structured fields (listed per code where they matter). |
| `retryable` | `true` only where retrying the identical request can succeed later. Retryable responses carry a `Retry-After` header (seconds). |
| `request_id` | Mirrors the `X-Request-Id` response header, for correlating with the read log and support. |

## `auth` — who is calling

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `auth.missing` | 401 | No `Authorization: Bearer` header. | Every request on either ring authenticates, and so does `/mcp`; only `/healthz`, `/readyz`, the discovery documents and the OAuth endpoints do not. |
| `auth.invalid` | 401 | The key is unknown, malformed, expired or revoked (`details.reason` says which). At `/mcp` the same reasons cover a connection's access token: unknown, expired, a revoked connection, a revoked or expired connector key; a token whose connection's resource is not the current `<issuer>/mcp` answers `reason: unknown`. Only a token of kind `access` authenticates at `/mcp`: a refresh token presented there answers `reason: unknown` and is neither rotated nor counted as reuse. Anywhere else an access token is not a credential and answers `reason: unknown`. | Tenant and identity derive from the key, server-side, or not at all — an unverifiable caller gets nothing (D6); a token is exactly its key ([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d11). |

At `/mcp` both codes carry the RFC 9728 challenge, `WWW-Authenticate: Bearer
resource_metadata="…", scope="mcp"`, with `error="invalid_token"` added to an `auth.invalid`
([HTTP API — connections](http-api.md#connections-oauth-for-hosted-mcp-clients)). The bodies are
unchanged.

## `oauth` — connections

The refusals of the connection flow
([HTTP API — connections](http-api.md#connections-oauth-for-hosted-mcp-clients),
[roles and keys — connections](../architecture/roles-and-keys.md#connections),
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d13). Where they surface decides
their shape. At `POST /oauth/token` and `POST /oauth/register` the body is the RFC's
`{"error", "error_description"}`: `error` is the RFC code in the third column, the one an OAuth
client branches on, and `error_description` begins with the OML code and its rule. At
`/oauth/authorize` a refusal is a redirect to the client carrying `error`, `error_description`,
`state` and `iss` (RFC 9207 §2), except where the third column says *page*, and for a parameter
past its bound: then nothing is redirected, and the `text/html` page names the code. The HTTP
column is the status when the code is answered as a body or a page; a refusal redirected from
`/oauth/authorize` is a `302` (`GET`) or a `303` (`POST`) to the client, and its status is that of
the redirect; and `oauth.client_invalid` answering a token request that sent an `Authorization`
header is `401` with `WWW-Authenticate` naming the scheme the request used (RFC 6749 §5.2).
`oauth.connect_refused` answers on `/v2` in the envelope above, and
`oauth.attach_refused` as an `/mcp` tool result in the same envelope.
Malformed parameters on the OAuth endpoints are `request.validation`, carried as
`invalid_request` or as the RFC's own name for the fault (`unsupported_grant_type`,
`unsupported_response_type`, `invalid_scope`, `invalid_client_metadata`).

| Code | HTTP | OAuth `error` | Fires when | The rule it names |
|---|---|---|---|---|
| `oauth.client_invalid` | 400 | *page* at authorize; `invalid_client` at token | At authorize: the `client_id` is unknown; a document URL is not in the admitted form (`reason: client_id_form`); its metadata document cannot be fetched within the fetch bounds, or a fetch cap is full with no cached copy to serve (`reason: fetch_busy`); the document is not JSON, lacks `client_id`, `client_name` or `redirect_uris`, names a `client_id` other than its own URL, or asks for client authentication other than `none` (`details.reason` in the description). At token: only a request that sends client authentication — an `Authorization` header (`401`, above) or a `client_secret` in the body (`400`) ([HTTP API](http-api.md#connections-oauth-for-hosted-mcp-clients)). | Clients are public and identified by a verified document or a registration; a client the server cannot identify is never shown to a person, and a connection keeps its client for its whole life (ADR-0031 d5). |
| `oauth.redirect_refused` | 400 | *page* at authorize; `invalid_redirect_uri` at register | A redirect URI is outside the allowlist, or an authorize request's `redirect_uri` is not exactly one of the client's registered URIs. | An authorization code goes only to `https://claude.ai/api/mcp/auth_callback`, by exact match; a refused redirect is never followed (ADR-0031 d6; RFC 6749 §4.1.2.1). |
| `oauth.pkce_required` | 400 | `invalid_request` | An authorize request without `code_challenge`, or whose `code_challenge_method` is absent (RFC 7636 §4.3 reads that as `plain`) or not `S256`; redirected once the client and the redirect URI are accepted. A `code_challenge` that is present but does not match `^[A-Za-z0-9_-]{43}$` is `request.validation` on the page. | Every authorization is bound by S256 PKCE to the client that started it (ADR-0031 d4). |
| `oauth.resource_mismatch` | 400 | `invalid_target` | A `resource` parameter that is not exactly `<issuer>/mcp`, at authorize or token. | A token is issued for the one protected resource and accepted nowhere else (ADR-0031 d10; RFC 8707 §2). |
| `oauth.authorization_expired` | 400 | `access_denied`; *page* when the handle is unknown | The authorization request outlived its ten minutes or spent its five attempts; the form's handle does not open under the lead's sealing key or names no live request, a restart or takeover of the lead included; or a confirmation's handle names no row, because Connect or Cancel already used it, or its code is voided ([roles and keys — connections](../architecture/roles-and-keys.md#connections)). | A page acts only on a live request it rendered, for a bounded number of tries (ADR-0031 d9). |
| `oauth.connect_code_invalid` | 400 | *page* — the code form again, with the attempts left | The connect code entered is unknown, expired or already spent, or belongs to a revoked or expired key — one answer for all four. | A connect code is single-use and short-lived, and the page never tells a guesser that a code existed (ADR-0031 d8, d9). |
| `oauth.authorization_code_invalid` | 400 | `invalid_grant` | The authorization code is unknown, expired, already redeemed (which revokes its connection only under the reuse rule of [roles and keys — connections](../architecture/roles-and-keys.md#connections)), issued to another client, its PKCE verifier does not match, its `redirect_uri` does not follow the token endpoint's rule ([HTTP API](http-api.md#connections-oauth-for-hosted-mcp-clients)), its connect code is voided, its key is revoked or expired, or its connection's resource is not the current `<issuer>/mcp`. | An authorization code is redeemed once, by the client that holds the verifier, within its sixty seconds (ADR-0031 d9, d10; RFC 6749 §4.1.2). |
| `oauth.refresh_invalid` | 400 | `invalid_grant` | The refresh token is unknown or expired, is not a token of kind `refresh` (an access token sent as one included), was already rotated — which revokes the connection — belongs to a revoked connection, was issued to another client, belongs to a revoked or expired key, or belongs to a connection whose resource is not the current `<issuer>/mcp`. | A refresh token is used once; a reused one ends its connection (ADR-0031 d10). |
| `oauth.origin_refused` | 403 | *page* | A `POST /oauth/authorize` carries neither `Sec-Fetch-Site: same-origin` nor, without that header, an `Origin` equal to the issuer's — `Origin: null` included. Never redirected; it consumes no attempt. | The page acts only on its own form; a cross-site submission is refused before its fields are read (ADR-0031 d9). |
| `oauth.attach_refused` | 403 | — (`/mcp` tool result) | `oml_attach` with `action: "open"` under a connection, refused before any upload session is created. | An upload's parts travel on `/v1` under a key value, which a connector key does not have, and a token is a credential only at `/mcp`; a connection opens no session it could never fill (ADR-0031 d11). |
| `oauth.connect_refused` | 422 | — (`/v2` envelope) | A connect code is asked for a key that cannot hold a connection: an `admin` or `operator` role, `held_by: service`, a key with a bearer value, a revoked or expired key, or a deployment without `oauth.issuer`; or a connector key is rotated (`details.reason` ∈ `role` \| `held_by_service` \| `bearer_key` \| `revoked` \| `expired` \| `not_configured` \| `connector_key`). | A connection acts as one agent key on the agent-safe surface; a key that cannot act there, or already has a value, gets no connect code, and a connector key never gains a value by rotation (ADR-0031 d7, d8). |

The one throttle on these endpoints is a cap on stored state: the cap on unused dynamic
registrations answers `429 limit.rate` at `POST /oauth/register` in the same RFC shape, as
`invalid_request` with `Retry-After`. An authorization request stores nothing until its code
verifies, and a full document cache evicts rather than refuses, so `GET /oauth/authorize` answers
no `429`. No limit keys on a source address (ADR-0031 d5, d9, d14).

## `access` — what this key may do

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `access.verb_ceiling` | 403 | A key's **role** may never call this verb: any agent key (reader/writer/dreamer) calling an admin verb — re-parent, keys, grants, bans, simulations, reassign, demote, erase, tip-override, export, settings (a reader or writer key proposing a status transition is also this code; a dreamer proposing or ratifying one is not — transitions are dreamer-or-admin verbs, D8); an admin key posting a verdict (admin ≠ reviewer); an operator key touching content; a verb whose needed grant role the key's role does not admit (step 1 of the [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) — a reader key submitting, a writer key reviewing); a `held_by: service` key calling any review-class verb (review queue, review next, verdict, reopen, claim-edge ratify, workarea-edge ratify) or status-transition ratify, refused before any lookup ([the role each verb needs](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)). | **R1: the surface an agent can call never promotes, signs, demotes or deletes** — the verb ceiling is per role (D9), and no claimed purpose lifts it. |
| `access.grant_refused` | 403 | The node is readable — reader is among the key's effective roles there — but its effective access (grants minus denies, at the current permission epoch) does not confer the verb's role on this workarea (step 3 of the [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)). `details`: `{workarea_id, needed_role}`. | Effective access is the union of grants, each attenuated by its denies, deny-wins for global denials; what a grant does not confer, the key does not have (D7). |
| `access.not_found` | 404 | The resource does not exist, **or** lies outside the key's effective read scope, **or** is a simulation row and the key is outside that simulation's audience. It also answers a verb on a node where the key holds neither the needed role nor reader (step 4 of the [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)), a supersede or `derives_from` target the key cannot read, a listing `workarea_id` the key may not name ([Search](http-api.md#search-get-v1memories)), and a fetch of an erased version the key could not read before its erasure; every supersede refusal that depends on the target comes after this check (step 5 of the supersede rule). Indistinguishable by design. | Reads fail closed as absence: the service never confirms the existence of what a key may not see (D7, D10, R3). |
| `access.author_only` | 403 | A writer key proposes a supersession of a readable target version it did not author — [own](../architecture/roles-and-keys.md#own) is read on the target version, the current tip, not on the claim's first version; an unreadable target is `access.not_found`. | A writer proposes supersessions of its own claims only; editing another author's claim is the dreamer's edit-as-proposal path (D9). |
| `access.issuer_ceiling` | 403 | Key issuance (or a grant on it) requests a role above the issuer's, or a grant role outside the key role's admissible set ([roles-and-keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)); an operator key issues anything beyond a tenant's first admin key. `details`: for a key role above the issuer's, `{role, admissible}` — the requested key role and the key roles this issuer may issue; for an inadmissible grant, `{role, admissible}` — the grant's role and the key role's admissible set — plus, at issuance, `grant_index`, the first refused grant's 0-based index in `grants` ([issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)). | A key is issued with a role no higher than the issuer's (D9); authority never amplifies through issuance. |
| `access.role_exclusive` | 403 | Key issuance or a grant amendment would combine the `admin` role with a dreamer-class grant on one key. | Admin is never a reviewer (D9): a key never combines admin with dreamer verbs. A person holding both duties holds two keys, so the journal shows which capacity acted ([roles-and-keys](../architecture/roles-and-keys.md)). |
| `access.banned` | 403 | Admission-time check: the submitting principal or source is under an active ban. | Bans are journalled admin acts checked at every admission from the moment they are imposed (D9); lifting one is an equally journalled act. |

## `request` — the request itself

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `request.validation` | 400 | Malformed JSON, missing or ill-typed fields, unknown enum values, a stale or foreign pagination cursor. Also: an unknown field in a recall, supersede, key-issuance or grant body; `max_tokens` with `include_bodies: false`; an empty `kinds` or `terms` array in a recall; `terms` or `expand_terms` sent to a server that does not yet serve them (refused by name, never ignored); an events-tail `after_seq` beyond `head_seq`; a deny outside its own grant's subtree; a `default_workarea_id` no grant of the key covers; a key issuance by an operator key without `tenant_id`, or by an admin key with it; `held_by` on an operator key's issuance (only an admin attests it); an `X-OML-Workarea` that differs from a supersede target's workarea, or sent with get or the own-work listing (`own=true`), which resolve no workarea; a successor `kind` the [kind-change rule](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip) does not admit; `retirement: true` when the target's kind is `term`, or when a `kind` is sent that differs from the target's; an `oml_supersede` call whose target is of a vocabulary kind (`term`, `term_mapping`, `vocab_release`), which is superseded on the HTTP API. | The contract is [openapi.yaml](openapi.yaml); what it does not admit is refused, not coerced. |
| `request.idempotency_key_required` | 400 | A state-creating `POST` arrives without `Idempotency-Key`. | Every side-effecting request is replayable exactly once by construction (D13); the service refuses to guess whether a retry is a duplicate. |
| `request.payload_too_large` | 413 | Body or part exceeds the admitted size. | Size bounds are admission policy, enforced before bytes are stored. |
| `request.unsupported_media` | 415 | The request `Content-Type` is not supported by this operation (`details.supported` names what is). | Each operation's request media type is fixed by the contract — `application/json` everywhere except attachment part upload (`application/octet-stream`); a body in any other type is refused before parsing, never sniffed into shape. |
| `request.workarea_unresolved` | 422 | A provider-ring (`/v1`) submit, recall or non-own listing arrives from a key with no journalled `default_workarea` and no `X-OML-Workarea` header — get, the own-work listing (`own=true`) and supersede never answer it: get and `own=true` resolve no workarea, and supersede takes the target's workarea; or an events tail without `workarea_id`, or an MCP tool whose omitted workarea defaults to the key's, from a key with no default ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d14, d15). | The target workarea comes from the key ([ADR-0022](../adr/ADR-0022-two-ring-api.md) d4): a `/v1` request names no workarea — the key's `default_workarea` is the target, and `X-OML-Workarea` (a node id, honored only within the key's grants) is the explicit override. A key with neither is refused, never guessed for. |

Response media types are not negotiated in v1: servers emit `application/json` (the binary
downloads excepted, [http-api.md](http-api.md)), and the `Accept` header never varies a response.

## `idempotency` — the replay contract

Semantics in full on the [HTTP API page](http-api.md#idempotency); the record is
`(key_id, workarea, idem_key) → request digest + state + stored response`, claimed before any side
effect (D13). On the provider ring the workarea component is the **resolved** target — the key's
`default_workarea` or the `X-OML-Workarea` override (ADR-0022 d4); for supersede it is the fixed
nil value, as on lab endpoints that name no workarea
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d10). A supersede record is looked
up after step 1 of the
[supersede rule](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) and
before step 2, so a committed one replays its stored response whatever the target's state or the
key's access now, and it is claimed only after step 2 passes, so a refusal at step 1 or 2 claims
no record. The request digest covers the method, the route template and the path parameters as
well as the body ([http-api](http-api.md#idempotency)), so one `Idempotency-Key` reused for two
supersede targets is `idempotency.digest_mismatch`, never a replay of the first.

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `idempotency.in_progress` | 409 | A replay arrives while the record is `in_progress` — the first attempt has not committed or failed yet. **Retryable**, with `Retry-After`. | The record is claimed in its own transaction before side effects, so two racing attempts can never both execute (D13). |
| `idempotency.digest_mismatch` | 409 | Same `(key, workarea, idem_key)`, different request digest. | An idempotency key names exactly one request; replay returns the stored response, a *different* request under the same key is refused — never silently honored as new work (D13). |

A successful replay is not an error: it answers the stored response with header
`Idempotency-Replay: true`.

## `submission` — admission refusals

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `submission.scan_veto` | 422 | The deterministic scan, running in the same call as capture, vetoes the submission (`details.finding` names the rule class that fired). The veto is a refusal and journals nothing — the verdict is reproducible from the journalled rule-set pin (a durable veto trace is RS-16). | The scan is rules, not a model: same input, same verdict, reproducible from the journalled rule-set version — and it can veto but never promote (D11; [admission](../architecture/admission.md)). |
| `submission.attachment_missing` | 422 | The submission binds an attachment digest with no completed upload session owned by this key. | The claim and its files are one signed unit; the envelope binds only bytes the service already holds and has digest-verified (D3). |
| `submission.simulation_id_required` | 422 | `derives_from` touches a simulation row but the submission does not carry that row's `simulation_id`. | Taint propagates and nesting is forbidden: a derivation from a simulation row carries that simulation's id, server-enforced, so one end-act demotes the closure (D10). |
| `submission.lineage_required` | 422 | A `verb: "learn"` submission arrives with an empty or omitted `derives_from`. | `learn` asserts a derived claim, so its lineage is required, never optional; a derived conclusion whose author cannot or will not name lineage is submitted as `remember` with `kind: derived_fact` ([http-api](http-api.md)). |
| `submission.tip_conflict` | 409 | A supersession's target is not the current served tip — at submission, or at verdict time when the target is no longer the tip because a competing successor was ratified first, or when the target or the successor was moved to another workarea or re-keyed so that the two no longer share a workarea and `claim_key` (the verdict is refused, nothing is journalled, and its serve is not consumed). `details.current_tip`: `{artifact_id, version}`, present only when the caller can read it. | Supersession promotion CAS-checks its target is the current tip; the tip flip is one journalled fact, never a lost update ([postgres-schema](../data/postgres-schema.md)). |
| `submission.claim_key_conflict` | 409 | A new claim (version 1), or a reopen, would put a second live version under one `claim_key` and ordinal in a workarea — a version-1 capture that lost the ordinal race to a concurrent one included, which is never renumbered (below) — when the key is released and which reopens collide is declared on [audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip); or a reassignment would collide at its destination without recording a re-key. | A `claim_key` names one live claim per (tenant, workarea) ([audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)); a colliding move is refused unless the journalled move event records the re-key (D7). |
| `submission.universe_refused` | 409 | A claim write addresses the tenant's `universe` root. | Universe is permanently `committed` governance structure and refuses direct claim writes — claims live in crew workareas (D7). |

A capture that loses the race for a version ordinal to a concurrent capture never surfaces as a
constraint error: the server retries it, re-running every read the capture makes before `BEGIN`,
the `claim_key` collision test included, so a version-1 capture that lost the race answers
`submission.claim_key_conflict` and is never renumbered, and only a successor takes a fresh
ordinal ([audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).

## `workarea` — structure and status refusals

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `workarea.frozen` | 409 | A `validated` workarea is asked to accept what freeze forbids: a new write, a promotion into it, a reassignment into it, a new child under it, or a re-parent into it. | `validated` is frozen: the only mutations it admits are demote, erasure, and crew-metadata update, each journalled (D8). |
| `workarea.transition_refused` | 409 | A backward transition is requested (none exist); a rung is skipped (`validated` proposed from `development`); `validated` is proposed while a child is not validated; any transition is proposed on `universe`. | The status ladder moves forward only — development → committed → validated — and an area becomes validated only when all its children are (D8). |

The two-party ceremony around transitions — proposer plus a second, non-proposing ratifier — is
refused through `review.ratifier_is_proposer` below, because it is the same independence rule.

## `review` — independence and the serve binding

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `review.author_in_lineage` | 403 | `review/next` would serve, or a verdict/ratification/reopen would count, for a key or principal that appears anywhere in the row's derivation lineage closure — pending **and** ratified edges, computed at that moment — or, for a held successor, that authored a rival held successor of the same target ([rival successors](../architecture/roles-and-keys.md#the-dreamer-split)). The original author reopening its own rejection is this code. | **R2: the reviewer is never the author** — enforced at promotion time against the author and the lineage closure, not just at key issuance. "I edited an ancestor" disqualifies exactly as "I wrote it" does ([roles-and-keys](../architecture/roles-and-keys.md)). |
| `review.ratifier_is_proposer` | 403 | The key ratifying a workarea edge, claim edge, or status transition is the key or principal that proposed it. | R2 extends to edges and transitions: proposal and ratification are two identities or they are nothing (D7, D8). |
| `review.serve_invalid` | 409 | A verdict names a serve that is expired, already consumed by its first verdict, bound to a different reviewer key, or invalidated because the served content changed (`details.reason` ∈ `expired` \| `consumed` \| `reviewer_mismatch` \| `content_changed` \| `unknown`). | A verdict counts only for exactly what was served: the serve is bound server-side to (reviewer key, artifact version, attachment digest set), and the journalled verdict records the served digests (D14; [security](../architecture/security.md#serve_id-binding-as-anti-replay)). |

## `simulation` — the fence

Reads of simulation rows by non-audience keys are `access.not_found` — the fence hides, it does
not advertise. The codes below fire where the caller has already named the simulation.

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `simulation.audience_refused` | 403 | A key outside the simulation's journalled audience writes with that `simulation_id`, or posts a verdict on one of its rows. | Simulation rows are written, read and reviewed only by keys in the audience; a simulation with no dreamer in its audience can promote nothing — fail closed (D10). |
| `simulation.ended` | 409 | A write or verdict names a simulation that has ended. | Ending a simulation is a journalled act that demotes its rows; nothing accretes to a closed channel (D10). |
| `simulation.taint_refused` | 409 | A promotion or reassignment would place a simulation row, or a claim whose lineage closure touches one (sim-derived), into a `validated` workarea. | Taint propagates and simulation rows never enter validated workareas (D10). |
| `simulation.tip_refused` | 409 | A promotion, supersession ratification or tip-override would make a `provenance: simulation` row or sim-derived claim the default served tip of a claim lineage. | Taint never captures the default tip: tainted versions shadow the tip only for the simulation's audience as an overlay tip; the default tip of a real claim is bit-for-bit indifferent to every simulation (D10). |

## `vocab` — vocabulary structure refusals

The vocabulary surface ([http-api.md](http-api.md#vocabularies);
[ontologies](../architecture/ontologies.md), [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)) adds four refusals. The scanner **finding
classes** (`vocab.term_pending`, `vocab.deprecation_replaced_by`, `vocab.mapping_exactMatch`,
`vocab.deprecated_successor`) are *not* in this taxonomy: they are review-policy vocabulary —
deterministic scan findings a workarea's `human_required_classes` may name — and live with the
admission policy docs ([admission](../architecture/admission.md)), never as API refusals.

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `vocab.cycle_refused` | 409 | Ratifying an `is_a` or `replaced_by` claim edge would close a cycle over the ratified edges plus the candidate. | Term hierarchy and supersession chains are acyclic by **domain invariant** — a deterministic transaction-time check at ratification, like the supersession tip-CAS; it narrows, so it is constitution-compatible (ADR-0021). |
| `vocab.prefix_conflict` | 409 | `workarea.create` carries a `prefix` already registered in this tenant's prefix registry. | A prefix is registered once, tenant-unique, and is permanent — a minted CURIE is never re-mintable with a different meaning (ADR-0021). |
| `vocab.unknown_curie` | 422 | A cited CURIE (a `terms` annotation under `enforcement: require`) names no term readable by this key. Vocab reads never answer this code — a read answers `access.not_found` or an empty match, by the fail-closed-as-absence rule. | CURIE citations fail closed as absence within the writer's effective read scope, exactly as `derives_from` targets do (D7); unknown and out-of-scope are indistinguishable. |
| `vocab.inactive_curie` | 422 | A cited term's served tip is deprecated, under `enforcement: require` (under `annotate`, the citation records a finding and never blocks). `details.successors` lists successor CURIEs and `details.relation` names the ratified pointer edge — `replaced_by` (safe for automated rewiring) or `consider` (requires judgment) — so a client self-heals in one hop. | Deprecation is supersession, never deletion: the successor pointer is a ratified claim edge, and the refusal hands it over rather than dead-ending the citer (ADR-0021). |

## `upload` — attachment sessions

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `upload.session_invalid` | 409 | A part or complete addresses a session that is expired, already complete, or aborted (`details.reason` says which). | A retry resumes or restarts the session — the behavior is the session state machine's, not improvised per client (D3). |
| `upload.digest_mismatch` | 422 | At complete, the assembled bytes do not hash to the session's `expected_digest`. | An attachment is named by the SHA-256 of its plaintext and bound into the signed envelope by that digest; bytes that are not what was promised are never stored as if they were (D3). |

## `lifecycle` — erased content

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `lifecycle.erased` | 410 | A fetch addresses an artifact whose DEK was destroyed by a journalled erase, from a key that could read the version immediately before its erasure — its author's key set ([own](../architecture/roles-and-keys.md#own)), or, when the version was promoted then, a key with reader effective access on its workarea under every other read predicate. Readability is checked first: every other key gets `access.not_found`. | **R4: no silent deletion** — erasure is a journalled crypto-shred scoped to body and attachment bytes; the journal trail and attribution remain and the response says *erased*, never *not found* (D4). |

`410` deliberately differs from `404`: erasure is an attested act with an inventory, visible to
anyone who could read the artifact before. What a key could never see stays `access.not_found`,
so the erasure of another author's held or never-promoted version tells no other key that it
existed.

## `limits`

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `limit.rate` | 429 | The key or tenant exceeds its journalled rate-limit grant attribute. **Retryable**, with `Retry-After`. | Rate limits are journalled grant attributes (D6). v1 enforcement is per-replica token buckets at limit/N — a documented approximation, [FR-8](../product/roadmap.md); a burst may transiently pass, a refusal is always honest. |
| `limit.quota` | 429 | A journalled quota attribute (e.g. a storage or submission budget) is exhausted. **Retryable** only where the budget replenishes; `details.quota` names the attribute. | Quotas are journalled grant attributes, so every enforcement decision is reproducible from the journal (D6). |

## `export`, `settings`

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `export.not_ready` | 409 | The bundle is fetched before the export job reaches `done`. | An evidence export is an async, journalled job; the bundle exists only when its chain verification has finished ([audit-and-lineage](../architecture/audit-and-lineage.md)). |
| `settings.retired_key` | 422 | A settings document names a retired settings key (`details.key`, `details.replacement`). | Retired keys are refused **by name**, with the setting that replaced them — silently ignoring unknown keys is how configuration rots (D14). |
| `settings.invalid` | 422 | `settings/apply` finds any validation error. | Apply is validate-then-journal-then-apply, refused whole on any error; there is no force flag (D14). |

## `system`

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `system.internal` | 500 | A defect. `request_id` is the handle for the operator. | — |
| `system.unavailable` | 503 | The replica cannot serve safely: Postgres unreachable, or writes are refused because the startup fork check has not verified the stored chain head against the last signed head (`details.reason`). **Retryable.** | A replica that cannot prove its journal head accepts no writes (D1); `/readyz` reports the same condition to the orchestrator ([deployment](../operations/deployment.md)). |

Review *unavailability* — a workarea whose policy cannot currently be satisfied by any eligible
reviewer — is deliberately **not** an API error: submissions still land `held`, wait, and expire
per the policy's `held_item_expiry` (D11 fails closed by holding, not by refusing). The condition
is surfaced by `oml status` / `oml doctor`, not by this taxonomy.

## Status-code index

| HTTP | Codes |
|---|---|
| 400 | `request.validation`, `request.idempotency_key_required`, `oauth.client_invalid`, `oauth.redirect_refused`, `oauth.pkce_required`, `oauth.resource_mismatch`, `oauth.authorization_expired`, `oauth.connect_code_invalid`, `oauth.authorization_code_invalid`, `oauth.refresh_invalid` |
| 401 | `auth.missing`, `auth.invalid` |
| 403 | `oauth.origin_refused`, `oauth.attach_refused`, `access.verb_ceiling`, `access.grant_refused`, `access.author_only`, `access.issuer_ceiling`, `access.role_exclusive`, `access.banned`, `review.author_in_lineage`, `review.ratifier_is_proposer`, `simulation.audience_refused` |
| 404 | `access.not_found` |
| 409 | `idempotency.in_progress`, `idempotency.digest_mismatch`, `submission.tip_conflict`, `submission.claim_key_conflict`, `submission.universe_refused`, `workarea.frozen`, `workarea.transition_refused`, `review.serve_invalid`, `simulation.ended`, `simulation.taint_refused`, `simulation.tip_refused`, `vocab.cycle_refused`, `vocab.prefix_conflict`, `upload.session_invalid`, `export.not_ready` |
| 410 | `lifecycle.erased` |
| 413 | `request.payload_too_large` |
| 415 | `request.unsupported_media` |
| 422 | `request.workarea_unresolved`, `oauth.connect_refused`, `submission.scan_veto`, `submission.attachment_missing`, `submission.simulation_id_required`, `submission.lineage_required`, `vocab.unknown_curie`, `vocab.inactive_curie`, `upload.digest_mismatch`, `settings.retired_key`, `settings.invalid` |
| 429 | `limit.rate`, `limit.quota` |
| 500 | `system.internal` |
| 503 | `system.unavailable` |

`oauth.client_invalid` is also a `401`, at `POST /oauth/token`, for a request that sent an
`Authorization` header ([`oauth`](#oauth-connections)).

## Implementation mapping

In the planned workspace ([workspace-plan](../engineering/workspace-plan.md)), each domain refusal
is a typed variant in `oml-domain` — the pure core refuses in types, not in strings. The
`code`/`rule`/HTTP-status/`retryable` attachment happens once, at the `oml-server` edge, where the
HTTP transport maps domain variants into this envelope. The tables on this page are the source
both the OpenAPI `ErrorCode` enum and that Rust mapping are checked against:
`scripts/check_contract_sync.py` enforces the OpenAPI side today, and a Rust-side exhaustiveness
check joins it once code exists. A variant with no code here, or a code here with no variant, is
the same documentation bug the opening paragraph names.
