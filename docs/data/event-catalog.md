<!--
id: data/event-catalog
tier: engineering
status: design draft for review · decided, not built
answers: every journal event type and its payload shape, the digest-only rule, and the system.* key-lifecycle events
-->

# Journal event catalog

**Status:** design draft for review · decided, not built

Every state change in OML is a journal event: signed, hash-chained per tenant, append-only. This
page catalogues every event type and its payload shape. The chain mechanics (sequencing, signing,
the commit transaction) are declared in [storage](../architecture/storage.md); the table that holds
these events is in [postgres-schema](postgres-schema.md). Event names and field spellings below are
design identifiers — normative in shape, adjustable in spelling at implementation without a design
change.

Two rules govern every payload:

- **Digest-only content.** A payload references bodies and attachments only as digests or
  per-artifact-DEK ciphertext references, never plaintext. The signature covers the
  digest/ciphertext envelope, so chain verification survives crypto-shredding (D1). Every event
  that first journals a body or attachment carries both the plaintext digest (content identity)
  and the ciphertext digest of the stored per-artifact-DEK encrypted object, computed at admission
  and covered by `signed_bytes` — the ciphertext digest is what the key-less reconciler compares
  store bytes against ([architecture/reconciliation.md](../architecture/reconciliation.md)).
- **Attribution in the clear.** `principal`, `api_key_id` and timestamps sit in the envelope, not
  the payload, and are not erased by shredding — attribution-metadata erasure is a tracked FR in
  the [roadmap](../product/roadmap.md).

## The envelope

Every event, regardless of type:

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

Only `oml-server` holds the signing key, so every event is written by it — including events it
writes on behalf of the reconciler or lake-writer, which authenticate with their own credentials
and never sign anything themselves (key custody per
[security](../architecture/security.md)). The payload shapes below show representative fields;
`…digest` fields are SHA-256.

The normative byte spec of this envelope is [chain-format.md](chain-format.md): which fields are
signed (nine — `hash` and `sig` are derived, outside the signed bytes), their canonical encodings,
`signed_bytes` as RFC 8785 canonical JSON, the hash, the signature, the chaining rule and the
verification procedure, with computed test vectors. In the signed form `at` carries exactly six
fractional digits; displays like the one above may trim.

## Submission family

Writes enter held; nothing in this family changes visibility, except that a write inside an
[in-progress workarea](../architecture/workareas.md#in-progress-workareas) is readable at once by
the keys the [permission matrix](../architecture/roles-and-keys.md#permission-matrix) allows, as
its readers' grants decide, not as any payload field says. Pipeline:
[admission](../architecture/admission.md).

### submission.remember

```json
{
  "artifact_id": "…", "workarea_id": "…", "claim_key": "deploy-runbook",
  "version": 3, "kind": "memory",
  "plaintext_digest": "sha256:…", "body_bytes": 18240,
  "ciphertext_digest": "sha256:…",
  "attachments": ["sha256:…", "sha256:…"],
  "derives_from": ["artifact_id…"],
  "provenance": "agent",
  "simulation_id": null,
  "replaces": null,
  "retirement": false,
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

A writer submits a memory; it is held on arrival, scanned in the same call, and visible to no one
else until ratified, except, inside an
[in-progress workarea](../architecture/workareas.md#in-progress-workareas), to the keys the
[permission matrix](../architecture/roles-and-keys.md#permission-matrix) allows. A supersession is a submission at the claim's next unused version whose
`derives_from` includes the replaced version: `replaces` is then
`{"artifact_id": "…", "version": 3}`, naming that version, and `retirement: true` marks a
proposal to retire the claim
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d10, d11). `body_bytes` is the
UTF-8 octet length of the NFC-normalized body — the clear integer the recall budget counts
([retrieval](../architecture/retrieval.md#token-budgets)).

### submission.learn

```json
{
  "artifact_id": "…", "workarea_id": "…", "claim_key": "…",
  "version": 1, "kind": "derived_fact",
  "plaintext_digest": "sha256:…", "body_bytes": 512,
  "ciphertext_digest": "sha256:…",
  "derives_from": ["artifact_id…", "artifact_id…"],
  "provenance": "agent", "simulation_id": null,
  "replaces": null, "retirement": false,
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

Same admission as `remember` but the claim is derived from named prior claims, so `derives_from` is
**required**: a `learn` submission with empty or omitted lineage is refused at capture
(`submission.lineage_required`, [errors.md](../api/errors.md)) and this event is never written for
one. A derived conclusion without nameable lineage is submitted as `remember` with
`kind: derived_fact` and journals `submission.remember`. The honesty rule of
[ADR-0001](../adr/ADR-0001-the-constitution.md) still bounds what the required field proves: the
service cannot verify that a declared lineage is complete or true, so the lineage closure is only
as good as what writers declare.

### submission.attach

```json
{
  "artifact_id": "…", "attachment_digest": "sha256:…",
  "media_type": "application/pdf", "size_bytes": 1048576,
  "upload_session_id": "…",
  "ciphertext_digest": "sha256:…",
  "extraction": {"extractor": "pdf-text/…", "extracted_digest": "sha256:…"},
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

Binds a completed upload session's content-addressed object into a claim's signed envelope by
digest; for opaque binaries `extraction` is null and the reviewed claim is provenance + digest +
scan result (D3).

## Review family

The ceremony that turns held into promotable. Policy objects and serve binding:
[admission](../architecture/admission.md) and the CLI
[review flow](../cli/reference.md).

### review.serve

```json
{
  "serve_id": "…", "artifact_id": "…", "version": 3,
  "reviewer_key_id": "…",
  "served_digests": {
    "body": "sha256:…",
    "attachments": ["sha256:…"],
    "replaces": "sha256:…"
  },
  "expires_at": "2026-10-29T15:03:17Z"
}
```

Records exactly what a reviewer was served — body, every attachment, and for a supersession the
version it replaces — under a serve_id bound server-side to (reviewer key, artifact version,
attachment digest set), expiring, consumed by its first verdict.

### review.approve

```json
{
  "serve_id": "…", "artifact_id": "…", "version": 3,
  "verdict": "approve",
  "served_digests": {"body": "sha256:…", "attachments": ["sha256:…"]},
  "reviewer_key_id": "…", "reason_digest": "sha256:…"
}
```

An approving verdict from a valid serve; a verdict without a valid serve, or after the served
content changed, is refused server-side, and the recorded digests are what the verdict counted for.

### review.reject

```json
{
  "serve_id": "…", "artifact_id": "…", "version": 3,
  "verdict": "reject",
  "served_digests": {"body": "sha256:…", "attachments": ["sha256:…"]},
  "reviewer_key_id": "…", "reason_digest": "sha256:…"
}
```

A rejecting verdict, same binding rules; the rejected version stays held-then-rejected and only a
non-author reopen revives it.

### review.ratify

```json
{
  "subject_kind": "claim",
  "artifact_id": "…", "version": 3,
  "edge_id": null,
  "verdict_event_ids": ["…"],
  "ratifier_key_id": "…",
  "policy_snapshot": {"k": 1, "required_roles": ["dreamer"],
                      "independence_axis": "principal", "human_required": false},
  "lineage_closure_digest": "sha256:…"
}
```

The act that finds the review policy satisfied — for a claim or a proposed claim edge
(`subject_kind: "claim_edge"` with `edge_id` set) — after checking R2 against the author and the
full lineage closure at this moment, with the policy snapshot journalled.

### review.reopen

```json
{
  "artifact_id": "…", "version": 3,
  "rejection_event_id": "…", "reopened_by_key_id": "…",
  "reason_digest": "sha256:…"
}
```

A non-author reopens a rejection, returning the version to the review queue
([admission](../architecture/admission.md) owns the reopen rules).

## Claim-edge family

Claim↔claim connection proposals enter the admission pipeline; ratification is journalled as
`review.ratify` with `subject_kind: "claim_edge"`.

### claim_edge.proposed

```json
{
  "edge_id": "…", "src_artifact_id": "…", "dst_artifact_id": "…",
  "label": "…",
  "proposer_key_id": "…"
}
```

A dreamer(propose) key proposes a labelled claim↔claim edge; the proposal is pending, confers
nothing, and enters the lineage closure computed at promotion time. The label vocabulary is not
fixed by the design — an open detail in the [roadmap](../product/roadmap.md).

## Lifecycle family

Visibility and existence changes. Each enqueues its outbox row in the same transaction (D1/D2).

### lifecycle.promote

```json
{
  "artifact_id": "…", "version": 3,
  "workarea_id": "…", "claim_key": "deploy-runbook",
  "ratification_event_id": "…",
  "previous_tip_version": null,
  "self_signed": false
}
```

Ratified content becomes readable by others — inside an in-progress subtree, at scopes inside it
only, until the subtree's commit — and silver materialization is enqueued. Journalled for
a claim's first promoted version, with `previous_tip_version` null; the promotion of a supersession
journals `lifecycle.supersede` instead
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d10).

### lifecycle.demote

```json
{
  "artifact_id": "…", "version": 3,
  "workarea_id": "…", "claim_key": "deploy-runbook",
  "reason": "simulation_ended",
  "demoted_by_key_id": "…", "cause_event_id": "…"
}
```

Removes a version from service while keeping its bytes (R4); the lake sees a retract through the
outbox. `workarea_id`, `claim_key` and `version` name the subject, so the
[events tail](../api/http-api.md#events-tail) filters this event from the journal alone
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d15).

### lifecycle.erase

```json
{
  "artifact_id": "…", "version": 3,
  "workarea_id": "…", "claim_key": "deploy-runbook",
  "dek_destroyed": true,
  "copy_classes": ["silver.body", "object_store", "fts", "embeddings", "held_fts"],
  "erased_by_key_id": "…"
}
```

Crypto-shredding: the DEK is destroyed and plaintext-derived index rows removed in the same
commit — `held_fts` among them when the erased version is held, with the row count checked
([retrieval](../architecture/retrieval.md#held-text-containment)); the payload names the copy
classes affected, and the honest boundary of what persists as ciphertext or in backups is
declared in [audit-and-lineage](../architecture/audit-and-lineage.md) (D4).
`workarea_id`, `claim_key` and `version` name the subject for the events tail, as on
`lifecycle.demote`.

### lifecycle.supersede

```json
{
  "claim_key": "deploy-runbook", "workarea_id": "…",
  "new_artifact_id": "…", "new_version": 4,
  "replaces_artifact_id": "…", "replaces_version": 3,
  "retirement": false,
  "ratification_event_id": "…",
  "self_signed": false
}
```

Journalled at ratification, after the compare-and-set that the replaced version is still the tip
under the successor's own `workarea_id` and `claim_key`, so the one `workarea_id` names both
versions: it promotes the successor and flips the tip in one event, so the tip flip is a single
journalled fact rather than a demote/promote pair to correlate. `self_signed` carries the
successor's mark exactly as `lifecycle.promote` carries a first version's
([ADR-0001](../adr/ADR-0001-the-constitution.md) d5): it is the promotion event of the version it
names, so the mark reaches the journal, the `memories.self_signed` fold and the
[events tail](../api/http-api.md#events-tail) from this event alone. Several held successors of one tip
may coexist; the first ratified takes the tip, and a lost compare-and-set refuses the verdict and
journals nothing. With `retirement: true` the successor is a retirement version and the claim
becomes a retired claim
([audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).
For a tainted successor (a simulation row or sim-derived) the same event sets that simulation's
audience overlay tip instead, and the default tip does not move (same section).

### lifecycle.tip_override

```json
{
  "claim_key": "deploy-runbook", "workarea_id": "…",
  "from_version": 4, "to_version": 2,
  "from_artifact_id": "…", "to_artifact_id": "…",
  "overridden_by_key_id": "…", "reason_digest": "sha256:…"
}
```

An admin designates an earlier promoted version as the served tip without demoting the later one —
an explicit, journalled act, never a silent re-rank. `from_artifact_id` and `to_artifact_id` name
the two versions, so the events tail names its subject from the journal alone. A later
supersession of the restored tip takes the next unused ordinal, never `to_version` + 1
([audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).

### lifecycle.reassign

```json
{
  "artifact_id": "…", "version": 3, "claim_key": "deploy-runbook",
  "from_workarea_id": "…", "to_workarea_id": "…",
  "re_key": null,
  "source_policy_snapshot": {"k": 1, "required_roles": ["dreamer"]},
  "destination_policy_snapshot": {"k": 2, "required_roles": ["dreamer"]},
  "re_enters_held": true,
  "reassigned_by_key_id": "…"
}
```

An admin moves a claim between workareas, with both policy snapshots recorded. A stricter
destination policy re-enters the claim `held`; a move into a `validated` area is refused; a
`claim_key` collision at the destination is refused unless this event records the re-key. The lake
sees the move as a retract MERGE through the outbox. `claim_key` and `version` name the subject
for the events tail, as on `lifecycle.demote`; a move that re-enters `held` takes the version out
of the shared serving indexes and puts its text in the author-fenced held index in the same
commit ([retrieval](../architecture/retrieval.md#held-text-containment)).

## Workarea family

Tree and edge governance. Rules: [workareas](../architecture/workareas.md).

### workarea.create

```json
{
  "workarea_id": "…", "parent_id": "…",
  "name": "crew-redline", "status": "development",
  "crew": {"name": "redline", "charter_uri": "…", "external_manager": "…"},
  "prefix": null,
  "in_progress": false
}
```

A new node under an existing parent; refused when the parent is validated. `in_progress: true`
makes it an [in-progress workarea](../architecture/workareas.md#in-progress-workareas); this event
is the only one that sets the mark, and `workareas.in_progress` folds from it
([ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d1). A non-null `prefix`
registers the workarea as a vocabulary ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)): the
prefix is tenant-unique and **permanent** — never reused or reassigned, even after the vocabulary
retires — and `vocab_prefixes` folds from exactly these events.

### workarea.reparent

```json
{
  "workarea_id": "…",
  "old_parent_id": "…", "new_parent_id": "…",
  "old_path": "universe.a.b", "new_path": "universe.c.b"
}
```

A journalled admin act whose fold is the single-transaction descendant path rewrite; grants are
untouched because they bind node ids.

### workarea.relate_proposed

```json
{
  "edge_id": "…", "src_id": "…", "dst_id": "…",
  "label": "derived_from",
  "proposer_key_id": "…"
}
```

Proposes a labelled edge conferring zero access; the proposer needs dreamer(propose) on at least
one endpoint and reader on both.

### workarea.relate_ratified

```json
{
  "edge_id": "…", "src_id": "…", "dst_id": "…",
  "ratifier_key_id": "…"
}
```

Ratifies the edge; the ratifier needs dreamer(review) on both endpoints and must not be the
proposer (R2 extends to edges). `src_id` and `dst_id` repeat the endpoints, so the events tail
filters this event from the journal alone.

### workarea.status_proposed

```json
{
  "workarea_id": "…", "from": "committed", "to": "validated",
  "transition_id": "…",
  "proposed_by_key_id": "…"
}
```

A dreamer or admin proposes a forward ladder step; the transition stays pending — listed by the
status read — until a second, non-proposing principal ratifies it.

### workarea.status_transition

```json
{
  "workarea_id": "…", "from": "committed", "to": "validated",
  "transition_id": "…",
  "proposed_by_key_id": "…", "ratified_by_key_id": "…",
  "policy_snapshot": {"children_all_validated": true},
  "in_progress_released": false
}
```

Forward-only ladder step — the ratification of a pending `workarea.status_proposed`, which it
references — by a second, distinct dreamer/admin principal, with the policy that held at the
moment snapshotted. `in_progress_released: true` marks an in-progress workarea's commit: it is
journalled only when no version in the subtree is held, `workareas.in_progress_released_seq`
folds from it, and from this event on the subtree is no longer in progress, so its promoted claims are readable at scopes outside it and the
[events tail](../api/http-api.md#events-tail) judges its later events by the ordinary clauses
([workareas](../architecture/workareas.md#in-progress-workareas)).

### workarea.crew_updated

```json
{
  "workarea_id": "…",
  "crew": {"name": "redline", "charter_uri": "…", "external_manager": "…"}
}
```

Crew metadata update — recorded, not operated — and permitted on validated areas, since freeze
covers claim content and promotions, not workarea metadata.

## Keys and grants family

Authorization changes. Semantics: [roles-and-keys](../architecture/roles-and-keys.md).

### tenant.created

```json
{"tenant_id": "…", "name": "…", "created_by_key_id": "…"}
```

An operator creates a tenant — the root of the issuance chain; the operator then issues the
tenant's first admin key and nothing else. Journalled in the **reserved provider tenant's chain**
([storage.md](../architecture/storage.md)); the new tenant's own chain opens with
`tenant.genesis` below.

### tenant.genesis

```json
{"tenant_id": "…", "name": "…", "provider_seq": 2, "created_event_id": "…"}
```

The first event of every non-provider tenant chain: a predecessor-less anchor that roots the
tenant's chain by **reference** to the provider-chain `tenant.created` event that created it — a
reference, not a second numbering. Chain verification of a tenant starts here and checks the
reference against the provider chain. The reserved provider tenant's own chain opens the same way
— `tenant.genesis` at seq 1 — with `created_event_id` and `provider_seq` null: the deployment root
anchors to nothing because nothing precedes it ([chain-format.md](chain-format.md) owns the
genesis byte rule).

### key.issued

```json
{
  "key_id": "…", "role": "writer", "principal": "crew-redline/agent-3",
  "issued_by_key_id": "…", "expiry": "2027-10-29T00:00:00Z",
  "value_disclosed": false,
  "held_by": "agent",
  "default_workarea_id": "…"
}
```

A key issued without its value (show-once ceremony; the journal never learns the secret), under the
issuer ceiling: role no higher than the issuer's. `held_by` is the issuing admin's journalled
attestation that the key is held by a human, an agent, or a service process — the server verifies
the attestation, never the humanness ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md));
`human_required_classes` consumes it, and the evidence export shows it. A key attested
`held_by: service` can never satisfy `human_required_classes` and never counts as review capacity.

### key.revoked

```json
{"key_id": "…", "revoked_by_key_id": "…", "reason_digest": "sha256:…"}
```

Revocation: the key's next request served by the lead answers `auth.invalid`, since keys and
effective access are resolved on every request, and a follower, which resolves keys from its
local replica, does so after replication lag
([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)). Its seq
is the key's permission epoch from then on. A key whose
issuance failed after `key.issued` gets this event in a following commit, with `reason_digest`
over the fixed reason `issuance_incomplete`
([roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)).

### key.rotated

```json
{"old_key_id": "…", "new_key_id": "…", "grace_until": "2026-11-12T00:00:00Z"}
```

One journalled act carries both key ids and the grace deadline: the successor is minted with a new
key id, linked to the old by `rotated_from`, its grants reproduced from the journalled grant
records; the old key stays usable through the bounded grace window and is revoked at `grace_until`
— a journalled `key.revoked` citing this event as cause, with the epoch bump revocation always
carries. The successor has no `key.issued` and its reproduced grants no `grant.issued`, so this
event's seq is the successor's `issued_seq`, the epoch of each of its grant rows, and its first
permission epoch ([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)).
Each reproduced grant carries its source grant's `unreviewed`, folded as
[grant.issued](#grantissued) says ([ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d7,
proposed).

### grant.issued

```json
{
  "grant_id": "…", "key_id": "…",
  "workarea_node_id": "…", "role": "reader",
  "deny": [{"subtree_node_id": "…"}],
  "unreviewed": "allow",
  "epoch": 17
}
```

Binds a key to a workarea node id (never a path) with per-grant deny masks; `epoch` is this
event's own journal seq, which makes every historical effective-access computation reproducible
([workareas](../architecture/workareas.md)). `unreviewed`, on a reader grant only and absent on
the others, is whether the holder reads other keys' held claims inside the in-progress workareas
the grant covers ([permission matrix](../architecture/roles-and-keys.md#permission-matrix),
[ADR-0030](../adr/ADR-0030-in-progress-workareas.md) d7, proposed); a grant's value never
changes. A reader grant's `grant.issued` that carries no `unreviewed`, as every one journalled
before 0.3.0-design does, folds as `allow`.

### grant.revoked

```json
{"grant_id": "…", "key_id": "…", "revoked_by_key_id": "…", "epoch": 19}
```

Journalled grant removal; `epoch` is this event's own journal seq, and the key's next request
served by the lead is resolved without the grant, since effective access is computed per request;
a follower's reads follow after replication lag ([workareas](../architecture/workareas.md)).

### key.denied

```json
{
  "denial_id": "…", "key_id": "…",
  "workarea_node_id": "…", "epoch": 18
}
```

A global per-key deny: a distinct journalled object with deny-wins semantics across all of the
key's grants; `epoch` is this event's own journal seq.

### key.denial_lifted

```json
{"denial_id": "…", "key_id": "…", "lifted_by_key_id": "…", "epoch": 20}
```

Lifts a global per-key deny — a journalled act with its own epoch bump; the denial row stays as
history.

### ban.imposed

```json
{
  "ban_id": "…", "subject_kind": "principal",
  "subject": "crew-redline/agent-3",
  "banned_by_key_id": "…", "reach_preview_digest": "sha256:…"
}
```

Bans an author or source, checked at admission from then on; the CLI previews the ban's reach
before this event exists.

### ban.lifted

```json
{"ban_id": "…", "lifted_by_key_id": "…"}
```

Lifts a ban; the ban row stays as history rather than being deleted (R4 in spirit: no silent
removal of governance acts).

## Simulation family

The fenced provenance channel. Semantics and taint rules:
[simulations](../architecture/simulations.md).

### simulation.opened

```json
{
  "simulation_id": "…",
  "audience": [{"key_id": "…"}, {"grant_ref": "…"}]
}
```

Opens a simulation with a journalled audience; a simulation whose audience holds no dreamer key can
promote nothing (fail closed).

### simulation.audience_changed

```json
{
  "simulation_id": "…",
  "added": [{"key_id": "…"}],
  "removed": [{"grant_ref": "…"}],
  "reason": "replace departed reviewer"
}
```

An admin corrects a live simulation's audience (`POST /v2/simulations/{id}/audience` — the only
recovery from the fail-closed no-dreamer state short of ending it); at least one of
`added`/`removed` is non-empty, and the audience at any seq is the fold of `simulation.opened`
plus every `audience_changed` since — that fold is what makes audience membership reconstructible
from the journal. Removal takes effect on the next read; past reads stand in the read log.

### simulation.write

```json
{
  "artifact_id": "…", "workarea_id": "…", "claim_key": "…",
  "version": 1, "kind": "memory",
  "plaintext_digest": "sha256:…", "body_bytes": 2048,
  "ciphertext_digest": "sha256:…",
  "derives_from": ["…"],
  "provenance": "simulation",
  "simulation_id": "…",
  "simulated_time": "1944-06-06T06:30:00Z",
  "replaces": null,
  "retirement": false,
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

The submission envelope with `provenance: simulation` and `simulation_id` mandatory and indelible —
the event gets a real chain timestamp (`at` in the envelope) and the declared `simulated_time` is
data, never backdating; a submission whose `derives_from` touches a simulation row is
server-forced to carry that row's `simulation_id`. It is a simulation row's submission event and
carries every field `submission.remember` carries: `body_bytes`, the clear integer the recall
budget counts ([retrieval](../architecture/retrieval.md#token-budgets)); `replaces`, naming the
version a simulation successor replaces; and `retirement`, marking a proposal to retire the claim
([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d6, d10, d11). Its visibility on
the events tail is declared beside the `submission.*` family's
([HTTP API](../api/http-api.md#events-tail)).

### simulation.ended

```json
{
  "simulation_id": "…", "ended_by_key_id": "…",
  "disposition": "demote"
}
```

Ends the simulation; its rows are demoted and sim-derived taints re-reviewed through subsequent
`lifecycle.demote` events that cite this event as cause.

## Export family

### export.requested

```json
{
  "export_id": "…", "kind": "evidence",
  "from_seq": 0, "include_simulations": false,
  "requested_by_key_id": "…",
  "exporter": {"name": "…", "version": "…"}
}
```

An admin requests an evidence or chain export ([http-api](../api/http-api.md)): an async,
journalled job that rides the outbox as kind `export`; the bundle exists only when chain
verification finishes.

## System family

Events the machinery writes about itself — all signed by `oml-server`, authored on behalf of the
named service.

### system.materialize_skipped

```json
{
  "outbox_seq": 90231, "kind": "materialize",
  "artifact_id": "…", "attempts": 5,
  "error_class": "schema_mismatch"
}
```

The lake-writer exhausted a poison row's retry budget, dead-lettered it, and advanced the frontier
— journalled so the skip is a fact, not a log line (D2).

### system.reconciliation_finding

```json
{
  "finding_id": "…",
  "class": "state-mismatch",
  "artifact_id": "…", "version": 3,
  "journal_seq": 4182, "delta_version": 771,
  "attempt": 2
}
```

One finding from the closed vocabulary (missing, orphan, duplicate, state-mismatch,
digest-mismatch, stale-frontier, dead-lettered), with the identity and attempt counter that let
recurrence halt auto-repair (D16, [reconciliation](../architecture/reconciliation.md)).

### system.reconciliation_repair

```json
{
  "finding_id": "…",
  "repair_kind": "lake_reemit",
  "fold_seq": 4182,
  "outbox_seq": 90515
}
```

A repair emitted through the outbox from the current journal fold (`repair_kind:
"metadata_refold"` for Postgres-side repairs); the lake-writer refuses operations older than an
artifact's last applied fold seq.

### system.readlog_anchor

```json
{
  "batch_id": "…",
  "from_at": "2026-10-29T00:00:00Z", "to_at": "2026-10-30T00:00:00Z",
  "events_digest": "sha256:…", "event_count": 12907
}
```

Anchors one read-log batch's digest into the chain, so the retention-bounded read_events table can
be verified intact while it exists and attested-but-gone after its partition drops (D12).

### system.settings_applied

```json
{
  "settings_digest": "sha256:…",
  "applied_by_key_id": "…",
  "refused_keys": []
}
```

A validated settings apply (`oml settings apply`), recording the effective configuration by digest;
an apply naming a retired key is refused before this event exists, so `refused_keys` is normally
empty and present for the audit trail.

### system.lead_acquired

```json
{
  "node_id": "…", "address": "…",
  "lease_epoch": 7,
  "previous_lead_node_id": "…"
}
```

A lead takeover ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d14): signed
by the **new** lead as its first act after acquiring the lead lease and passing the startup fork
check, so the journal always answers "which process was lead when." Journalled in the reserved
provider tenant's chain. The lease itself is never load-bearing — the per-tenant chain lock is;
a split-brain pair contends on the lock, it cannot fork a chain.

### system.signing_key_rotated

```json
{
  "old_key_id": "d92de0b4…",
  "new_key_id": "a41f09d2…",
  "new_public_key": "ed25519:…",
  "rotation": "planned",
  "cross_sig": "ed25519:…",
  "ceremony_ref": null
}
```

Deployment signing-key rotation, journalled in the reserved provider tenant's chain as the
**first event the new key signs**. Key ids are public-key fingerprints
([chain-format.md](chain-format.md)). A `planned` rotation carries `cross_sig` — the old key's
signature over the new key, byte rule in [chain-format.md](chain-format.md) — so a verifier's
trust extends across the rotation without a new ceremony. An `unplanned` rotation (old key
compromised or lost) has `cross_sig` null by definition and `ceremony_ref` **required**: a digest
reference to the out-of-band ceremony record in which the new key's fingerprint was established,
where trust re-roots.

### system.kek_rotated

```json
{
  "old_kek_id": "kek-2026-04",
  "new_kek_id": "kek-2026-10",
  "rewrap_job_id": "…"
}
```

Scheduled KEK rotation ([security](../architecture/security.md)): emitted in the reserved
provider tenant's chain when the new KEK becomes the wrapping key for newly minted DEKs and the
re-wrap job for **live** DEKs is enqueued — `rewrap_job_id` is the journal-side handle for that
job, which rides the outbox like other background work. Re-wrap covers live DEKs only; wrapped-DEK
copies in retained backups decay on the retirement schedule below
([audit-and-lineage](../architecture/audit-and-lineage.md) owns the bound).

### system.kek_destroyed

```json
{
  "kek_id": "kek-2026-04",
  "retired_by_event_id": "…",
  "backup_horizon": "2027-01-29T00:00:00Z",
  "evidence_digest": "sha256:…"
}
```

Retired-KEK destruction — the act that completes erasure against backups: once no KEK can unwrap
them, wrapped-DEK copies in older backups are noise. Emitted only after the retirement window
passes; the payload carries that evidence: `retired_by_event_id` names the `system.kek_rotated`
event that retired this KEK, `backup_horizon` is the instant past which no retained backup holds
DEKs wrapped by it, and `evidence_digest` is the digest of the operator's retention-evidence
record. In the standalone and docker profiles the destruction is journalled file deletion of the
keyring entry ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d4).

## Reserved: the `import` family

The `import.*` namespace (manifest_submitted, completed, superseded, erased) is reserved for
dataset onboarding — one chain event per imported dataset, never per row, with per-row digests
anchored by Merkle roots inside the signed manifest event. The family's definitions live in
[ADR-0020](../adr/ADR-0020-dataset-onboarding.md) and enter this catalog when that post-v1 feature
is scheduled. Until then, `provenance: connector` appears in the schema's CHECK constraint with no
submission event that produces it — reserved, not an oversight.

## What is deliberately not an event

Reads are not journal events: they go to the retention-bounded `read_events` log, whose batch
digests enter the chain via `system.readlog_anchor` — journalling every read would couple read
latency to the per-tenant chain lock. Serve/verdict/ratify are events because they change what may
become visible. A **scan veto is not an event either**: `submission.scan_veto` is a refusal like
any other refusal, and refusals journal nothing — the deterministic scan's verdict is reproducible
from the journalled rule-set pin, and letting hostile input write into the signed chain would hand
an attacker the chain's growth. Whether refused submissions should leave a durable trace outside
the chain (a scan-refusal log shaped like the read log) is RS-16 on the
[roadmap](../product/roadmap.md). Upload-session part arrivals are operational state in `upload_sessions`; only the
completed attachment binding (`submission.attach`) is journalled. Where an event's exact payload
fields are still open (for example the independence-axis fields a `policy_snapshot` must carry for
crew-level independence), the owning page says so and the gap is tracked in the
[roadmap](../product/roadmap.md).
