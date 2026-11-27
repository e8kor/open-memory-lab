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

Every creation event of a named record records its [name](../api/http-api.md). These are `workspace.create`, `crew.created`, `key.issued`, `tenant.created` and `tenant.genesis`. Every other payload field names a row by its id, never its name.

`…digest` fields are SHA-256.

## Submissions

Writes enter held. Nothing in this family makes anything readable to anyone new: a withdrawal only takes a held version out of review and out of in-progress reads.

| Event | Payload fields | Meaning |
|---|---|---|
| `submission.remember` | `artifact_id`, `workspace_id`, `claim_key`, `version`, `kind`, `plaintext_digest`, `ciphertext_digest`, `body_bytes`, `derives_from`, `format`, `title`, `anchor`, `origin`, `provenance`, `replaces`, `retirement`, `scan` | A writer submits a memory. It is held and scanned in the same call. A supersession names the replaced version in `replaces`. `retirement: true` proposes to retire the claim. A skill, a document or a test case also carries its object, `skill {name, description}`, `document {title}` or `test {shape}`, signed with the version ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d1; [ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md) d1; [ADR-0086](../adr/ADR-0086-a-workspaces-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates.md) d1). |
| `submission.learn` | the same fields | The memory derives from named prior memories, so `derives_from` is required. An empty one is refused as `submission.lineage_required`. |
| `source.checked` | `artifact_id`, `workspace_id`, `rev`, `result`, `hash`, `path`, `fragment` | A key compared a version's anchored span with its source at `rev`. `result` is `fresh`, `equivalent`, `moved`, `changed` or `removed`. It changes no text and moves nothing through review. |
| `memory.withdrawn` | `artifact_id`, `workspace_id`, `reason` | A held version's author retracted it: its own key, or a session key of the author's person. The version is kept as `withdrawn` with its text and leaves every review queue. No verdict is recorded and no reviewer is involved. `reason` is null when none was given (ADR-0083 d13). |
| `scan.findings_recorded` | `scanned_under`, `versions` (each `artifact_id`, `findings`, a list of `class`, `rule`, `action`, `at`) | An admin's `oml admin rescan --record-held` read held versions under the server's rule set: each takes `scanned_under`, and its findings, an empty list when there are none, become its scan finding unless it has one, and each finding its findings list does not hold by class, rule and field is added to that list under this event's seq ([ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d23). Never the text. It moves nothing through review ([ADR-0065](../adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md) d15; [ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d28). |

`format` is the media type signed with the version and fixed for it, from the closed list of ADR-0056 d1; `title` is the one-line title, present only when the version has one. `plaintext_digest` is the digest of the source as stored, whatever its format, so the signature covers both. An event without `format` reads as `text/plain` and keeps its digest (since v0.6.0).

`body_bytes` is the UTF-8 length of the NFC-normalized body. `anchor` is the origin the author declared, or `null`. `origin.kind` is as declared, or `manual` when none was sent. Neither is inherited by a successor. A chat anchor's `uri` is `chat://<harness>/<session>` with a `turn=<n>` fragment. Erasure does not remove `anchor`.

## Review

| Event | Payload fields | Meaning |
|---|---|---|
| `review.approve` | `serve_id`, `artifact_id`, `version`, `verdict`, `served_digests`, `reviewer_key_id`, `reason_digest` | An approving verdict on a valid served item. The recorded digests are what the verdict counted for. |
| `review.reject` | the same fields | A rejecting verdict. The version stays rejected until a different reviewer reopens it. |
| `review.ratify` | `subject_kind`, `artifact_id`, `version`, `edge_id`, `verdict_event_ids`, `ratifier_key_id`, `policy_snapshot`, `lineage_closure_digest` | The act that finds the review policy satisfied, after checking the author against the full lineage at that moment. `subject_kind` is `claim`, `claim_edge` or `brief_edge`. Edge ratifications carry `same_user`. A brief edge carries no lineage digest; a brief ratification by the proposer's own person outside that person's root is refused, save in a `sole_admin` tenant, where it carries `sole_admin: true` (ADR-0097 d17 and its build notes). |
| `claim_edge.proposed` | `edge_id`, `src_artifact_id`, `dst_artifact_id`, `label`, `proposer_key_id` | A reviewer key proposes a labeled link between two memories. The proposal is pending and confers nothing. |
| `brief_edge.proposed` | `edge_id`, `artifact_id`, `workspace_id`, `proposer_key_id`, and `reason` when sent | A key proposes one approved memory into a workspace's brief. The proposal is pending. A sent `reason` is scanned as a verdict's is (ADR-0095 d1). |

`policy_snapshot` holds `k`, `required_roles`, `independence_axis` and `human_required`. Neither edge family reaches the events tail.

## Lifecycle

| Event | Payload fields | Meaning |
|---|---|---|
| `lifecycle.promote` | `artifact_id`, `version`, `workspace_id`, `claim_key`, `ratification_event_id`, `previous_tip_version`, `self_signed`, `same_user`, `policy` | An approved memory becomes readable by others. This is the state `promoted`. `previous_tip_version` is null for a claim's first version. `same_user` is `true` when the reviewing and authoring keys act for one person. |
| `lifecycle.supersede` | `claim_key`, `workspace_id`, `new_artifact_id`, `new_version`, `replaces_artifact_id`, `replaces_version`, `retirement`, `ratification_event_id`, `self_signed`, `same_user`, `policy` | A supersession is promoted and takes the tip from the replaced version in one event. It is written after a check that the replaced version is still the tip. |
| `lifecycle.demote` | `artifact_id`, `version`, `workspace_id`, `claim_key`, `restored_artifact_id`, `restored_version`, `reopened_artifact_id`, `restored_valid_until`, `demoted_by_key_id` | An admin's fix stops serving a promoted version and keeps its bytes. When it was the tip, the latest earlier approved version that is not demoted is the tip again and is named in `restored_artifact_id`; otherwise that field is null. When the demoted version's ratification gave the version it replaced an end, and that version is still approved, `reopened_artifact_id` names it and `restored_valid_until` is the end it gets back, null for an open end; with no such version both are null. No read serves a demoted version at any instant ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d16). |
| `lifecycle.erase` | `artifact_id`, `version`, `workspace_id`, `claim_key`, `artifact_ids`, `dek_destroyed`, `copy_classes`, `erased_by_key_id` | An admin's fix erases a claim: `artifact_id` names the version asked for, and `artifact_ids` every version of its claim. In the same commit each version's text and the index rows derived from it are emptied, its search telemetry is erased and its encryption key destroyed. `copy_classes` names the copies affected. The text in database backups and lake ciphertext persists until its window ends ([ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md) d2, d4). |

`policy` holds the review policy the promotion or supersession was decided under: `k`, `required_roles`, `class` (`knowledge`, `instruction` or `vocabulary`), the class whose rule the verdict met: the version's own, or the replaced version's where that is an instruction, and `human_required`, true for an instruction, whose approval counts only from a person's reviewer key ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d16). A promotion journalled before v0.8.3 carries `k` and `required_roles` only.

## Workspaces and crews

| Event | Payload fields | Meaning |
|---|---|---|
| `workspace.create` | `workspace_id`, `parent_id`, `name`, `status`, `crew_id`, `prefix`, `user_root`, and `reason` when sent | A new node under a parent. A sent `reason` is scanned as a verdict's is (ADR-0095 d1). A tenant's root has the name `universe` and a null `parent_id`. A non-null `prefix` registers a vocabulary. The prefix is permanent. An issuance that creates its key's workspaces journals each `workspace.create` before its `key.issued`, naming the issuance's `act_id` for an admin's, and a pending issuance journals them at its confirmation (ADR-0097 d30). |
| `workspace.mode_set` | `workspace_id`, `mode`, `from` | A person's reviewer key puts a workspace in progress or commits it back to `standard`. |
| `workspace.imported` | `import_id`, `workspace_id`, `manifest_digest`, `source` {`server_id`, `tenant_id`}, `exported_at`, `counts`, `refused`, `authenticated`, `allow_older`, `signer_fingerprint`, `upload_digest`, `versions` [{`memory_id`, `source` {`artifact_id`, `version`}, `outcome`}] | A person's contributor key imported a bundle into an in-progress workspace. `counts` holds the plan's outcomes and `refused` the versions the target skipped; `versions` lists each version the import wrote, so the journal rebuilds what each import wrote (I14). `authenticated` says whether the signature was checked against `expect_key`. Each version it wrote has its own `submission.remember` event, held by that key ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d11, d15). |
| `workspace.federation_set` | `workspace_id`, `field`, `value`, `from` | A person's reviewer key or an admin sets whether recalls scoped elsewhere may search the workspace (`accept_federated`). It grants nothing. |
| `workspace.status_proposed` | `workspace_id`, `from`, `to`, `transition_id`, `proposed_by_key_id`, and `reason` when sent | A forward step of the status ladder is proposed. A sent `reason` is scanned as a verdict's is (ADR-0095 d1). It stays pending until a different principal ratifies it. |
| `workspace.status_transition` | `workspace_id`, `from`, `to`, `transition_id`, `proposed_by_key_id`, `ratified_by_key_id`, `policy_snapshot`, `same_user` | The ratification of a pending proposal. A step to `frozen` also records `children_all_frozen`. |
| `workspace.exported` | `workspace_id`, `manifest_digest`, `mode`, `counts` (`workspaces`, `versions`, `erased`, `verdicts`, `events`, `edges`), `recipients`, `bytes` | A person's key exported the workspace's subtree as a signed bundle ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d10). `manifest_digest` is the `sha256:` digest of the bundle's `manifest.json`. `recipients` lists each age recipient's `sha256:` fingerprint, never the key itself. The payload holds counts and digests and no line of content. Nothing changes on the source; the fold counts `bytes` on the exporter's `exports` meter. |
| `workspace.crew_updated` | `workspace_id`, `crew_id` | The workspace now names this crew record. Allowed on frozen workspaces. |
| `crew.created` | `crew_id`, `name`, `charter_uri`, `external_manager`, `created_by_key_id` | An admin creates a crew record. The name never changes. |
| `crew.updated` | `crew_id`, `charter_uri`, `external_manager`, `updated_by_key_id` | An admin corrects the charter URI or the external manager. The payload holds the full new values. |

No key sees the crew family on the events tail.

## Tenants, keys and grants

| Event | Payload fields | Meaning |
|---|---|---|
| `tenant.created` | `tenant_id`, `name`, `created_by_key_id` | An operator creates a tenant. It is written in the provider tenant's chain. |
| `tenant.genesis` | `tenant_id`, `name`, `provider_seq`, `created_event_id` | The first event of every tenant chain. It references the provider chain's `tenant.created` event. The provider tenant's own genesis has null `name`, `created_event_id` and `provider_seq`. |
| `key.issued` | `key_id`, `name`, `role`, `principal`, `used_by`, `issued_by_key_id`, `default_workspace_id`, `crew_id`, `credential`, `user`, `value_disclosed`, `expiry`, `requests_per_minute` | A key is issued. `credential` is `bearer`, or `connection` for a connector key that has no value. `user` is the id of a user key's root, or null. `expiry` is the instant from which the key authenticates nothing, or null. `requests_per_minute` is present only for a key that carries a rate of its own. An admin key's issuance names its `act_id`, and `pending_act_id` while the key waits on a second admin person: it authenticates nothing until the act is confirmed (ADR-0097 d5, d23). |
| `key.revoked` | `key_id`, `revoked_by_key_id`, `actor_person`, `reason`, `act_id`, `cause` | A key is revoked by `POST /v2/keys/{key}/revoke`, or by the decline of the admin act that issued it. Its next request, on any path, answers `auth.invalid`; the keys it issued keep working. `reason` is in clear and was scanned; `act_id` names the admin act it rode, null for a person's own revocation of their own key (ADR-0097 d14). An issuance that failed part way before v0.8.6 journalled `reason_digest` in place of the four last fields. |
| `key.limits_set` | `key_id`, `set_by_key_id`, `expiry`, `requests_per_minute` | An admin key set the expiry, the requests per minute or both of a key that is not an admin key. The payload holds both values as they stand after the change, null for none. No event marks the instant a key expires. |
| `key.suspended` | `key_id`, `cause`, `act_id` | A key authenticates nothing for now (ADR-0097 d8, d21). `cause: against_admin`: an act against its admin person is open, named by `act_id`; the suspension holds while the act is open, a contest included. `cause: unlinked`: an admin key with no accepted link at the start of v0.8.6, signed by the server with no key acting; it still reaches `/v2/whoami` and `POST /v2/keys/self/link` for thirty days, then is revoked (`key.revoked`, `cause: upgrade`). |
| `key.suspension_lifted` | `key_id`, `cause`, `act_id` | The suspension ends: the act against its admin person was settled reversed, or the unlinked key's link was accepted, in the acceptance's commit (ADR-0097 d8, d22). |
| `key.rotated` | `key_id`, `rotated_by_key_id`, `previous_valid_until` | A bearer key got a new value; `previous_valid_until` is when the previous value stops authenticating, null where it stopped at once. Never the value or a hash: the new hash rides the commit as an issuance's does (ADR-0096 d13, since v0.8.5). |
| `key.linked_person` | `key_id`, `user`, `user_root_id`, `linked_by_key_id`, `accepted` | An admin key linked a tenant bearer key `used_by: person`, an admin key's included, to a user, by name and root id. A console admin's link carries `accepted: false` and is pending: the key acts for no one until that user's `key.link_accepted`. The bootstrap ceremony links its first admin key to the person its claim names, `linked_by_key_id` null and `accepted: true`, since that person runs it (ADR-0085 d16 and ADR-0089, the operator's decisions of 2026-11-19). From the accepted link on, the key writes into the user's root wherever its grants allow and the console may act with a non-admin key for that user's session. |
| `key.unlinked_person` | `key_id`, `unlinked_by_key_id` | An admin key ended a key's link, or withdrew one still pending. The console's next act with it is refused. |
| `key.link_accepted` | `key_id`, `user`, `user_root_id`, `accepted_by_key_id` | The user a pending link names accepted it from their own console session, `accepted_by_key_id` that session's key; the key acts for them from this event on (ADR-0085 d16, the operator's decision of 2026-11-19). |
| `grant.issued` | `grant_id`, `key_id`, `workspace_node_id`, `role`, `deny`, `subtree_node_id`, `covers`, `create`, `epoch` | A key is bound to a workspace node id, never a path. `covers` is `subtree` or `node`. `create: true` is present on a grant that lets its key create child workspaces where it reaches, and absent on every other, so an earlier grant reads `false` (ADR-0097 d31). A child created under a node grant carrying it journals the same grant on the child in the create's commit. `epoch` is the event's own `seq`. |
| `grant.revoked` | `key_id`, `grant_id`, `revoked_by_key_id`, `act_id` | A grant an admin act added under `sole_admin` is revoked by the decline of that act, whose `act_id` it names (ADR-0097 d10, the operator's decision of 2026-11-27). The grant confers nothing from this event on, its row staying as history, and the event's `seq` moves the key's permission epoch. |
| `key.denied` | `denial_id`, `key_id`, `workspace_node_id`, `epoch`, `reason` when sent | A global per-key denial. It wins over every grant. The admin's reason is scanned and journalled when the request sends one, since v0.8.5 (ADR-0097 d27). |
| `key.denial_lifted` | `denial_id`, `key_id`, `lifted_by_key_id`, `reason_digest`, `epoch` | A denial is lifted. The denial row stays as history. |
| `connection.code_issued` | `code_id`, `key_id`, `issued_by_key_id`, `expires_at`, `voids_code_id` | A connect code is minted for a connector key. The payload holds the code's id and expiry, never the code or its digest. |
| `connection.code_voided` | `code_id`, `key_id`, `voided_by_key_id`, `act_id` | An unredeemed connect code an admin act minted under `sole_admin` is voided by the decline of that act, whose `act_id` it names; it redeems nothing from this event on (ADR-0097 d10, the operator's decision of 2026-11-27). |
| `connection.opened` | `connection_id`, `key_id`, `code_id`, `client_id`, `client_kind`, `client_metadata_digest`, `redirect_uri`, `resource`, `replaces_connection_id` | A connection opens at the token exchange. `client_kind` is `cimd` or `dcr`. `resource` is the `<issuer>/mcp` its tokens are bound to. |
| `connection.revoked` | `connection_id`, `key_id`, `revoked_by_key_id`, `cause`, `reason_digest` | A connection ends. `cause` is `admin`, `refresh_reuse` or `code_reuse`. For the two reuse causes `revoked_by_key_id` is null. |

Issuing an access token and refreshing are not events. They change no authority.

## Web and admin reads

| Event | Payload fields | Meaning |
|---|---|---|
| `web.email_allowed` | `email_digest`, `user`, `admin`, `by_key_id`, `pending` | An admin adds an email to the web allowlist. The journal holds a digest, never the address. `pending` is true when the user is a person who signs in already: the entry signs in to nothing until that person accepts it ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md), the decision of 2026-11-19). |
| `web.email_changed` | the same fields | An entry's user or admin flag changed. Its sessions end. `pending` is true when the change names another person who signs in already, or keeps an entry that waits. |
| `web.email_accepted` | `email_digest`, `user`, `accepted_by_key_id` | The person a pending entry names accepted it from a console session they already had, `accepted_by_key_id` that session's key. The email signs in from this event on (the decision of 2026-11-19). |
| `web.email_removed` | `email_digest`, `user`, `admin`, `by_key_id`, `pending` | An entry was removed, by an admin or, while it waited (`pending: true`), by the person it names from their own session (`by_key_id` that session's key). Its sessions end. The user's agent keys keep working. |
| `web.signed_up` | `email_digest`, `user`, `admin`, `provider`, `terms_version`, `self_signup`, `by_key_id` | A person signed up in open mode ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md) d2). The actor is the `signup.issuer` admin key. The fold is `web.email_allowed`'s, with `terms_version` recorded on the entry. The user is drawn, never derived from the email. |
| `account.profile_set` | `user_root`, `profile`, `overrides` | An admin key set an account's quota profile and its overrides, replacing the ones it had, or took both away: `profile` is then null and `overrides` empty, and the account is on the deployment's default again. The fold writes or deletes the `account_profiles` row and replaces the account's `user_quotas` rows ([ADR-0071](../adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d5). Counts and names only. |
| `web.quota_set` | `user`, `user_root`, `quota`, `limit` | An admin set one person's quota. A null `limit` returns it to the deployment's default (d5). |
| `web.user_disabled` | `user`, `by_key_id` | An admin disabled a person: every entry of the user is disabled and its sessions end in the same commit. Each live agent key of the person is then cut off by its own `key.denied` (d8). |
| `web.user_enabled` | `user`, `act_id` | A disabled admin person's entries sign in again: the contest of the act against them that disabled them, or its reversal (ADR-0097 d8). The keys the disable cut off stay cut off. |
| `web.deletion_requested` | `user` | The person asked, as `<user>-web`, for their account to be deleted. Nothing ends and no access changes (d6). |
| `web.deletion_closed` | `user`, `by_key_id` | An admin closed the request after disabling the person. The memories stay until they are erased (d6). |
| `memory.admin_read` | `artifact_ids`, `query_digest`, `investigation_id` | An admin key read memory text and attribution under one of its open investigations. The payload holds ids and a query digest, never text ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d10). |
| `admin.investigation_opened` | `investigation_id`, `reason` | An admin key opened an investigation; the reason is in clear and was scanned (ADR-0089 d10, d17). |
| `admin.investigation_closed` | `investigation_id` | The key that opened the investigation closed it (ADR-0089 d17). |
| `admin.act` | `act_id`, `act`, `class`, `target`, `reason`, `actor_person`, `state`, `confirmed_by_person`, `sole_admin`, `cause` | One admin act outside content review (ADR-0097 d1, d2). It journals first in the commit of its effect, and the effect's own events (`key.issued`, `grant.issued`, `connection.code_issued`, `key.revoked`) name its `act_id`. `class` is fixed by `act`: `narrowing`, `erase`, `widening`, `consent`, `against_admin` or `neutral`. A widening act is `effective`, `sole_admin: true`, in a tenant that has never had two live admin persons, and otherwise `pending_confirmation`: its effect waits in the act until a second admin person confirms it. A later `admin.act` with the same `act_id` moves its `state`: `confirmed`, with `confirmed_by_person` and the effect's events in the same commit, or `declined` (after the decline's own `decline_act` act, narrowing; a declined `sole_admin` act is undone in the same commit by `key.revoked`, `grant.revoked`, `connection.code_voided` or `connection.revoked`, `key.limits_set` back to the limits the act found, or `web.email_changed` taking the flag off). A pending act seven days old reads `expired` and never takes effect (d23). An act against an admin (`unflag_admin`, `remove_admin`, `disable_admin`, `change_admin`) moves to `contested` when its target contests it, and to `settled` when a third admin person settles it, `confirmed_by_person` naming the settler for a confirmation and null for a reversal (d8). An admin person's own un-flag or removal (`unflag_own`, `remove_own`) and a closed deletion revoke their admin keys, `cause: deactivation` (d21). |
| `admin.fix` | `fix_id`, `act`, `targets`, `workspace_id`, `claim_key`, `replaced_artifact_id`, `served_artifact_id`, `reason`, `loosens_guardrail`, `lifecycle_event_id`, `admin_person` | An admin key used by a person took a marked break-glass fix: `demote`, `edit`, `retire` or `erase`. It follows, in the same commit, the lifecycle event it caused, which `lifecycle_event_id` names: `lifecycle.demote`, `lifecycle.erase`, or an edit's or retirement's `lifecycle.supersede` after its `submission.remember` (`provenance: admin_fix`, `fix_id`). The reason is in clear and was scanned. `admin_person` is the person the admin key acted for at the fix, which the settlement compares (ADR-0089, the operator's decision of 2026-11-19). When the fix serves a version an earlier unconfirmed fix serves, that earlier fix is settled `superseded` by this one in the same commit (d17). The fix waits for a person to confirm or revert it (ADR-0089 d3, d6, d16, d17). |
| `admin.fix_confirmed` | `fix_id`, `same_user` | A person's reviewer key confirmed the fix; `same_user` is true only for the admin's own person inside that person's own root (ADR-0089 d8, amendment of 2026-11-18). |
| `admin.fix_reverted` | `fix_id`, `same_user`, `restored_artifact_id`, `restored_version`, `derives_from` | A person's reviewer key reverted the fix. In the same commit, before it: the tenant's reserved server key's `key.issued` and `key.revoked` the first time, the restored version's `submission.remember` by that key (`provenance: admin_fix`), the reverter's `review.approve` and the promotion (ADR-0089 d8, d17 c5). |
| `search.default_set` | `pipeline`, `by_key_id` | An admin set the tenant's default search pipeline. A null `pipeline` follows the deployment's default. |
| `federation.policy_set` | `policy`, `from`, `by_key_id` | An admin set the tenant's federation policy: `enabled`, `default_sources` and `max_sources`, with the previous policy ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workspaces-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md) d5). |
| `telemetry.settings_set` | `query_text`, `by_key_id` | An admin turned recall query text on or off. Off also clears stored query text in the same commit. |
| `telemetry.events_read` | `events`, `with_text`, `newest`, `oldest` | An admin read the tenant's recall events. Counts and bounds only. |
| `telemetry.replayed` | `pipelines`, `labeled`, `ran`, `capped` | An admin replayed labeled recalls through named pipelines. Counts only. |

## System

| Event | Payload fields | Meaning |
|---|---|---|
| `system.signing_key_rotated` | `old_key_id`, `new_key_id`, `new_public_key`, `rotation`, `cross_sig`, `ceremony_ref` | The first event a new deployment signing key signs, in the provider tenant's chain. A `planned` rotation carries `cross_sig`. An `unplanned` rotation has a null `cross_sig` and a required `ceremony_ref`. See [chain format](chain-format.md). |
| `system.names_assigned` | `rule`, `names`, `crews` | Names the rows that predate names, once per chain. `names` lists each row with its `kind` (`workspace`, `key` or `tenant`), `id`, `name` and `prior_name`. `crews` lists the crew records made from older workspace events. |

## Former names

Events signed before v0.8.3 keep the names they were signed with, and every reader (replay, the
lake writer's projection, the events tail and its visibility, `oml events`) reads them through
one map, after the stored bytes are verified as they stand
([ADR-0091](../adr/ADR-0091-workareas-become-workspaces-and-the-dreamer-role-becomes-reviewer-on-every-surface-in-one-cut-and-the-journals-former-names-are-read-through-one-map.md)
d4). Events signed before v0.8.4 keep the role `writer` and are read through the map's rule 7 ([ADR-0093](../adr/ADR-0093-contributor-and-submitter-replace-writer.md) d4). The map is the identity on every event v0.8.3 and v0.8.4 write. A `workarea.*` type outside the first
table fails the reader closed, named `journal_unmapped_former_name`, as does an object that holds a
former key beside its successor. This table is the only place
the catalog spells the former names.

| Former event type | Read as |
|---|---|
| `workarea.create` | `workspace.create` |
| `workarea.mode_set` | `workspace.mode_set` |
| `workarea.federation_set` | `workspace.federation_set` |
| `workarea.status_proposed` | `workspace.status_proposed` |
| `workarea.status_transition` | `workspace.status_transition` |
| `workarea.crew_updated` | `workspace.crew_updated` |
| `workarea.reparent` | `workspace.reparent` |
| `workarea.relate_proposed` | `workspace.relate_proposed` |
| `workarea.relate_ratified` | `workspace.relate_ratified` |

| Former value | Where | Read as |
|---|---|---|
| a payload key holding `workarea` or `workareas`, at any depth | every event | the same key with `workspace` or `workspaces` |
| `workarea` | `kind` of each row of `system.names_assigned`'s `names`; `guardrail_scope` of `key.issued` and of any event that records it | `workspace` |
| `dreamer` | `role` of `key.issued` and `grant.issued`; each of `required_roles` inside a `policy_snapshot`, and inside the `policy` of `lifecycle.promote` and `lifecycle.supersede` | `reviewer` |
| `writer` (rule 7, [ADR-0093](../adr/ADR-0093-contributor-and-submitter-replace-writer.md) d4) | `role` of `key.issued` | `contributor` |
| `writer` (rule 7) | `role` of `grant.issued`; each of `required_roles` inside a `policy_snapshot`, and inside the `policy` of `lifecycle.promote` and `lifecycle.supersede` | `submitter` |
| `committed`, `validated` | `status` of `workspace.create`; `from` and `to` of `workspace.status_proposed` and `workspace.status_transition` | `protected`, `frozen` |
| `children_all_validated` | a key of `workspace.status_transition`'s `policy_snapshot` | `children_all_frozen` |
| `held_by`, valued `human` | a key of `key.issued` | `used_by`, valued `person` |

## What is not an event

- **Reads.** They go to a retention-bounded read log, not the journal.
- **Refusals.** A scan veto or any other refusal journals nothing. Hostile input must not grow the signed chain.
- **Connection housekeeping.** Authorization requests, wrong connect-code entries, client registrations and metadata fetches write nothing.
