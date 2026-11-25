---
status: Stable
verified: 2026-11-14
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
    "message": "a contributor key cannot transition a workspace",
    "rule": "R1: the surface an agent can call never promotes, signs, demotes or deletes.",
    "details": {"verb": "POST /v2/workspaces/{workspace}/transitions", "role": "contributor"},
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
| `auth.invalid` | 401 | The key or token is unknown, malformed, expired or denied. `details.reason` is `unknown` for keys, and for a capacity credential (`omls_`) whose web session has ended. A key presented at or after its expiry, by its value or by its connection's access token, is `expired`, with `details.expiry` the instant; only a caller holding that key's own credential is told ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md) d2). It is `session_outside_web` for a capacity credential on `/mcp`, or on `/v1` and `/v2` without `oml-web`'s client identity, or beside a client certificate a gateway forwarded. It is `acting_key_not_linked` for an `OML-Acting-Key` header that names a key not linked to the session's own user, revoked, past its expiry or not `used_by: person`, or that is sent twice, without a capacity credential, or on a route the console does not act through; nothing runs ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d16). On web pages it also carries `not_allowed`, `email_unverified`, `state_mismatch`, `identity_changed`, `provider_refused`, `session_expired`, `disabled` and `email_pending`, the last for an email an admin added for a person who signs in already, which signs in to nothing until that person accepts it ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md), the decision of 2026-11-19). |

## `oauth` — connections

The refusals of the connection flow for claude.ai and phone apps. At `POST /oauth/token` and `POST /oauth/register` the body is the RFC's `{error, error_description}`. At `/oauth/authorize` a refusal is a redirect to the client, or a page where the table says so.

| Code | HTTP | Fires when |
|---|---|---|
| `oauth.client_invalid` | 400 | The `client_id` is unknown, its metadata document is not admissible, or a token request sent client authentication. A token request with an `Authorization` header answers 401. |
| `oauth.redirect_refused` | 400 | The redirect URI is outside the allowlist or not one of the client's registered URIs. |
| `oauth.pkce_required` | 400 | The authorize request has no `code_challenge`, or its method is not `S256`. |
| `oauth.resource_mismatch` | 400 | `resource` is not exactly `<issuer>/mcp`. |
| `oauth.authorization_expired` | 400 | The request outlived ten minutes or spent five attempts, or its handle names no live request. |
| `oauth.connect_code_invalid` | 400 | The connect code is unknown, expired, voided, used, or belongs to a key that is denied or past its expiry. One answer for all six. |
| `oauth.authorization_code_invalid` | 400 | The authorization code is unknown, expired, already redeemed, issued to another client, its PKCE verifier does not match, or its key is past its expiry. The expired key's code is not spent, and redeems if an admin moves the key's date before the code lapses ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md) d2, d5). |
| `oauth.refresh_invalid` | 400 | The refresh token is unknown, expired, already rotated (which ends the connection), issued to another client, or its key is past its expiry. An expired key ends no connection and rotates nothing, so the token works again if an admin moves the key's date ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md) d2, d5). |
| `oauth.origin_refused` | 403 | A `POST /oauth/authorize` came from another origin. It uses no attempt. |
| `oauth.attach_refused` | 403 | Reserved. This build does not return it. |
| `oauth.connect_refused` | 422 | The key cannot hold a connection. `details.reason` is `role`, `used_by_service`, `bearer_key`, `revoked`, `expired`, `not_configured`, `connector_key`, `session_key` or `not_confined`; the last is a code that lives past ten minutes asked for a key that reaches more than one workspace, the universe or a user root. |

## `access` — what this key may do

A key's role, grants and denials decide these. Reads fail closed as absence.

| Code | HTTP | Fires when |
|---|---|---|
| `access.verb_ceiling` | 403 | The key's role never calls this verb, whatever its grants. Examples: an agent key calling an admin verb, an admin key posting a verdict or calling recall (it reads memory text only through the journaled [admin read](../architecture/roles-and-keys.md#the-admin-read)), a `used_by: service` key reviewing, any web-page request carrying an `Authorization` header. `details.reason` `session_key_only`: a person's own act (listing their keys, ending their session, the export, the deletion request, the To do counts read) sent by any key but their web session key, an agent's key naming them included, and the administration To do counts read sent by any key but the person's admin session key. `details.reason` `session_capacity`: a key operation sent as a person's session key of another capacity than the one it serves, such as `POST /v2/keys` as `<user>-admin`. `details.reason` `session_key_denied`: `POST /v2/keys`, `POST /v2/keys/{key}/connect-codes`, `POST /v2/keys/{key}/denials` or `POST /v2/keys/{key}/rotate` sent as a person's session key that carries a denial, on any workspace; such a key issues no key, mints no connect code, cuts off no key and rotates none, and still ends its session. `details.reason` `previous_value`, with `details.verb`: any act that is not a memory read or write sent with a rotated key's previous value inside its grace: every key act (rotate, issue, connect code, cut-off, deny, limits, link, unlink), every admin verb (web emails, capacities, people, crews, settings, fixes, investigations), creating a workspace, its mode, federation and status proposals and ratifications, and accepting a key link; the grace keeps a deployed value reading and writing memories and runs nothing else ([ADR-0096](../adr/ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits.md) d13). `details.reason` `federation_is_person_only`: a workspace's federation field set by a key other than a person's reviewer key or an admin key. `details.reason` `export_person_only`: a workspace export sent by an agent, admin or service key; only a key used by a person exports ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d10). `details.reason` `review_key_in_own_root`: `POST /v1/memories` (or its preview) as a person's `<user>-review` session key in a workspace inside that person's own root; the person's web key starts a claim there, and the review key starts one only outside it ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d2, d10). `details.reason` `create_is_own_root_only`: MCP's `oml_write` with `create_workspace` from a key that is not a contributor key naming a user, an admin key included, refused before any lookup; such a key creates nowhere over MCP, and an admin key keeps its create on HTTP, the CLI and the console ([ADR-0095](../adr/ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people.md) d3). |
| `access.grant_refused` | 403 | The key can read the workspace, but its effective access does not confer the verb's role there. `details`: `workspace_id`, `needed_role`. |
| `access.not_found` | 404 | The thing does not exist, or is outside the key's read scope. The two cases look the same. Also answers a name that resolves to nothing the key may see. |
| `access.author_only` | 403 | A contributor or submitter key tried to supersede a version it did not author (a person's session web key supersedes one any key of its person wrote), or a key tried to withdraw a version it did not author that it can read: only the author key, or a session key of the author's person, withdraws one. |
| `access.issuer_ceiling` | 403 | Key issuance or a grant asks for a role above the issuer's, or a grant role the key role does not admit. |
| `access.role_exclusive` | 403 | Issuance or a grant change would combine the `admin` role with a reviewer grant on one key. Use two keys. |
| `access.banned` | 403 | Reserved. This build does not return it. |

## `request` — the request itself

Malformed or unsupported requests. `details.field` names the refused value: a path or query parameter, a header name, or an [RFC 6901](https://www.rfc-editor.org/rfc/rfc6901) JSON Pointer for a body member.

| Code | HTTP | Fires when |
|---|---|---|
| `request.validation` | 400 | Malformed JSON, a missing or mistyped field, an unknown field or enum value, a stale cursor. A renamed field also carries `details.renamed_to`. A key's limits asked for a key that takes none carry `details.reason`: on `POST /v2/keys/{key}/limits`, `own_key`, `session_key`, `revoked` or `admin_key` with `details.field: key`; on `POST /v2/keys` for an admin key, `admin_key` with `details.field` the limit sent, since an admin key carries no expiry and no rate ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md) d1, d5). A document whose text cuts into more than 4,096 passages carries `details.field: /body` and `details.reason: too_many_passages`; nothing is journalled ([ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md) d3). A contributor key issued without a reader or without a submitter grant carries `details.field: /grants` and `details.reason: contributor_grants`; the server adds no grant and nothing is journalled ([ADR-0093](../adr/ADR-0093-contributor-and-submitter-replace-writer.md) d2). The role `writer`, retired in v0.8.4, is an unknown enum value with no `details.renamed_to`, since it has two successors (d6). A person's web session issuance past 16 grants, or 16 denials on one grant, carries `details.field: /grants` or `/grants/<i>/deny` and `details.reason: too_many`; nothing is journalled ([ADR-0096](../adr/ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits.md) d1). |
| `request.idempotency_key_required` | 400 | A state-creating `POST` has no `Idempotency-Key`. |
| `request.payload_too_large` | 413 | The body exceeds the admitted size. See [limits](../reference/limits.md). |
| `request.unsupported_media` | 415 | The `Content-Type` is not supported. `details.supported` names what is. |
| `request.workspace_unresolved` | 422 | The call names no workspace and the key has no default workspace. |

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
| `submission.claim_key_conflict` | 409 | A new memory would put a second live version under one `claim_key` in the workspace. |
| `submission.similar_exists` | 409 | The write sent `on_similar: refuse` and a readable memory already says the same. `details.similar` lists up to three. Resend with `hold`. |
| `submission.universe_refused` | 409 | A write addressed the tenant's `universe` root from a key that may not write there. |
| `submission.user_root_refused` | 409 | A new memory addressed a user's root, or a workspace beneath it, from a key that is not that user's; or such a key proposed a successor of a ratified retirement there, which would put the claim back in service. Answered where the key reads the workspace or the target; a key that reads neither gets `access.not_found`. |

## `name` — unique names

A name is accepted wherever an id is. An unresolved name is `access.not_found`, exactly as an unknown id would be. See [names and ids](http-api.md).

| Code | HTTP | Fires when |
|---|---|---|
| `name.taken` | 409 | A creation names a name its kind already holds, including a denied key's. `details`: `kind`, `name`. |

## `workspace` — structure and status

Refusals from the workspace tree and its status ladder.

| Code | HTTP | Fires when |
|---|---|---|
| `workspace.frozen` | 409 | A `frozen` workspace was asked to accept a new write, child, promotion or move. |
| `workspace.transition_refused` | 409 | The status change skips a rung, goes backward, or breaks a rule such as a child not being frozen, or moves a user root or a workspace under it off `development` (`details.reason`: `user_root`; [ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d17). |
| `workspace.in_progress_refused` | 409 | The in-progress mode or change conflicts with the node's subtree, ancestors or status. `details.reason` says which. |
| `workspace.held_remaining` | 409 | The commit of an in-progress workspace was asked while a version in its subtree is held. |
| `proposal.pending` | 409 | A proposal of a status move to a workspace already holding a live proposal to the same status, or of a claim already proposed for the same brief or already its member, whoever made the first. Nothing is journalled; a different identity ratifies the live one. `details`: `transition_id` for a status move, `edge_id` for a brief member; the workspace read and the brief's edge listing serve the live row's `proposed_at` and, to a key of the same person, `proposed_by` ([ADR-0095](../adr/ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people.md) d5, d6; [#515](https://github.com/e8kor/open-memory-lab/issues/515)). |
| `workspace.admin_key_under_root` | 409 | A person's disable, or the deletion close that disables them, met a live admin key whose row names their user root. No verb issues one and no denial cuts one off, so nothing is journalled. `details`: `user`, `key`. Clear the key row's `user_root_id`, then send again ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d8). |

## `review` — independence and the serve binding

These codes enforce [R2](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) and the serve binding.

| Code | HTTP | Fires when |
|---|---|---|
| `review.author_in_lineage` | 403 | The reviewer, or its principal, appears in the memory's derivation lineage, or authored a rival successor. With `details.reason: same_person`, the reviewer acts for the person whose key wrote the version, outside that person's own root ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d7). |
| `review.person_required` | 403 | An approval of an instruction-class version, `steering`, `guardrail` or `skill`, or of a successor of another kind that replaces one, came from a key that is not a person's reviewer key (`used_by: person`, no crew, no profile). Until #214 builds [ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d2's full definition of a person (a confirmed, fresh sign-in session), that attestation is what counts as a person. A `used_by: service` key is refused `access.verb_ceiling` before this check, since it reviews nothing. `review next` serves such a version to no key that cannot approve it (the person lane, ADR-0074 d19). Nothing is journalled and the version stays held; a reject from the same key is admitted. `details`: `reason` (`instruction_needs_person`), `kind`, the replaced version's kind when that is the instruction ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d3, d16). |
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

Refusals from vocabulary workspaces and their terms.

| Code | HTTP | Fires when |
|---|---|---|
| `vocab.cycle_refused` | 409 | Ratifying an `is_a` or `replaced_by` edge would close a cycle. |
| `vocab.prefix_conflict` | 409 | The prefix is already registered in the tenant. |
| `vocab.unknown_curie` | 422 | A cited term does not exist or is not readable by the key. |
| `vocab.inactive_curie` | 422 | A cited term is deprecated. `details.successors` and `details.relation` name the replacement. |

## `upload` — upload sessions

The routes under `/v2/uploads` ([ADR-0004](../adr/ADR-0004-bodies-and-attachments.md) d3, as amended 2026-11-17). Nothing is journalled for any of them, and each is counted by code on the `uploads` line of `oml doctor`. A session another key opened answers `access.not_found`.

| Code | HTTP | Fires when |
|---|---|---|
| `upload.session_invalid` | 409 | The session's state does not allow the act: a part or a completion on a session that is not open, or an abandonment of a bound one. `details.state` names the state. |
| `upload.digest_mismatch` | 422 | The completed parts' SHA-256 is not the one the session declared. The session fails, its key is nulled and its parts are removed; open another. |
| `upload.expired` | 410 | The session is past its expiry: 24 hours after its last part, 72 hours after it opened, or 24 hours after an unbound completion. Open another. |
| `upload.too_large` | 413 | The declared size is over `uploads.max_bytes`. `details.max_bytes`. |
| `upload.part_size` | 422 | A part's length is not 8,388,608 bytes, or the remainder for the last part. `details.expected`, `details.got`. |
| `upload.part_number` | 422 | The part number is outside 1 to the session's part count. `details.parts`. |
| `upload.incomplete` | 409 | A completion with parts missing. `details.missing` lists the first 100; send them and complete again. |
| `upload.session_limit` | 409 | The key holds 4 sessions that are open, or completed and unbound. `details.limit`. Complete and use one, abandon one, or wait for one to expire. |
| `upload.tenant_limit` | 409 | The tenant's open and unbound sessions would declare more than `uploads.tenant_max_bytes`. `details.max_bytes`. |
| `upload.storage_failed` | 503 | The object store at `storage.url` failed a read or a write, or the server has none; nothing changed state. Retryable, with `Retry-After`. |

## `bundle` and `import` — workspace imports

`POST /v2/imports` ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d6-d11, R6, R11, R16). The server verifies the whole bundle before it writes, and `oml workspaces import` runs the same `bundle.*` checks before it uploads. No refusal journals anything, and each is counted by code on the `imports` line of `oml doctor`. A message names a file, line or field, never content. A first import from a source without `expect_key` is `request.validation` with `details.field` `/expect_key` and `details.reason` `first_import`; a key other than a person's contributor key, a person's submitter key included, is `access.verb_ceiling` with `details.reason` `import_person_only`.

| Code | HTTP | Fires when |
|---|---|---|
| `bundle.format_unknown` | 422 | The manifest names a format other than `oml-bundle/1`. |
| `bundle.layout` | 422 | An entry outside the fixed layout, out of order, or missing. |
| `bundle.path_unsafe` | 422 | An entry path with `..`, an absolute path, a symlink or a hard link. |
| `bundle.too_large` | 413 | The unpacked size, the entry count or one entry is over its limit; the server's total is `uploads.max_bytes`. |
| `bundle.line_too_long` | 422 | A line over 2 MiB. |
| `bundle.depth` | 422 | JSON nested deeper than 32. |
| `bundle.duplicate` | 422 | A duplicate artifact id and version, or a duplicate body. |
| `bundle.unreferenced_body` | 422 | A body no line references. |
| `bundle.malformed` | 422 | A line that does not parse or breaks the closed schema; the message names the file, line and field. |
| `bundle.digest_mismatch` | 422 | A file, body or version whose digest is not the one stated. |
| `bundle.signature_invalid` | 422 | The manifest or a source event does not verify, or the manifest names a tenant other than `expect_tenant`. |
| `bundle.signature_unexpected` | 422 | The manifest verifies under a key other than `expect_key`. |
| `bundle.decrypt_failed` | 422 | The CLI's identity is not a recipient, or the ciphertext does not authenticate. The server never sees one. |
| `bundle.io` | 422 | The bundle could not be read. |
| `import.target_not_in_progress` | 409 | The target is not a development workspace in progress, nor a new name the import creates in progress. `details.workspace_id`. |
| `import.over_quota` | 429 | The whole plan does not fit the account's meters; nothing is written. `details: {quota, limit, used}`. |
| `import.bundle_older` | 409 | The bundle is older than the last one imported from its source into this target, and `allow_older` is not set. |
| `import.since_mismatch` | 409 | An incremental bundle's base is not the release last imported from its source. |
| `import.source_key_changed` | 409 | The target pins another key for this source, or the bundle names no `expect_key` over an existing pin. Import into a new workspace if the source rotated its key. |
| `import.busy` | 409 | The tenant runs an import or a dry run already, or the server runs two. Nothing is read or written. Retryable. |

## `memory` — a version's own state

| Code | HTTP | Fires when |
|---|---|---|
| `memory.not_held` | 409 | A withdrawal named a version that is not held: a reviewed, rejected or already withdrawn one. A reviewed version changes by supersession instead. |

## `lifecycle` — erased content

| Code | HTTP | Fires when |
|---|---|---|
| `lifecycle.erased` | 410 | The fetched version's key was destroyed by a journaled erase, and this key could read it before. Every other key gets `access.not_found`. Erasure is designed and not built, so this build never returns it. |

## `admin` — admin investigations and fixes

An admin key reads claim text under one reason per investigation, and takes a marked fix that a person confirms or reverts afterwards ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d3, d8, d10, d17). A person's reviewer key of the admin's own person is refused a confirm or revert outside that person's own root with `review.author_in_lineage`, `details.reason: same_person` (amendment of 2026-11-18).

| Code | HTTP | Fires when |
|---|---|---|
| `admin.investigation_required` | 403 | An admin read of claim text, `POST /v2/admin/memories/search` or `GET /v2/admin/memories/{artifact_id}`, names no open investigation of its key: none, an unknown or malformed id, one closed, or another key's. `details.field` is `/investigation` on the search and `investigation` on the one-memory read. Nothing is journalled; open one with `POST /v2/admin/investigations {reason}`. |
| `admin.fix_requires_person` | 403 | `POST /v2/admin/fixes` by an admin key that is not used by a person (`details.reason: not_used_by_person`) or names no person (`details.reason: unlinked`: a bearer admin key no console admin linked and no bootstrap claim named) (ADR-0089 d1, the operator's decision of 2026-11-19). Nothing is journalled. |
| `admin.fix_target_not_promoted` | 409 | A demotion names a version that is not approved: held, rejected, withdrawn, demoted or erased (ADR-0089 d16). `details.state` names its state. Nothing is journalled. |
| `admin.fix_held_by_scan` | 422 | An admin edit's body carries a scan finding that would hold a write. A fix serves at once, so it is refused; demote instead (ADR-0089 d4, d17). A veto answers `submission.scan_veto`. Nothing is journalled. |
| `admin.fix_superseded` | 409 | `POST /v2/review/fixes/{fix_id}/revert` on a fix whose claim moved on since: the version the fix made the claim's tip is no longer its tip, or the fix demoted a version below the tip. A person fixes the claim forward (ADR-0089 d8, d17). Also a fix, confirm or revert whose mark found no version where the fix names one, `details.reason: served_version_missing`; nothing is written (ADR-0089 d7). |
| `admin.fix_not_revertible` | 409 | A revert of an erasure, which is confirmed and never reverted (ADR-0089 d8, d17). |
| `admin.fix_settled` | 409 | A confirm or revert of a fix already confirmed or reverted, including one settled while the request was made. `details.state` names its state when known (ADR-0089 d8, d17). |

## `limit` — rate limits

| Code | HTTP | Fires when |
|---|---|---|
| `limit.rate` | 429 | Unused OAuth client registrations passed 10,000, or a read's `feedback` would pass 1,000 labels in a UTC day. A key past its requests per minute is `limit.rate` with `details: {quota: "requests_per_minute", limit}`, whether the rate is its account's quota or the key's own ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md) d4); sign-ups from one client address past the hour's limit are `limit.rate` too ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d5, d9). Retryable, with `Retry-After`. |
| `limit.quota` | 429 | A person of the sign-up tenant is past a quota of memories, keys or connections, with `details: {quota, limit, used}`. Nothing is journaled, and an admin raises the limit on `/web/admin/users` ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d5). Any account on a quota profile past `memories`, `text_bytes`, `writes_per_month`, `keys`, `connections` or `workspaces` is refused by the write's own commit, a workspace create's included ([ADR-0095](../adr/ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people.md) d4), so two writers at the limit never both pass ([ADR-0071](../adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d3, d5). |

## `bundle` — workspace bundles

| Code | HTTP | Fires when |
|---|---|---|
| `bundle.unencrypted` | 400 | An export names no recipient, or a recipient that is not an age X25519 public key (`age1…`). It is checked before anything is read. `details.field` is `/recipients`. A bundle is always encrypted ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d5). |

The reader's other `bundle.*` codes are the [bundle format](../data/bundle-format.md#refusals)'s; the CLI names them when it verifies a bundle.

## `export` and `settings`

This build serves a workspace export in `history` mode and no settings apply. An export refused for `mode`, `release` or `since` is `request.validation` with `details.reason` `releases_unbuilt` until releases are built.

| Code | HTTP | Fires when |
|---|---|---|
| `export.disabled` | 403 | The tenant's export policy refuses the export. `details.reason` `off`: the policy is `off`. `details.reason` `own_root`: the policy is `own_root` (the default), the workspace lies outside the exporter's own root, and no admin allowance is in force ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d10). |
| `export.not_readable` | 403 | A workspace of the exported subtree is one the exporting key cannot read. The whole export is refused; a workspace the key cannot read at all is `access.not_found`. |
| `export.busy` | 409 | The tenant runs an export already. Nothing is read. Retryable. |
| `export.too_large` | 413 | The bundle would be larger than `uploads.max_bytes`, the most an import takes. Nothing is journalled or metered. `details.max_bytes`. Export a smaller subtree. |
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
| 400 | `bundle.unencrypted`, `oauth.client_invalid`, `oauth.redirect_refused`, `oauth.pkce_required`, `oauth.resource_mismatch`, `oauth.authorization_expired`, `oauth.connect_code_invalid`, `oauth.authorization_code_invalid`, `oauth.refresh_invalid`, `request.validation`, `request.idempotency_key_required` |
| 401 | `auth.missing`, `auth.invalid` |
| 403 | `oauth.origin_refused`, `oauth.attach_refused`, `access.verb_ceiling`, `admin.investigation_required`, `admin.fix_requires_person`, `access.grant_refused`, `access.author_only`, `access.issuer_ceiling`, `access.role_exclusive`, `access.banned`, `review.author_in_lineage`, `review.person_required`, `review.ratifier_is_proposer`, `simulation.audience_refused`, `export.disabled`, `export.not_readable` |
| 404 | `access.not_found` |
| 409 | `idempotency.in_progress`, `idempotency.digest_mismatch`, `submission.tip_conflict`, `submission.version_conflict`, `submission.claim_key_conflict`, `submission.similar_exists`, `submission.universe_refused`, `submission.user_root_refused`, `name.taken`, `workspace.frozen`, `workspace.transition_refused`, `proposal.pending`, `workspace.in_progress_refused`, `workspace.held_remaining`, `workspace.admin_key_under_root`, `review.serve_invalid`, `memory.not_held`, `simulation.ended`, `simulation.taint_refused`, `simulation.tip_refused`, `vocab.cycle_refused`, `vocab.prefix_conflict`, `upload.session_invalid`, `upload.incomplete`, `upload.session_limit`, `upload.tenant_limit`, `export.not_ready`, `import.target_not_in_progress`, `import.bundle_older`, `import.since_mismatch`, `import.source_key_changed`, `import.busy`, `export.busy`, `admin.fix_target_not_promoted`, `admin.fix_superseded`, `admin.fix_not_revertible`, `admin.fix_settled` |
| 410 | `lifecycle.erased`, `upload.expired` |
| 413 | `request.payload_too_large`, `upload.too_large`, `bundle.too_large`, `export.too_large` |
| 415 | `request.unsupported_media` |
| 422 | `oauth.connect_refused`, `request.workspace_unresolved`, `submission.scan_veto`, `submission.format_invalid`, `submission.attachment_missing`, `submission.simulation_id_required`, `submission.lineage_required`, `vocab.unknown_curie`, `vocab.inactive_curie`, `upload.digest_mismatch`, `upload.part_size`, `upload.part_number`, `settings.retired_key`, `settings.invalid`, `bundle.format_unknown`, `bundle.layout`, `bundle.path_unsafe`, `bundle.line_too_long`, `bundle.depth`, `bundle.duplicate`, `bundle.unreferenced_body`, `bundle.malformed`, `bundle.digest_mismatch`, `bundle.signature_invalid`, `bundle.signature_unexpected`, `bundle.decrypt_failed`, `bundle.io`, `admin.fix_held_by_scan` |
| 429 | `limit.rate`, `limit.quota`, `import.over_quota` |
| 500 | `system.internal` |
| 503 | `system.unavailable`, `upload.storage_failed` |

`oauth.client_invalid` is also a 401, at `POST /oauth/token`, for a request that sent an `Authorization` header.
