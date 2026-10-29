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

Every non-2xx response under `/v1`:

```json
{
  "error": {
    "code": "access.verb_ceiling",
    "message": "a writer key cannot transition a workarea",
    "rule": "R1: the surface an agent can call never promotes, signs, demotes or deletes.",
    "details": {"verb": "POST /v1/workareas/{id}/transitions", "role": "writer"},
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
| `auth.missing` | 401 | No `Authorization: Bearer` header. | Every `/v1` request authenticates; only `/healthz` and `/readyz` do not. |
| `auth.invalid` | 401 | The key is unknown, malformed, expired or revoked (`details.reason` says which). | Tenant and identity derive from the key, server-side, or not at all — an unverifiable caller gets nothing (D6). |

## `access` — what this key may do

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `access.verb_ceiling` | 403 | A key's **role** may never call this verb: any agent key (reader/writer/dreamer) calling an admin verb — re-parent, keys, grants, bans, simulations, reassign, demote, erase, tip-override, export, settings (a reader or writer key proposing a status transition is also this code; a dreamer proposing or ratifying one is not — transitions are dreamer-or-admin verbs, D8); an admin key posting a verdict (admin ≠ reviewer); an operator key touching content. | **R1: the surface an agent can call never promotes, signs, demotes or deletes** — the verb ceiling is per role (D9), and no claimed purpose lifts it. |
| `access.grant_refused` | 403 | The key can read the resource but its effective access — grants minus denies, at the current permission epoch — does not confer the verb's role on this workarea. | Effective access is the union of grants, each attenuated by its denies, deny-wins for global denials; what a grant does not confer, the key does not have (D7). |
| `access.not_found` | 404 | The resource does not exist, **or** lies outside the key's effective read scope, **or** is a simulation row and the key is outside that simulation's audience. Indistinguishable by design. | Reads fail closed as absence: the service never confirms the existence of what a key may not see (D7, D10, R3). |
| `access.author_only` | 403 | A writer key proposes a supersession of a claim it did not author. | A writer proposes supersessions of its own claims only; editing another author's claim is the dreamer's edit-as-proposal path (D9). |
| `access.issuer_ceiling` | 403 | Key issuance (or a grant on it) requests a role above the issuer's; an operator key issues anything beyond a tenant's first admin key. | A key is issued with a role no higher than the issuer's (D9); authority never amplifies through issuance. |
| `access.role_exclusive` | 403 | Key issuance or a grant amendment would combine the `admin` role with a dreamer-class grant on one key. | Admin is never a reviewer (D9): a key never combines admin with dreamer verbs. A person holding both duties holds two keys, so the journal shows which capacity acted ([roles-and-keys](../architecture/roles-and-keys.md)). |
| `access.banned` | 403 | Admission-time check: the submitting principal or source is under an active ban. | Bans are journalled admin acts checked at every admission from the moment they are imposed (D9); lifting one is an equally journalled act. |

## `request` — the request itself

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `request.validation` | 400 | Malformed JSON, missing or ill-typed fields, unknown enum values, a stale or foreign pagination cursor. | The contract is [openapi.yaml](openapi.yaml); what it does not admit is refused, not coerced. |
| `request.idempotency_key_required` | 400 | A state-creating `POST` arrives without `Idempotency-Key`. | Every side-effecting request is replayable exactly once by construction (D13); the service refuses to guess whether a retry is a duplicate. |
| `request.payload_too_large` | 413 | Body or part exceeds the admitted size. | Size bounds are admission policy, enforced before bytes are stored. |

## `idempotency` — the replay contract

Semantics in full on the [HTTP API page](http-api.md#idempotency); the record is
`(key_id, workarea, idem_key) → request digest + state + stored response`, claimed before any side
effect (D13).

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `idempotency.in_progress` | 409 | A replay arrives while the record is `in_progress` — the first attempt has not committed or failed yet. **Retryable**, with `Retry-After`. | The record is claimed in its own transaction before side effects, so two racing attempts can never both execute (D13). |
| `idempotency.digest_mismatch` | 409 | Same `(key, workarea, idem_key)`, different request digest. | An idempotency key names exactly one request; replay returns the stored response, a *different* request under the same key is refused — never silently honored as new work (D13). |

A successful replay is not an error: it answers the stored response with header
`Idempotency-Replay: true`.

## `submission` — admission refusals

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `submission.scan_veto` | 422 | The deterministic scan, running in the same call as capture, vetoes the submission (`details.finding` names the rule class that fired). The veto is journalled as a rejection. | The scan is rules, not a model: same input, same verdict, reproducible from the journalled rule-set version — and it can veto but never promote (D11; [admission](../architecture/admission.md)). |
| `submission.attachment_missing` | 422 | The submission binds an attachment digest with no completed upload session owned by this key. | The claim and its files are one signed unit; the envelope binds only bytes the service already holds and has digest-verified (D3). |
| `submission.simulation_id_required` | 422 | `derives_from` touches a simulation row but the submission does not carry that row's `simulation_id`. | Taint propagates and nesting is forbidden: a derivation from a simulation row carries that simulation's id, server-enforced, so one end-act demotes the closure (D10). |
| `submission.lineage_required` | 422 | A `verb: "learn"` submission arrives with an empty or omitted `derives_from`. | `learn` asserts a derived claim, so its lineage is required, never optional; a derived conclusion whose author cannot or will not name lineage is submitted as `remember` with `kind: derived_fact` ([http-api](http-api.md)). |
| `submission.tip_conflict` | 409 | A supersession's target is not the current served tip — at submission, or re-checked at promotion when a competing supersession won in between. | Supersession promotion CAS-checks its target is the current tip; the tip flip is one journalled fact, never a lost update ([postgres-schema](../data/postgres-schema.md)). |
| `submission.claim_key_conflict` | 409 | A new claim's `claim_key` collides with a non-erased claim in the same workarea; or a reassignment would collide at its destination without recording a re-key. | `claim_key` is unique per (tenant, workarea) over non-erased rows; a colliding move is refused unless the journalled move event records the re-key (D7). |
| `submission.universe_refused` | 409 | A claim write addresses the tenant's `universe` root. | Universe is permanently `committed` governance structure and refuses direct claim writes — claims live in crew workareas (D7). |

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
| `review.author_in_lineage` | 403 | `review/next` would serve, or a verdict/ratification/reopen would count, for a key or principal that appears anywhere in the row's derivation lineage closure — pending **and** ratified edges, computed at that moment. The original author reopening its own rejection is this code. | **R2: the reviewer is never the author** — enforced at promotion time against the author and the lineage closure, not just at key issuance. "I edited an ancestor" disqualifies exactly as "I wrote it" does ([roles-and-keys](../architecture/roles-and-keys.md)). |
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

## `upload` — attachment sessions

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `upload.session_invalid` | 409 | A part or complete addresses a session that is expired, already complete, or aborted (`details.reason` says which). | A retry resumes or restarts the session — the behavior is the session state machine's, not improvised per client (D3). |
| `upload.digest_mismatch` | 422 | At complete, the assembled bytes do not hash to the session's `expected_digest`. | An attachment is named by the SHA-256 of its plaintext and bound into the signed envelope by that digest; bytes that are not what was promised are never stored as if they were (D3). |

## `lifecycle` — erased content

| Code | HTTP | Fires when | The rule it names |
|---|---|---|---|
| `lifecycle.erased` | 410 | A fetch addresses an artifact whose DEK was destroyed by a journalled erase. | **R4: no silent deletion** — erasure is a journalled crypto-shred scoped to body and attachment bytes; the journal trail and attribution remain and the response says *erased*, never *not found* (D4). |

`410` deliberately differs from `404`: erasure is an attested act with an inventory, visible to
anyone who could read the artifact before. What a key could never see stays `access.not_found`.

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
| 400 | `request.validation`, `request.idempotency_key_required` |
| 401 | `auth.missing`, `auth.invalid` |
| 403 | `access.verb_ceiling`, `access.grant_refused`, `access.author_only`, `access.issuer_ceiling`, `access.role_exclusive`, `access.banned`, `review.author_in_lineage`, `review.ratifier_is_proposer`, `simulation.audience_refused` |
| 404 | `access.not_found` |
| 409 | `idempotency.in_progress`, `idempotency.digest_mismatch`, `submission.tip_conflict`, `submission.claim_key_conflict`, `submission.universe_refused`, `workarea.frozen`, `workarea.transition_refused`, `review.serve_invalid`, `simulation.ended`, `simulation.taint_refused`, `simulation.tip_refused`, `upload.session_invalid`, `export.not_ready` |
| 410 | `lifecycle.erased` |
| 413 | `request.payload_too_large` |
| 422 | `submission.scan_veto`, `submission.attachment_missing`, `submission.simulation_id_required`, `submission.lineage_required`, `upload.digest_mismatch`, `settings.retired_key`, `settings.invalid` |
| 429 | `limit.rate`, `limit.quota` |
| 500 | `system.internal` |
| 503 | `system.unavailable` |
