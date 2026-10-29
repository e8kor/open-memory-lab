# Deployment

**Status:** design draft for review · decided, not built

This page is the single declaration site for D15: what runs on Kubernetes, how
many of each, what state and credentials each workload holds, and how the two
Helm profiles differ. The guarantees the workloads implement are declared
elsewhere and only *placed* here: the commit point and chain in
[storage](../architecture/storage.md), key custody and tenancy controls in
[security](../architecture/security.md), the reconciler's job in
[reconciliation](../architecture/reconciliation.md), the erasure inventory this
page's retention windows feed in
[audit and lineage](../architecture/audit-and-lineage.md). The deployment
principle: the service is a set of workloads on Kubernetes and nothing in it
names a machine.

One Helm umbrella chart, two value profiles — `local` and `scaled`. The same
workload set runs in both; the profiles differ only in what backs Postgres and
the object store, and in the durability caveat that difference creates.

## Workloads

Everything durable lives in Postgres and the object store. The OML binaries are
deliberately stateless pods: any of them can be killed and rescheduled, and
recovery is a matter of reading the system of record, not of recovering a
volume. Identity belongs to the deployment, not to any volume or process: the
shared signing key attests the deployment
(see [storage](../architecture/storage.md)).

| Workload | Kind, replicas | State it holds | Credentials it holds |
|---|---|---|---|
| `oml-server` | Deployment, N stateless replicas behind one Service | None in the pod. All state is Postgres; chain-head assignment is serialized in Postgres, not in any replica. | The **deployment signing key** (K8s Secret or KMS reference) — the only workload that loads it. A Postgres application role (RLS-governed). KMS unwrap grants for serving-path decryption. Object-store credentials for the quarantine and bodies prefixes. |
| `oml-lakewriter` | Deployment, exactly 1 replica (singleton; leader lease, see below) | None in the pod. Recovery authority is the Delta log's per-table transaction versions; the Postgres frontier column is a cache. | A SET-tenant, RLS-scoped Postgres session role (no BYPASSRLS). A KMS grant limited to generate-data-key / encrypt — **never decrypt**. Object-store write credentials for the per-tenant lake prefixes. Never the signing key. |
| `oml-reconciler` | CronJob (settings below) | Its persisted cursor (last reconciled seq + running chain hash) lives in Postgres and is itself journalled — nothing in the pod. | Read-only Postgres credentials. Object-store read access — it verifies ciphertext digests and needs no key. A service credential to `oml-server`, through which its repairs are emitted as journalled outbox re-enqueues. No KMS decrypt, no signing key. |
| `oml-indexer` | Deployment | None in the pod; embeddings and tsvector rows live in Postgres and are in the erasure inventory. | Its own per-tenant credentials, never the signing key; the exact KMS grant is declared in [security — key custody](../architecture/security.md). |
| Postgres | CloudNativePG cluster (local) / managed service (scaled) | Everything: journal, fold tables, outbox, indexes, read-event log. | n/a (it *is* the store); application roles are granted per workload as above. |
| MinIO / S3 | StatefulSet via pinned MinIO chart (local) / external S3 (scaled) | Quarantine objects, ciphertext bodies, Delta tables. | n/a; per-tenant storage credentials when externalized, per [security](../architecture/security.md). |
| bootstrap Job | Helm hook, runs once on first install | None | A one-time Postgres credential. It mints the show-once root operator key, journalled as issued-without-value ([roles and keys](../architecture/roles-and-keys.md)). |

There is no web UI workload; administration is the [CLI](../cli/reference.md),
which talks to the same `oml-server` Service from outside the cluster.

## The two profiles

### `local` — one machine, honest about what that costs

Runs on k3d or kind on a single machine. Backing choices:

- **Postgres:** a single-node CloudNativePG cluster. With no synchronous
  standbys, the journal's durability domain is reduced to one machine's disk —
  a committed-and-acknowledged write can be lost with that disk. This is the
  one documented reduction of a [storage](../architecture/storage.md)
  guarantee, stated here rather than discovered in an incident review.
- **Object store:** MinIO at a **pinned version** known to support
  `If-None-Match` conditional put. Delta commits on S3-compatible stores
  require put-if-absent (the second fence below); an unpinned MinIO that
  silently drops the precondition would turn "exactly-once" into "usually
  once", so the chart refuses to float this image tag.

The local profile exists so the whole system — gate, chain, lake,
reconciliation — runs end to end on a laptop. It is a correctness environment,
not a durability one.

### `scaled` — managed Postgres, S3

- **Postgres:** a managed service configured with **synchronous quorum
  replication for the journal domain** — `synchronous_commit =
  remote_write`/`remote_apply` with quorum synchronous standbys, as
  [storage](../architecture/storage.md) requires. The chart does not manage
  this Postgres; the profile documents the settings the deployment must verify,
  and `oml doctor` reports whether the connected Postgres satisfies them
  ([observability](observability.md)).
- **Object store:** S3, with conditional put (supported natively). Per-tenant
  lake prefixes and, when externalized, per-tenant storage credentials per
  [security](../architecture/security.md).
- `oml-server` scales horizontally; every replica loads the same signing key,
  and chain serialization stays in Postgres, so adding replicas adds API
  capacity without touching ordering. Per-tenant write throughput is still
  bounded by the per-tenant chain lock — more replicas do not change that.

Where the scale ceiling actually sits is unmeasured — tracked as
[FR-7](../product/roadmap.md).

## Local quickstart

!!! warning "Design mockup"
    Nothing below exists. No chart is published, no image is built, no CLI is
    compiled. This transcript is the committed shape of the experience — an
    exit criterion for the deployment work, not a record of it.

```console
$ k3d cluster create oml
$ helm repo add oml https://example.github.io/open-memory-lab/charts
$ helm install oml oml/oml --namespace oml --create-namespace \
    --values values-local.yaml
$ kubectl --namespace oml wait --for=condition=Ready pod --all --timeout=300s
$ kubectl --namespace oml port-forward svc/oml-server 8080:8080 &
$ oml bootstrap --endpoint http://localhost:8080
root operator key (shown once, journalled as issued-without-value):
  oml_op_************************
```

From here the ceremony continues in [cli/sessions.md](../cli/sessions.md):
create a tenant, issue its first admin key, create workareas, issue crew keys.

## The lake-writer singleton and its two fences

Exactly one lake-writer drains the outbox (declared in
[storage](../architecture/storage.md)). Kubernetes alone cannot promise
"exactly one": a Deployment with `replicas: 1` still produces two live pods
during node partitions, kubelet restarts, and rolling updates. The design
therefore uses two fences, and only the second one is load-bearing:

1. **Leader lease** (a `coordination.k8s.io` Lease): the ordinary mechanism.
   A pod drains the outbox only while it holds the lease. This keeps the
   common case quiet — one writer, no conflicts — but it is an optimization,
   because a lease can be held by a pod the API server believes dead.
2. **Delta conditional put + transaction-id conflict**: the fence that is
   allowed to be the last line. Every Delta commit is a put-if-absent of the
   next log entry, and every batch records appId `oml-lakewriter` with
   version = last applied outbox seq per table. A split-brain second writer
   either loses the conditional put (its commit is refused by the store) or
   finds the txn version already advanced and skips the batch. Either way the
   outcome is a retried, deterministic batch — never a duplicate application.

The cost of this arrangement: failover is not instant (lease expiry before a
new pod drains), so outbox frontier lag is the metric that pages, not pod
restarts ([observability](observability.md)). Whether delta-rs exposes
transaction ids exactly as this relies on is verified at implementation time —
the conformance test is [FR-3](../product/roadmap.md).

## Reconciler CronJob

The reconciler is a CronJob, not a Deployment, because its work is a bounded
incremental pass ([reconciliation](../architecture/reconciliation.md)), and
because the CronJob contract gives two settings the design depends on:

```yaml
# design sketch — values are profile defaults, not measurements
spec:
  schedule: "…"                # profile value; local defaults to hourly
  concurrencyPolicy: Forbid    # two concurrent passes would race the cursor
  jobTemplate:
    spec:
      activeDeadlineSeconds: … # a pass that cannot finish is killed, not left
                               # holding its read snapshot open indefinitely
```

- `concurrencyPolicy: Forbid` — the cursor (last reconciled seq + running
  chain hash) is single-writer by design; a second concurrent pass would
  either duplicate findings or fight over the cursor. Forbid makes a slow pass
  delay the next one instead of overlapping it.
- `activeDeadlineSeconds` — set in both profiles. A reconciliation pass that
  exceeds it is a finding in itself (reported by the next pass as cursor lag),
  and killing it is safe: the cursor advances only at journalled checkpoints,
  so a killed pass redoes work, never corrupts it. The occasional
  full-genesis verification is a separately named Job with its own, larger
  deadline.

The deadline values themselves are unmeasured; they ship as profile defaults
to be revised when reconciliation has numbers (the measurement belongs to the
same effort as [FR-7](../product/roadmap.md)).

## Probes

`oml-server` exposes `/healthz` and `/readyz`
([HTTP API](../api/http-api.md)); the other binaries expose the same pair on a
local port for the kubelet.

- **Liveness (`/healthz`)**: process is up and its event loop responds.
  Deliberately shallow — a Postgres outage must not make the kubelet restart
  every pod in a loop.
- **Readiness (`/readyz`)**: the pod may receive traffic. For `oml-server`
  this is where the **startup fork check** lives: a replica is not ready until
  the stored chain head verifies against the last signed head
  ([storage](../architecture/storage.md)). A failover that lost committed
  journal events therefore surfaces as a deployment that refuses to become
  ready — loud, before any write is accepted — rather than as a silently
  forked chain. The cost is accepted: after such an event the service stays
  down until an operator resolves it; there is no automatic
  "accept-and-fork" path.
- The lake-writer's readiness includes object-store reachability and a
  conditional-put self-test against a scratch key, so a store that lost
  `If-None-Match` support (a MinIO upgrade outside the pin) is caught at
  rollout, not at the first real commit.

## Backups, retention windows, and the erasure SLO

Backups are a deployment concern with a correctness consequence: every backup
is a copy class in the [erasure inventory](../architecture/audit-and-lineage.md),
and the windows chosen here become terms in the erasure SLO.

| Store | Mechanism | Retention window's role |
|---|---|---|
| Postgres | Base backups + WAL archiving (CloudNativePG-managed locally; the managed service's facility when scaled) | Plaintext-derived rows — pgvector embeddings, tsvector entries — are removed from live tables at erase time but **persist in base backups and WAL until the retention window expires**. The window is therefore an upper bound on how long erased-memory derivatives survive, and it is stated, per tenant policy, in the evidence export. |
| Delta tables | Table history until `VACUUM` + checkpoint cleanup, run inside the lake-writer | Old parquet versions and checkpoints hold **ciphertext** of erased bodies until vacuumed. The DEK is already destroyed, so the bytes are unreadable, but the inventory still names them, and the vacuum cadence is the stated bound on their lifetime. |
| Object store | Versioning (if enabled) on quarantine and bodies prefixes | Same shape: ciphertext versions survive until lifecycle rules expire them; the lifecycle window is the stated bound. Quarantine objects of refused submissions get a short, named expiry of their own. |
| Read-event log | Partition drops on the retention window set by tenant policy ([retrieval](../architecture/retrieval.md)) | Not a backup, but the same discipline: the window is declared, and exports scope to it. |

The composition rule: **the erasure SLO for a copy class is the longest window
that can still contain it.** For plaintext-derived data that is the Postgres
backup retention window; for ciphertext copies it is vacuum cadence and
lifecycle rules — and for those, key destruction already provides the
security property, so the windows bound tidiness, not exposure. The profiles
ship defaults for every window; none of the defaults is a measured or
regulator-sourced number, and the sanctioned claim stays what
[audit and lineage](../architecture/audit-and-lineage.md) states: a journalled
erase event whose effect on each copy class is listed.

Restores are the dual hazard: restoring a Postgres backup resurrects erased
plaintext-derived rows and rewinds the chain head. A restore is therefore an
operator ceremony, not a routine act — the restored deployment must re-run the
fork check, re-apply erase events newer than the backup (replayable because
erase is journalled), and let a full-genesis reconciliation pass confirm the
lake and journal agree before readiness. The ceremony's transcript shape
belongs to [cli/sessions.md](../cli/sessions.md); its existence is decided
here, not built.

## What this page does not decide

Resource requests and limits, node counts, ingress and TLS termination, and
the Prometheus stack that scrapes the metrics are deployment-site choices the
chart parameterizes but this design does not fix. Postgres sizing beyond "the
journal domain is synchronous" is open until [FR-7](../product/roadmap.md) has
a measured ceiling.
