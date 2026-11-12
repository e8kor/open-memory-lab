---
status: Stable
verified: 2026-11-06
answers: "which typed error codes exist, their family, HTTP status and retryability, and how an error response is shaped"
---

# Errors

Every refusal is one of the codes below. The list is closed and matches the `ErrorCode` enum in [openapi.yaml](openapi.yaml). Branch on `code`, never on `message`.

## Reading an error

Every non-2xx response has one envelope.

```json
{
  "error": {
    "code": "access.verb_ceiling",
    "message": "a writer key cannot transition a workarea",
    "rule": "the surface an agent can call never promotes, signs, demotes or deletes",
    "details": {"verb": "POST /v2/workareas/{workarea}/transitions", "role": "writer"},
    "retryable": false
  },
  "request_id": "req_01J..."
}
```

| Field | Meaning |
|---|---|
| `code` | `family.name`. Stable. |
| `message` | A description of this instance. Unstable. Never parse it. |
| `rule` | The rule that refused the request. Stable per code. |
| `details` | Fields specific to the code, listed where they matter. |
| `retryable` | `true` only where the identical request can succeed later. Retryable responses carry `Retry-After`, in seconds. |
| `request_id` | The `X-Request-Id` response header, for finding the request in the server log. |

On the agent plugin, a refusing tool answers a JSON-RPC result with `isError: true`. The envelope is in `structuredContent` and in the one text content. The CLI prints `refused (<status>): <code>` and exits 1. The SDKs raise `ApiError` with `code`, `message` and `details`.

Where OML cannot establish that an act is permitted, the act does not happen. A read never reveals what the key may not see.

## `auth` — who is calling

Every request except `/healthz`, `/readyz`, the discovery documents and the OAuth endpoints authenticates.

| Code | HTTP | Fires when |
|---|---|---|
| `auth.missing` | 401 | No `Authorization: Bearer` header. |
| `auth.invalid` | 401 | The key or token is unknown, malformed, expired or denied. `details.reason` is `unknown` for keys, and for a capacity credential (`omls_`) whose web session has ended. It is `session_outside_web` for a capacity credential on `/mcp`, or on `/v1` and `/v2` without `oml-web`'s client identity, or beside a client certificate a gateway forwarded. On web pages it also carries `not_allowed`, `email_unverified`, `state_mismatch`, `identity_changed`, `provider_refused` and `session_expired`. |

## `oauth` — connections

The refusals of the connection flow for claude.ai and phone apps. At `POST /oauth/token` and `POST /oauth/register` the body is the RFC's `{error, error_description}`. At `/oauth/authorize` a refusal is a redirect to the client, or a page where the table says so.

| Code | HTTP | Fires when |
|---|---|---|
| `oauth.client_invalid` | 400 | The `client_id` is unknown, its metadata document is not admissible, or a token request sent client authentication. A token request with an `Authorization` header answers 401. |
| `oauth.redirect_refused` | 400 | The redirect URI is outside the allowlist or not one of the client's registered URIs. |
| `oauth.pkce_required` | 400 | The authorize request has no `code_challenge`, or its method is not `S256`. |
| `oauth.resource_mismatch` | 400 | `resource` is not exactly `<issuer>/mcp`. |
| `oauth.authorization_expired` | 400 | The request outlived ten minutes or spent five attempts, or its handle names no live request. |
| `oauth.connect_code_invalid` | 400 | The connect code is unknown, expired, voided, used, or belongs to a denied key. One answer for all five. |
| `oauth.authorization_code_invalid` | 400 | The authorization code is unknown, expired, already redeemed, issued to another client, or its PKCE verifier does not match. |
| `oauth.refresh_invalid` | 400 | The refresh token is unknown, expired, already rotated (which ends the connection), or issued to another client. |
| `oauth.origin_refused` | 403 | A `POST /oauth/authorize` came from another origin. It uses no attempt. |
| `oauth.attach_refused` | 403 | Reserved. This build does not return it. |
| `oauth.connect_refused` | 422 | The key cannot hold a connection. `details.reason` is `role`, `held_by_service`, `bearer_key`, `revoked`, `expired`, `not_configured`, `connector_key`, `session_key` or `not_confined`; the last is a code that lives past ten minutes asked for a key that reaches more than one workarea, the universe or a user root. |

## `access` — what this key may do

A key's role, grants and denials decide these. Reads fail closed as absence.

| Code | HTTP | Fires when |
|---|---|---|
| `access.verb_ceiling` | 403 | The key's role never calls this verb, whatever its grants. Examples: an agent key calling an admin verb, an admin key posting a verdict or reading memory text, a `held_by: service` key reviewing, any web-page request carrying an `Authorization` header. `details.reason` `session_key_only`: a person's own act (listing their keys, ending their session, the export, the deletion request) sent by any key but their web session key, an agent's key naming them included. `details.reason` `session_capacity`: a key operation sent as a person's session key of another capacity than the one it serves, such as `POST /v2/keys` as `<user>-admin`. `details.reason` `federation_is_human_only`: a workarea's federation field set by a key other than a person's reviewer key or an admin key. |
| `access.grant_refused` | 403 | The key can read the workarea, but its effective access does not confer the verb's role there. `details`: `workarea_id`, `needed_role`. |
| `access.not_found` | 404 | The thing does not exist, or is outside the key's read scope. The two cases look the same. Also answers a name that resolves to nothing the key may see. |
| `access.author_only` | 403 | A writer key tried to supersede a version it did not author. |
| `access.issuer_ceiling` | 403 | Key issuance or a grant asks for a role above the issuer's, or a grant role the key role does not admit. |
| `access.role_exclusive` | 403 | Issuance or a grant change would combine the `admin` role with a reviewer grant on one key. Use two keys. |
| `access.banned` | 403 | Reserved. This build does not return it. |

## `request` — the request itself

Malformed or unsupported requests. `details.field` names the refused value: a path or query parameter, a header name, or an [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901) JSON Pointer for a body member.

| Code | HTTP | Fires when |
|---|---|---|
| `request.validation` | 400 | Malformed JSON, a missing or mistyped field, an unknown field or enum value, a stale cursor. A renamed field also carries `details.renamed_to`. |
| `request.idempotency_key_required` | 400 | A state-creating `POST` has no `Idempotency-Key`. |
| `request.payload_too_large` | 413 | The body exceeds the admitted size. See [limits](../reference/limits.md). |
| `request.unsupported_media` | 415 | The `Content-Type` is not supported. `details.supported` names what is. |
| `request.workarea_unresolved` | 422 | The call names no workarea and the key has no default workarea. |

## `idempotency` — the replay contract

See [idempotency](http-api.md). A successful replay is not an error. It answers the stored response with `Idempotency-Replay: true`.

| Code | HTTP | Fires when |
|---|---|---|
| `idempotency.in_progress` | 409 | A replay arrived while the first attempt is still running. Retryable, with `Retry-After`. |
| `idempotency.digest_mismatch` | 409 | The same `Idempotency-Key` was sent with a different request. |

## `submission` — admission refusals

Refusals at write time. A refused write stores nothing.

| Code | HTTP | Fires when |
|---|---|---|
| `submission.format_invalid` | 422 | The source fails its format's adapter: `details.reason` names the rule (`mermaid.directive_refused`, `mermaid.interaction_refused`, `mermaid.styling_refused`, `mermaid.html_label_refused`, `mermaid.unsupported_diagram`, `mermaid.syntax_error`, `mermaid.too_many_nodes`, `mermaid.too_many_relations`, `json.invalid`, `json.duplicate_key`, `yaml.invalid`, `yaml.alias_refused`, `yaml.tag_refused`, `yaml.merge_refused`, `yaml.complex_key`, `yaml.second_document`, `yaml.duplicate_key`, `yaml.empty`, `format.bidi_control`, `format.too_large`, `format.too_deep`, `format.too_many_scalars`), `details.line` and `details.column` where it was found. Nothing is journalled and no row is stored. | The source is the signed text, so it must be a document its format accepts; a refusal names the rule so the writer corrects it in one retry. |
| `submission.scan_veto` | 422 | The scan vetoed the write. `details.finding` names the class, the rule and where, never the matched text. |
| `submission.attachment_missing` | 422 | Reserved. This build does not return it. |
| `submission.simulation_id_required` | 422 | Reserved. This build does not return it. |
| `submission.lineage_required` | 422 | A `learn` write has an empty `derives_from`. |
| `submission.tip_conflict` | 409 | The supersession target is not the current tip. `details.current_tip` names it. |
| `submission.version_conflict` | 409 | `expected_version` is not the latest version the caller can read. `details.current_version` names it. |
| `submission.claim_key_conflict` | 409 | A new memory would put a second live version under one `claim_key` in the workarea. |
| `submission.similar_exists` | 409 | The write sent `on_similar: refuse` and a readable memory already says the same. `details.similar` lists up to three. Resend with `hold`. |
| `submission.universe_refused` | 409 | A write addressed the tenant's `universe` root from a key that may not write there. |
| `submission.user_root_refused` | 409 | A new memory, a successor or a retirement addressed a user's root, or a workarea beneath it, from a key that is not that user's. Answered where the key reads the workarea or the target; a key that reads neither gets `access.not_found`. |

## `name` — unique names

A name is accepted wherever an id is. An unresolved name is `access.not_found`, exactly as an unknown id would be. See [names and ids](http-api.md).

| Code | HTTP | Fires when |
|---|---|---|
| `name.taken` | 409 | A creation names a name its kind already holds, including a denied key's. `details`: `kind`, `name`. |

## `workarea` — structure and status

Refusals from the workarea tree and its status ladder.

| Code | HTTP | Fires when |
|---|---|---|
| `workarea.frozen` | 409 | A `validated` workarea was asked to accept a new write, child, promotion or move. |
| `workarea.transition_refused` | 409 | The status change skips a rung, goes backward, or breaks a rule such as a child not being validated. |
| `workarea.in_progress_refused` | 409 | The in-progress mode or change conflicts with the node's subtree, ancestors or status. `details.reason` says which. |
| `workarea.held_remaining` | 409 | The commit of an in-progress workarea was asked while a version in its subtree is held. |
| `workarea.admin_key_under_root` | 409 | A person's disable, or the deletion close that disables them, met a live admin key whose row names their user root. No verb issues one and no denial cuts one off, so nothing is journalled. `details`: `user`, `key`. Clear the key row's `user_root_id`, then send again ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d8). |

## `review` — independence and the serve binding

The reviewer is never the author.

| Code | HTTP | Fires when |
|---|---|---|
| `review.author_in_lineage` | 403 | The reviewer, or its principal, appears in the memory's derivation lineage, or authored a rival successor. |
| `review.ratifier_is_proposer` | 403 | The key ratifying an edge or status change is the one that proposed it. |
| `review.serve_invalid` | 409 | The served item expired, was used, was served to another key, or changed. `details.reason`: `expired`, `consumed`, `reviewer_mismatch`, `content_changed` or `unknown`. |

## `simulation` — reserved

This build serves no simulations and returns none of these codes.

| Code | HTTP | Fires when |
|---|---|---|
| `simulation.audience_refused` | 403 | Reserved. |
| `simulation.ended` | 409 | Reserved. |
| `simulation.taint_refused` | 409 | Reserved. |
| `simulation.tip_refused` | 409 | Reserved. |

## `vocab` — vocabulary structure

Refusals from vocabulary workareas and their terms.

| Code | HTTP | Fires when |
|---|---|---|
| `vocab.cycle_refused` | 409 | Ratifying an `is_a` or `replaced_by` edge would close a cycle. |
| `vocab.prefix_conflict` | 409 | The prefix is already registered in the tenant. |
| `vocab.unknown_curie` | 422 | A cited term does not exist or is not readable by the key. |
| `vocab.inactive_curie` | 422 | A cited term is deprecated. `details.successors` and `details.relation` name the replacement. |

## `upload` — attachment sessions

Reserved. This build serves no attachment uploads.

| Code | HTTP | Fires when |
|---|---|---|
| `upload.session_invalid` | 409 | Reserved. |
| `upload.digest_mismatch` | 422 | Reserved. |

## `lifecycle` — erased content

| Code | HTTP | Fires when |
|---|---|---|
| `lifecycle.erased` | 410 | The fetched version's key was destroyed by a journaled erase, and this key could read it before. Every other key gets `access.not_found`. |

## `limit` — rate limits

| Code | HTTP | Fires when |
|---|---|---|
| `limit.rate` | 429 | Unused OAuth client registrations passed 10,000, or a read's `feedback` would pass 1,000 labels in a UTC day. A key of a person of the sign-up tenant past its requests per minute is `limit.rate` with `details.quota: requests_per_minute`; sign-ups from one client address past the hour's limit are `limit.rate` too ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d5, d9). Retryable, with `Retry-After`. |
| `limit.quota` | 429 | A person of the sign-up tenant is past a quota of memories, keys or connections, with `details: {quota, limit, used}`. Nothing is journaled, and an admin raises the limit on `/web/admin/users` ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d5). Any account on a quota profile past `memories`, `text_bytes`, `writes_per_month`, `keys` or `connections` is refused by the write's own commit, so two writers at the limit never both pass ([ADR-0071](../adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d3, d5). |

## `export` and `settings`

Reserved. This build serves neither an evidence export nor a settings apply.

| Code | HTTP | Fires when |
|---|---|---|
| `export.not_ready` | 409 | Reserved. |
| `settings.retired_key` | 422 | Reserved. |
| `settings.invalid` | 422 | Reserved. |

## `system` — the server

| Code | HTTP | Fires when |
|---|---|---|
| `system.internal` | 500 | A defect. `request_id` is the handle for the operator. A handler that panicked answers it with `details.reason` `handler_panic`; the panic's text is in the lead's log under `event=handler_panic`, never in the body. |
| `system.unavailable` | 503 | The replica cannot serve safely. `details.reason` says why, for example Postgres unreachable or a failed fork check. Retryable, with `Retry-After`. |

Review unavailability is not an API error. When no reviewer can approve, writes still land held. `oml doctor` reports it.

## Status-code index

| HTTP | Codes |
|---|---|
| 400 | `oauth.client_invalid`, `oauth.redirect_refused`, `oauth.pkce_required`, `oauth.resource_mismatch`, `oauth.authorization_expired`, `oauth.connect_code_invalid`, `oauth.authorization_code_invalid`, `oauth.refresh_invalid`, `request.validation`, `request.idempotency_key_required` |
| 401 | `auth.missing`, `auth.invalid` |
| 403 | `oauth.origin_refused`, `oauth.attach_refused`, `access.verb_ceiling`, `access.grant_refused`, `access.author_only`, `access.issuer_ceiling`, `access.role_exclusive`, `access.banned`, `review.author_in_lineage`, `review.ratifier_is_proposer`, `simulation.audience_refused` |
| 404 | `access.not_found` |
| 409 | `idempotency.in_progress`, `idempotency.digest_mismatch`, `submission.tip_conflict`, `submission.version_conflict`, `submission.claim_key_conflict`, `submission.similar_exists`, `submission.universe_refused`, `submission.user_root_refused`, `name.taken`, `workarea.frozen`, `workarea.transition_refused`, `workarea.in_progress_refused`, `workarea.held_remaining`, `workarea.admin_key_under_root`, `review.serve_invalid`, `simulation.ended`, `simulation.taint_refused`, `simulation.tip_refused`, `vocab.cycle_refused`, `vocab.prefix_conflict`, `upload.session_invalid`, `export.not_ready` |
| 410 | `lifecycle.erased` |
| 413 | `request.payload_too_large` |
| 415 | `request.unsupported_media` |
| 422 | `oauth.connect_refused`, `request.workarea_unresolved`, `submission.scan_veto`, `submission.format_invalid`, `submission.attachment_missing`, `submission.simulation_id_required`, `submission.lineage_required`, `vocab.unknown_curie`, `vocab.inactive_curie`, `upload.digest_mismatch`, `settings.retired_key`, `settings.invalid` |
| 429 | `limit.rate`, `limit.quota` |
| 500 | `system.internal` |
| 503 | `system.unavailable` |

`oauth.client_invalid` is also a 401, at `POST /oauth/token`, for a request that sent an `Authorization` header.
