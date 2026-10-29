<!--
id: operations/failure-modes
tier: engineering
status: design draft for review · decided, not built
answers: what each dependency failure does to each client-facing operation; which refusals fire with which typed error code; which degradations are visible in which signal
-->

# Failure modes

**Status:** design draft for review · decided, not built — every cell below is a
consequence derived from the architecture pages, not an observed behavior;
nothing has failed because nothing has run.

This page is the dependency-failure matrix: for each dependency a deployment
runs on, what each client-facing operation does while that dependency is
unavailable. Every cell gives one of three honest answers:

- **refuse** — the operation fails closed with a typed code from the
  [error taxonomy](../api/errors.md); the code is named in the cell.
- **degrade** — the operation keeps working with a stated reduction, and the
  reduction is visible in a named [observability](observability.md) signal —
  never silent.
- **unaffected** — the operation does not touch the dependency.

The standing rule, restated once: where the service cannot establish that an
act is permitted and durable, the act does not happen
([errors](../api/errors.md); [admission — fail closed](../architecture/admission.md#fail-closed)).
No dependency failure is absorbed by weakening a guarantee — a scanner that
cannot load never admits unscanned bytes, absent reviewers never relax `k`, a
lost quorum never acknowledges an undurable write. The operator procedures for
the conditions this page names live in the [runbooks](runbooks.md).

## The matrix

Each row is a single-dependency failure; compound failures compose, refuse
winning over degrade. Columns are the client-facing operations: submit (memory
writes on either ring), recall/get (all reads), attachment upload and download,
verdict (the reviewer surface: serves and verdict recording), and the evidence
export. Row-by-row derivations follow the table.

| Dependency down | Submit | Recall / get | Attachment upload | Attachment download | Verdict | Evidence export |
|---|---|---|---|---|---|---|
| Postgres primary | refuse `system.unavailable` | refuse `system.unavailable`; cluster followers degrade (staleness grows) | refuse `system.unavailable` | as recall/get | refuse `system.unavailable` | refuse `system.unavailable` |
| Postgres replica | unaffected while quorum holds; refuse `system.unavailable` on quorum loss | degrade: that follower stops serving; remaining replicas carry the load | as submit | as recall/get | as submit | as submit |
| KMS | refuse `system.unavailable` | degrade: DEK-cache hits serve within TTL; misses refuse `system.unavailable` | unaffected until bind | as recall/get | recording unaffected; new serves refuse `system.unavailable` | unaffected |
| Object store | degrade: Postgres-placed bodies admit; bucket-placed refuse `system.unavailable` | degrade: Postgres-placed bodies serve; object-placed refuse `system.unavailable` | refuse `system.unavailable` | refuse `system.unavailable` | recording unaffected; new serves of attachment-bearing claims refuse `system.unavailable` | unaffected |
| Deterministic scanner | cannot fail independently: a replica that cannot load its rule set is not ready; an unclassifiable scan failure refuses that write (`system.internal`) | unaffected | unaffected | unaffected | unaffected | unaffected |
| Indexer | unaffected | degrade: search misses promotions newer than the backlog; get by id unaffected | unaffected | unaffected | unaffected | unaffected |
| Lake-writer | unaffected | unaffected | unaffected | unaffected | unaffected | unaffected — degradation is non-client (frontier lag, erasure-SLO drain term) |
| Reviewer capacity | unaffected: lands `held`, deliberately not an API error | unaffected | unaffected | unaffected | the condition **is** absent verdicts; nothing new to refuse | unaffected |

## Postgres primary

Postgres is the system of record and the only transactional commit point
([storage](../architecture/storage.md)): every journalled act — submit, verdict,
upload-session state, export start — is a Postgres transaction, so the primary
being unreachable refuses them all with `system.unavailable` (503, retryable),
exactly as the taxonomy defines that code. `/readyz` reports the same condition
to the orchestrator ([deployment — probes](deployment.md#probes)).

Reads split by profile:

- **Single-replica profiles** (standalone, docker, kubernetes without cluster
  mode): every read also refuses `system.unavailable` — the serving store is
  the store that is down.
- **Cluster mode** ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)):
  followers keep serving promoted-row recall and get from their local replicas.
  The degradation is staleness: replication has halted, so the follower's
  staleness bound — replication lag, exposed as a follower metric
  ([retrieval — reads in cluster mode](../architecture/retrieval.md#reads-in-cluster-mode))
  — grows without bound until the primary returns. Own-held reads and
  submit-then-get sequences route to the primary by the routing policy and
  refuse with it.

The evidence export reads the journal, the read log and the fold tables — all
Postgres — so both starting a bundle and fetching one refuse.

## Postgres replica

Two different things are called a replica, and they fail differently:

- **A journal-domain synchronous standby** (the kubernetes `scaled` profile,
  [storage — durability domain](../architecture/storage.md#durability-domain)).
  Losing one standby with quorum intact changes nothing. Losing quorum stalls
  the commit path: every journalled act — submit, verdict, upload progress —
  refuses `system.unavailable` rather than acknowledging a write the quorum
  does not hold. The signal is store-level, not an OML metric: synchronous-standby
  quorum health, listed under store-level signals in
  [observability](observability.md).
- **A cluster follower's local read replica.** The follower fails `/readyz`
  and takes no traffic; promoted-row reads shift to the remaining replicas and
  the lead. Capacity degrades, correctness does not. Writes are unaffected —
  they were forwarded to the lead regardless.

## KMS

The KMS wraps and unwraps per-artifact DEKs; the KEK never leaves it
([security](../architecture/security.md)).

- **Submit** performs DEK generation and the KMS wrap *before* the commit
  transaction opens ([admission](../architecture/admission.md)); with KMS
  unreachable nothing can be encrypted, so the write refuses
  `system.unavailable`. Nothing is ever admitted unencrypted.
- **Recall, get, attachment download** decrypt on serve
  ([retrieval — decrypt on serve](../architecture/retrieval.md#decrypt-on-serve)).
  The DEK serving cache ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d3)
  masks the outage for artifacts whose keys are cached, within the cache TTL;
  a cache miss refuses `system.unavailable` for the body-bearing read. The
  degradation window is exactly the TTL, which is itself a term in
  [FR-17](../product/roadmap.md)'s erasure bound.
- **Verdict**: recording a verdict against a valid serve is a journalled write
  with no decryption — unaffected. Creating a *new* serve decrypts the held
  body and its attachments for the reviewer, so serves refuse
  `system.unavailable`; review drains only as far as outstanding serves reach.
- **Attachment upload** sessions store and digest-verify bytes; the DEK work
  happens at the submission that binds the digest, which is where a KMS outage
  refuses (the precise encryption moment for session bytes is an ADR-0004 open
  point, [FR-15](../product/roadmap.md)).
- **Evidence export** is digest-only and verifies signatures keylessly — the
  bundle never decrypts a body — so it is unaffected.

One honesty note: no KMS-call metric is named in
[observability](observability.md) today; the visible symptom of a KMS outage
is the 5xx rate in `oml_http_request_duration_seconds`. A dedicated KMS-call
signal is a requested addition to that page, not something this page can
invent.

## Object store

The object store holds quarantine (`oml-quarantine/`), promoted ciphertext
objects (`oml-bodies/`), and the lake prefix (`oml-lake/`).

- **Submit** degrades by placement: a body the size threshold places in
  Postgres admits normally; a bucket-placed body's quarantine PUT precedes the
  commit transaction and its failure refuses the write `system.unavailable`
  ([admission — quarantine placement](../architecture/admission.md#quarantine-placement)).
  Which placement each body takes is recorded per row (`body_ref`).
- **Recall and get** degrade the same way: bodies whose ciphertext lives in
  Postgres serve; `body_ref = object` fetches refuse `system.unavailable`.
- **Attachment upload and download** refuse `system.unavailable` outright —
  attachment bytes are always object-placed
  ([storage — attachments](../architecture/storage.md#attachments-d3)).
- **Verdict**: recording against a valid serve is unaffected; a new serve of a
  claim with attachments must serve every attachment before a verdict can
  count ([admission](../architecture/admission.md)), so those serves refuse.
- **Evidence export** is unaffected: the bundle is journal, read log and
  statistics from Postgres, digests throughout.
- **Non-client effect**: the lake-writer cannot commit, so
  `oml_outbox_frontier_lag_seconds` grows
  ([runbook: frontier lag growing](runbooks.md#frontier-lag-growing)); no read
  is affected because the lake serves no client read
  ([retrieval](../architecture/retrieval.md)).

## Deterministic scanner

The scanner is in-process deterministic rules inside `oml-server`, not a
workload — it cannot be "down" while the server is up
([admission — the rule-set registry](../architecture/admission.md#the-rule-set-registry)).
The two failures that actually exist:

- **Rule-set load failure at boot.** A replica that cannot load its pinned
  `ruleset_vN` fails readiness and serves nothing — the failure surfaces as
  lost capacity on `/readyz`, never as unscanned admission. While any ready
  replica exists, every column is unaffected.
- **Unclassifiable scan failure mid-call.** The write is refused
  (`system.internal`) rather than admitted unscanned — admission's own rule,
  restated here because this is the page that would otherwise imply a softer
  answer.

## Indexer

`oml-indexer` computes FTS and embedding rows after promotion
([retrieval — indexing](../architecture/retrieval.md#indexing)). With it down:

- **Recall degrades honestly**: promotions newer than the backlog are
  invisible to FTS and vector search until indexed — the gap between
  "promoted" and "findable", shown by `oml_index_backlog_depth` and
  `oml_index_lag_seconds`. Get by id, and recall hits over already-indexed
  rows, are unaffected.
- Everything else is unaffected: the indexer sits on no write path and no
  serve path.

One erase-time interaction is named rather than implied away: erase removes
embedding and `tsvector` rows from live tables; an indexer outage delays new
indexing, not erasure, because the removal is the serving side's act
([audit and lineage](../architecture/audit-and-lineage.md) owns the inventory).

## Lake-writer

Release is asynchronous and follows promotion; reads are served from Postgres
([admission](../architecture/admission.md),
[retrieval](../architecture/retrieval.md)) — so a lake-writer outage touches
no client-facing column at all. What it does touch:

- **Frontier lag grows**: `oml_outbox_frontier_lag_events` /
  `oml_outbox_frontier_lag_seconds`
  ([runbook: frontier lag growing](runbooks.md#frontier-lag-growing)).
- **The lake erasure SLO keeps its clock running**: an erase MERGE waiting in
  the undrained outbox stretches the drain-lag term of the lake ciphertext
  bound ([delta-tables — the erasure-SLO interaction](../data/delta-tables.md#the-erasure-slo-interaction)).
  The erase *event* is journalled and the DEK already destroyed — the content
  is unreadable from the erase instant; what persists longer is ciphertext
  copies awaiting cleanup, stated as such.
- Analytics and reconciliation targets go stale; the reconciler reports the
  gap rather than guessing
  ([reconciliation](../architecture/reconciliation.md)).

## Reviewer capacity

Review capacity is external crews; the provider runs no model
([admission — review capacity](../architecture/admission.md#review-capacity-external-crews)).
Absent capacity is the one dependency failure that is deliberately **not** an
API error:

- **Submit is unaffected**: the write lands `held` and waits. Review
  unavailability is surfaced by `oml status` / `oml doctor`
  (`review_availability: false`) and by `oml_review_queue_depth` /
  `oml_review_queue_oldest_age_seconds`, never by a refusal at capture
  ([errors](../api/errors.md) states this exclusion;
  [runbook: review unavailable](runbooks.md#review-unavailable)).
- **The degradation is promotion**: nothing promotes, held items age toward
  `held_item_expiry` and dead-letter on reaching it — a journalled event,
  never a silent drop. A deployment in this state is green on every probe and
  down for its purpose, which is why the lag signals, not pod status, are the
  health signals that matter ([observability](observability.md)).
- Reads, uploads and the evidence export are unaffected; the export will
  simply show a growing held population, which is the honest record of the
  condition.

The cost of this shape is stated where it is decided: failing closed converts
under-provisioned review into an availability problem for new knowledge,
visibly, instead of converting it into a trust problem silently
([admission — fail closed](../architecture/admission.md#fail-closed)).

## What this page does not cover

Timing. How long each refusal window lasts, how fast staleness grows, how
quickly a follower drains its buffered read events after reconnecting — every
such number is unmeasured, tracked by the rows the architecture pages already
cite ([FR-2](../product/roadmap.md), [FR-17](../product/roadmap.md),
[FR-24](../product/roadmap.md), [RS-13](../product/roadmap.md)). This page
commits to the shape of each failure, and refuses to guess its duration.
