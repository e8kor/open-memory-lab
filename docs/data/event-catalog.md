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
  digest/ciphertext envelope, so chain verification survives crypto-shredding (D1).
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

## Submission family

Writes enter held; nothing in this family changes visibility. Pipeline:
[admission](../architecture/admission.md).

### submission.remember

```json
{
  "artifact_id": "…", "workarea_id": "…", "claim_key": "deploy-runbook",
  "version": 3, "kind": "memory",
  "plaintext_digest": "sha256:…", "body_bytes": 18240,
  "attachments": ["sha256:…", "sha256:…"],
  "derives_from": ["artifact_id…"],
  "provenance": "agent",
  "simulation_id": null,
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

A writer submits a memory; it is held on arrival, scanned in the same call, and visible to no one
else until ratified.

### submission.learn

```json
{
  "artifact_id": "…", "workarea_id": "…", "claim_key": "…",
  "version": 1, "kind": "derived_fact",
  "plaintext_digest": "sha256:…",
  "derives_from": ["artifact_id…", "artifact_id…"],
  "provenance": "agent", "simulation_id": null,
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

Same admission as `remember` but the claim is derived from named prior claims, so `derives_from` is
expected rather than optional — an omitted derivation is undetectable by the service (the honesty
rule of [ADR-0001](../adr/ADR-0001-the-constitution.md)), and the lineage closure is only as good
as what writers declare.

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
  "previous_tip_version": 2,
  "self_signed": false
}
```

Ratified content becomes readable by others and silver materialization is enqueued; a supersession
promotion CAS-checks that `previous_tip_version` is still the tip, else it is refused.

### lifecycle.demote

```json
{
  "artifact_id": "…", "version": 3,
  "reason": "simulation_ended",
  "demoted_by_key_id": "…", "cause_event_id": "…"
}
```

Removes a version from service while keeping its bytes (R4); the lake sees a retract through the
outbox.

### lifecycle.erase

```json
{
  "artifact_id": "…",
  "dek_destroyed": true,
  "copy_classes": ["silver.body", "object_store", "fts", "embeddings"],
  "erased_by_key_id": "…"
}
```

Crypto-shredding: the DEK is destroyed and plaintext-derived index rows removed in the same commit;
the payload names the copy classes affected, and the honest boundary of what persists as ciphertext
or in backups is declared in [audit-and-lineage](../architecture/audit-and-lineage.md) (D4).

### lifecycle.supersede

```json
{
  "claim_key": "deploy-runbook", "workarea_id": "…",
  "new_artifact_id": "…", "new_version": 4,
  "replaces_version": 3,
  "ratification_event_id": "…"
}
```

The promoted supersession takes the tip from the version it replaces — one event so the tip flip is
a single journalled fact rather than a demote/promote pair to correlate.

### lifecycle.tip_override

```json
{
  "claim_key": "deploy-runbook", "workarea_id": "…",
  "from_version": 4, "to_version": 2,
  "overridden_by_key_id": "…", "reason_digest": "sha256:…"
}
```

An admin designates an earlier promoted version as the served tip without demoting the later one —
an explicit, journalled act, never a silent re-rank.

### lifecycle.reassign

```json
{
  "artifact_id": "…",
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
sees the move as a retract MERGE through the outbox.

## Workarea family

Tree and edge governance. Rules: [workareas](../architecture/workareas.md).

### workarea.create

```json
{
  "workarea_id": "…", "parent_id": "…",
  "name": "crew-redline", "status": "development",
  "crew": {"name": "redline", "charter_uri": "…", "external_manager": "…"}
}
```

A new node under an existing parent; refused when the parent is validated.

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
  "edge_id": "…",
  "ratifier_key_id": "…"
}
```

Ratifies the edge; the ratifier needs dreamer(review) on both endpoints and must not be the
proposer (R2 extends to edges).

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
  "policy_snapshot": {"children_all_validated": true}
}
```

Forward-only ladder step — the ratification of a pending `workarea.status_proposed`, which it
references — by a second, distinct dreamer/admin principal, with the policy that held at the
moment snapshotted.

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
tenant's first admin key and nothing else.

### key.issued

```json
{
  "key_id": "…", "role": "writer", "principal": "crew-redline/agent-3",
  "issued_by_key_id": "…", "expiry": "2027-10-29T00:00:00Z",
  "value_disclosed": false
}
```

A key issued without its value (show-once ceremony; the journal never learns the secret), under the
issuer ceiling: role no higher than the issuer's.

### key.revoked

```json
{"key_id": "…", "revoked_by_key_id": "…", "reason_digest": "sha256:…"}
```

Immediate revocation; the permission epoch bump that invalidates caches rides the grant events the
revocation implies.

### key.rotated

```json
{"old_key_id": "…", "new_key_id": "…", "overlap_until": "2026-11-12T00:00:00Z"}
```

One journalled act links old and new key ids: the successor is issued with grants reproduced from
the journalled records, and the old key's expiry is scheduled to `overlap_until`.

### grant.issued

```json
{
  "grant_id": "…", "key_id": "…",
  "workarea_node_id": "…", "role": "reader",
  "deny": [{"subtree_node_id": "…"}],
  "epoch": 17
}
```

Binds a key to a workarea node id (never a path) with per-grant deny masks; `epoch` makes every
historical effective-access computation reproducible.

### grant.revoked

```json
{"grant_id": "…", "key_id": "…", "revoked_by_key_id": "…", "epoch": 19}
```

Journalled grant removal; the epoch bump invalidates cached effective-access computations.

### key.denied

```json
{
  "denial_id": "…", "key_id": "…",
  "workarea_node_id": "…", "epoch": 18
}
```

A global per-key deny: a distinct journalled object with deny-wins semantics across all of the
key's grants.

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

### simulation.write

```json
{
  "artifact_id": "…", "workarea_id": "…", "claim_key": "…",
  "version": 1, "kind": "memory",
  "plaintext_digest": "sha256:…",
  "derives_from": ["…"],
  "provenance": "simulation",
  "simulation_id": "…",
  "simulated_time": "1944-06-06T06:30:00Z",
  "scan": {"result": "pass", "scanner_version": "…"}
}
```

The submission envelope with `provenance: simulation` and `simulation_id` mandatory and indelible —
the event gets a real chain timestamp (`at` in the envelope) and the declared `simulated_time` is
data, never backdating; a submission whose `derives_from` touches a simulation row is
server-forced to carry that row's `simulation_id`.

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
  "requested_by_key_id": "…"
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

## What is deliberately not an event

Reads are not journal events: they go to the retention-bounded `read_events` log, whose batch
digests enter the chain via `system.readlog_anchor` — journalling every read would couple read
latency to the per-tenant chain lock. Serve/verdict/ratify are events because they change what may
become visible. Upload-session part arrivals are operational state in `upload_sessions`; only the
completed attachment binding (`submission.attach`) is journalled. Where an event's exact payload
fields are still open (for example the independence-axis fields a `policy_snapshot` must carry for
crew-level independence), the owning page says so and the gap is tracked in the
[roadmap](../product/roadmap.md).
