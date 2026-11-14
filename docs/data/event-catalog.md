---
status: Stable
verified: 2026-11-06
answers: "every journal event type OML writes, its payload fields, and the digest-only rule"
---

# Journal event catalog

Every state change is a journal event. It is signed, hash-chained per tenant and append-only. This page lists each event type the server writes and the fields of its payload. For chain mechanics, see [storage](../architecture/storage.md). For the byte-level format, see [chain format](chain-format.md).

Two rules hold for every payload:

- **Digest-only content.** A payload names bodies only as digests, never as plaintext. Chain verification therefore survives erasing a memory. An event that first journals a body carries a plaintext digest and a ciphertext digest. Both sit inside the signed bytes.
- **Attribution in the clear.** `principal`, `api_key_id` and `at` sit in the envelope. Erasing a memory does not remove them.

Only `oml-server` holds the signing key, so it writes every event.

## The envelope

Every event has these fields.

```json
{
  "tenant_id": "a1b2…",
  "seq": 4182,
  "event_id": "e7f0…",
  "event_type": "submission.remember",
  "prev_hash": "sha256:…",
  "hash": "sha256:…",
  "sig": "ed25519:…",
  "principal": "crew-redline/agent-3",
  "api_key_id": "k9c4…",
  "at": "2026-10-29T14:03:17Z",
  "payload": { }
}
```

Nine fields are signed. `hash` and `sig` are derived from the signed bytes. In the signed form `at` has exactly six fractional digits. See [chain format](chain-format.md).

Every creation event of a named record records its [name](../api/http-api.md). These are `workarea.create`, `crew.created`, `key.issued`, `tenant.created` and `tenant.genesis`. Every other payload field names a row by its id, never its name.

`…digest` fields are SHA-256.

## Submissions

Writes enter held. Nothing in this family changes what anyone can read.

| Event | Payload fields | Meaning |
|---|---|---|
| `submission.remember` | `artifact_id`, `workarea_id`, `claim_key`, `version`, `kind`, `plaintext_digest`, `ciphertext_digest`, `body_bytes`, `derives_from`, `format`, `title`, `anchor`, `origin`, `provenance`, `replaces`, `retirement`, `scan` | A writer submits a memory. It is held and scanned in the same call. A supersession names the replaced version in `replaces`. `retirement: true` proposes to retire the claim. |
| `submission.learn` | the same fields | The memory derives from named prior memories, so `derives_from` is required. An empty one is refused as `submission.lineage_required`. |
| `source.checked` | `artifact_id`, `workarea_id`, `rev`, `result`, `hash`, `path`, `fragment` | A key compared a version's anchored span with its source at `rev`. `result` is `fresh`, `equivalent`, `moved`, `changed` or `removed`. It changes no text and moves nothing through review. |
| `scan.findings_recorded` | `scanned_under`, `versions` (each `artifact_id`, `findings`, a list of `class`, `rule`, `action`, `at`) | An admin's `oml admin rescan --record-held` read held versions under the server's rule set: each takes `scanned_under`, and its findings, an empty list when there are none, become its scan finding unless it has one, and each finding its findings list does not hold by class, rule and field is added to that list under this event's seq ([ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d23). Never the text. It moves nothing through review ([ADR-0065](../adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md) d15; [ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d28). |

`format` is the media type signed with the version and fixed for it, from the closed list of ADR-0056 d1; `title` is the one-line title, present only when the version has one. `plaintext_digest` is the digest of the source as stored, whatever its format, so the signature covers both. An event without `format` reads as `text/plain` and keeps its digest (since v0.6.0).

`body_bytes` is the UTF-8 length of the NFC-normalized body. `anchor` is the origin the author declared, or `null`. `origin.kind` is as declared, or `manual` when none was sent. Neither is inherited by a successor. A chat anchor's `uri` is `chat://<harness>/<session>` with a `turn=<n>` fragment. Erasure does not remove `anchor`.

## Review

| Event | Payload fields | Meaning |
|---|---|---|
| `review.approve` | `serve_id`, `artifact_id`, `version`, `verdict`, `served_digests`, `reviewer_key_id`, `reason_digest` | An approving verdict on a valid served item. The recorded digests are what the verdict counted for. |
| `review.reject` | the same fields | A rejecting verdict. The version stays rejected until a different reviewer reopens it. |
| `review.ratify` | `subject_kind`, `artifact_id`, `version`, `edge_id`, `verdict_event_ids`, `ratifier_key_id`, `policy_snapshot`, `lineage_closure_digest` | The act that finds the review policy satisfied, after checking the author against the full lineage at that moment. `subject_kind` is `claim`, `claim_edge` or `brief_edge`. Edge ratifications carry `same_user`. A brief edge carries no lineage digest. |
| `claim_edge.proposed` | `edge_id`, `src_artifact_id`, `dst_artifact_id`, `label`, `proposer_key_id` | A reviewer key proposes a labeled link between two memories. The proposal is pending and confers nothing. |
| `brief_edge.proposed` | `edge_id`, `artifact_id`, `workarea_id`, `proposer_key_id` | A key proposes one approved memory into a workarea's brief. The proposal is pending. |

`policy_snapshot` holds `k`, `required_roles`, `independence_axis` and `human_required`. Neither edge family reaches the events tail.

## Lifecycle

| Event | Payload fields | Meaning |
|---|---|---|
| `lifecycle.promote` | `artifact_id`, `version`, `workarea_id`, `claim_key`, `ratification_event_id`, `previous_tip_version`, `self_signed`, `same_user` | An approved memory becomes readable by others. This is the state `promoted`. `previous_tip_version` is null for a claim's first version. `same_user` is `true` when the reviewing and authoring keys act for one person. |
| `lifecycle.supersede` | `claim_key`, `workarea_id`, `new_artifact_id`, `new_version`, `replaces_artifact_id`, `replaces_version`, `retirement`, `ratification_event_id`, `self_signed`, `same_user` | A supersession is promoted and takes the tip from the replaced version in one event. It is written after a check that the replaced version is still the tip. |
| `lifecycle.erase` | `artifact_id`, `version`, `workarea_id`, `claim_key`, `dek_destroyed`, `copy_classes`, `erased_by_key_id` | The memory's encryption key is destroyed and its index rows are removed in the same commit. `copy_classes` names the copies affected. Ciphertext in backups persists until its window ends. |

## Workareas and crews

| Event | Payload fields | Meaning |
|---|---|---|
| `workarea.create` | `workarea_id`, `parent_id`, `name`, `status`, `crew_id`, `prefix`, `user_root` | A new node under a parent. A tenant's root has the name `universe` and a null `parent_id`. A non-null `prefix` registers a vocabulary. The prefix is permanent. |
| `workarea.mode_set` | `workarea_id`, `mode`, `from` | A person's reviewer key puts a workarea in progress or commits it back to `standard`. |
| `workarea.federation_set` | `workarea_id`, `field`, `value`, `from` | A person's reviewer key or an admin sets whether recalls scoped elsewhere may search the workarea (`accept_federated`). It grants nothing. |
| `workarea.status_proposed` | `workarea_id`, `from`, `to`, `transition_id`, `proposed_by_key_id` | A forward step of the status ladder is proposed. It stays pending until a different principal ratifies it. |
| `workarea.status_transition` | `workarea_id`, `from`, `to`, `transition_id`, `proposed_by_key_id`, `ratified_by_key_id`, `policy_snapshot`, `same_user` | The ratification of a pending proposal. A step to `validated` also records `children_all_validated`. |
| `workarea.crew_updated` | `workarea_id`, `crew_id` | The workarea now names this crew record. Allowed on validated workareas. |
| `crew.created` | `crew_id`, `name`, `charter_uri`, `external_manager`, `created_by_key_id` | An admin creates a crew record. The name never changes. |
| `crew.updated` | `crew_id`, `charter_uri`, `external_manager`, `updated_by_key_id` | An admin corrects the charter URI or the external manager. The payload holds the full new values. |

No key sees the crew family on the events tail.

## Tenants, keys and grants

| Event | Payload fields | Meaning |
|---|---|---|
| `tenant.created` | `tenant_id`, `name`, `created_by_key_id` | An operator creates a tenant. It is written in the provider tenant's chain. |
| `tenant.genesis` | `tenant_id`, `name`, `provider_seq`, `created_event_id` | The first event of every tenant chain. It references the provider chain's `tenant.created` event. The provider tenant's own genesis has null `name`, `created_event_id` and `provider_seq`. |
| `key.issued` | `key_id`, `name`, `role`, `principal`, `held_by`, `issued_by_key_id`, `default_workarea_id`, `crew_id`, `credential`, `user`, `value_disclosed`, `expiry`, `requests_per_minute` | A key is issued. `credential` is `bearer`, or `connection` for a connector key that has no value. `user` is the id of a user key's root, or null. `expiry` is the instant from which the key authenticates nothing, or null. `requests_per_minute` is present only for a key that carries a rate of its own. |
| `key.revoked` | `key_id`, `revoked_by_key_id`, `reason_digest` | A key is revoked. Its next request answers `auth.invalid`. |
| `key.limits_set` | `key_id`, `set_by_key_id`, `expiry`, `requests_per_minute` | An admin key set the expiry, the requests per minute or both of a key that is not an admin key. The payload holds both values as they stand after the change, null for none. No event marks the instant a key expires. |
| `grant.issued` | `grant_id`, `key_id`, `workarea_node_id`, `role`, `deny`, `subtree_node_id`, `covers`, `epoch` | A key is bound to a workarea node id, never a path. `covers` is `subtree` or `node`. `epoch` is the event's own `seq`. |
| `key.denied` | `denial_id`, `key_id`, `workarea_node_id`, `epoch` | A global per-key denial. It wins over every grant. |
| `key.denial_lifted` | `denial_id`, `key_id`, `lifted_by_key_id`, `reason_digest`, `epoch` | A denial is lifted. The denial row stays as history. |
| `connection.code_issued` | `code_id`, `key_id`, `issued_by_key_id`, `expires_at`, `voids_code_id` | A connect code is minted for a connector key. The payload holds the code's id and expiry, never the code or its digest. |
| `connection.opened` | `connection_id`, `key_id`, `code_id`, `client_id`, `client_kind`, `client_metadata_digest`, `redirect_uri`, `resource`, `replaces_connection_id` | A connection opens at the token exchange. `client_kind` is `cimd` or `dcr`. `resource` is the `<issuer>/mcp` its tokens are bound to. |
| `connection.revoked` | `connection_id`, `key_id`, `revoked_by_key_id`, `cause`, `reason_digest` | A connection ends. `cause` is `admin`, `refresh_reuse` or `code_reuse`. For the two reuse causes `revoked_by_key_id` is null. |

Issuing an access token and refreshing are not events. They change no authority.

## Web and admin reads

| Event | Payload fields | Meaning |
|---|---|---|
| `web.email_allowed` | `email_digest`, `user`, `admin`, `by_key_id` | An admin adds an email to the web allowlist. The journal holds a digest, never the address. |
| `web.email_changed` | the same fields | An entry's user or admin flag changed. Its sessions end. |
| `web.email_removed` | the same fields | An entry was removed. Its sessions end. The user's agent keys keep working. |
| `web.signed_up` | `email_digest`, `user`, `admin`, `provider`, `terms_version`, `self_signup`, `by_key_id` | A person signed up in open mode ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d2). The actor is the `signup.issuer` admin key. The fold is `web.email_allowed`'s, with `terms_version` recorded on the entry. The user is drawn, never derived from the email. |
| `account.profile_set` | `user_root`, `profile`, `overrides` | An admin key set an account's quota profile and its overrides, replacing the ones it had, or took both away: `profile` is then null and `overrides` empty, and the account is on the deployment's default again. The fold writes or deletes the `account_profiles` row and replaces the account's `user_quotas` rows ([ADR-0071](../adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d5). Counts and names only. |
| `web.quota_set` | `user`, `user_root`, `quota`, `limit` | An admin set one person's quota. A null `limit` returns it to the deployment's default (d5). |
| `web.user_disabled` | `user`, `by_key_id` | An admin disabled a person: every entry of the user is disabled and its sessions end in the same commit. Each live agent key of the person is then cut off by its own `key.denied` (d8). |
| `web.deletion_requested` | `user` | The person asked, as `<user>-web`, for their account to be deleted. Nothing ends and no access changes (d6). |
| `web.deletion_closed` | `user`, `by_key_id` | An admin closed the request after disabling the person. The memories stay until they are erased (d6). |
| `memory.admin_read` | `artifact_ids`, `query_digest` | An admin key read memory text and attribution. The payload holds ids and a query digest, never text. |
| `search.default_set` | `pipeline`, `by_key_id` | An admin set the tenant's default search pipeline. A null `pipeline` follows the deployment's default. |
| `federation.policy_set` | `policy`, `from`, `by_key_id` | An admin set the tenant's federation policy: `enabled`, `default_sources` and `max_sources`, with the previous policy ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workspaces-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md) d5). |
| `telemetry.settings_set` | `query_text`, `by_key_id` | An admin turned recall query text on or off. Off also clears stored query text in the same commit. |
| `telemetry.events_read` | `events`, `with_text`, `newest`, `oldest` | An admin read the tenant's recall events. Counts and bounds only. |
| `telemetry.replayed` | `pipelines`, `labeled`, `ran`, `capped` | An admin replayed labeled recalls through named pipelines. Counts only. |

## System

| Event | Payload fields | Meaning |
|---|---|---|
| `system.signing_key_rotated` | `old_key_id`, `new_key_id`, `new_public_key`, `rotation`, `cross_sig`, `ceremony_ref` | The first event a new deployment signing key signs, in the provider tenant's chain. A `planned` rotation carries `cross_sig`. An `unplanned` rotation has a null `cross_sig` and a required `ceremony_ref`. See [chain format](chain-format.md). |
| `system.names_assigned` | `rule`, `names`, `crews` | Names the rows that predate names, once per chain. `names` lists each row with its `kind` (`workarea`, `key` or `tenant`), `id`, `name` and `prior_name`. `crews` lists the crew records made from older workarea events. |

## What is not an event

- **Reads.** They go to a retention-bounded read log, not the journal.
- **Refusals.** A scan veto or any other refusal journals nothing. Hostile input must not grow the signed chain.
- **Connection housekeeping.** Authorization requests, wrong connect-code entries, client registrations and metadata fetches write nothing.
