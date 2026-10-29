<!--
id: adr/ADR-0016-kubernetes-workloads
tier: engineering
status: accepted design
answers: why the Kubernetes profile is a set of workloads with two Helm profiles, and why deployment identity replaces volume identity
-->

# ADR-0016 — Kubernetes workloads, two Helm profiles, deployment identity replaces volume identity

**Status:** accepted design · 2026-10-29 · **scope: this record governs the Kubernetes
profile.** Standalone and docker are first-class profiles of their own under
[ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md), which also supersedes the
every-replica-signs portion of d4 with cluster-mode lead/follower roles (ADR-0023 d13).

## Context

Two rules anchor the deployment. The first: *the service is a set of workloads
on Kubernetes, and nothing in it names a machine*. The second: no volume holds
the deployment's identity. PostgreSQL is the system of record and the
object store holds every body ([storage](../architecture/storage.md)); the
signature attests the deployment through a shared signing key in a Kubernetes
Secret or KMS, not a file a first boot wrote to a volume claim. That choice lets
every OML binary be a stateless pod — and it moves the hard singleton problem
from "protect a volume" to "ensure exactly one outbox drainer", which this
record answers with a fence that does not trust Kubernetes to count to one.

The full workload table, profile values, quickstart, backup windows and probe
semantics are declared in [deployment](../operations/deployment.md); this record
is the decision and its reasons. Decided, not built — no chart exists.

## Decision

- **d1 — The workload set.** `oml-server` as a Deployment of N stateless
  replicas behind one Service; `oml-lakewriter` as a singleton (d3);
  `oml-reconciler` as a CronJob with `concurrencyPolicy: Forbid` and
  `activeDeadlineSeconds` set, because its cursor is single-writer and a stuck
  pass must die rather than hold its snapshot open; `oml-indexer` as a
  Deployment; Postgres via CloudNativePG locally and a managed service in
  production, with the journal domain on synchronous quorum replication per
  [storage](../architecture/storage.md); MinIO (pinned to a version supporting
  `If-None-Match`) locally, S3 when scaled; a one-shot bootstrap Job that mints
  the show-once root operator key. There is no web UI workload — administration
  is the [CLI](../cli/reference.md) — and no crew workload: reviewing crews are
  external services holding keys ([admission](../architecture/admission.md)),
  never workloads OML launches. Nothing in the set names a machine.

- **d2 — One umbrella Helm chart, two value profiles.** `local` runs the entire
  system on one machine under k3d or kind; `scaled` targets managed Postgres and
  S3. The same workload set runs in both; the profiles differ only in what backs
  Postgres and the object store, and in the durability caveat that difference
  creates — the local profile's single-node Postgres reduces the journal's
  durability domain to one disk, and the [deployment](../operations/deployment.md)
  page states that reduction rather than leaving it to an incident review.

- **d3 — The lake-writer is a singleton held by a lease and fenced by the
  store.** Kubernetes `replicas: 1` still yields two live pods across node
  partitions and rolling updates, so the design uses two fences and lets only
  the second be load-bearing: a `coordination.k8s.io` leader lease keeps the
  common case quiet, and **Delta conditional put plus the transaction-id
  conflict** is the line that holds — a split-brain second writer loses the
  put-if-absent on the next log entry or finds the per-table txn version already
  advanced, and either way the batch is retried deterministically, never applied
  twice. The lease is an optimization; correctness never depends on it. Whether
  delta-rs exposes transaction ids exactly as this relies on is unverified —
  [FR-3](../product/roadmap.md) is the conformance test, and the capability
  citation rule is in [ADR-0006](ADR-0006-pure-rust.md).

- **d4 — Deployment identity replaces volume identity.** Every OML pod is
  disposable: recovery is reading the system of record, never recovering a
  volume. The signing key lives in a Secret or KMS and every `oml-server`
  replica loads the same one; what is unique is chain-head assignment in
  Postgres ([storage](../architecture/storage.md)). The guard a volume-identity
  design would put on storage lives in the protocol instead: the **startup fork check** refuses to
  accept writes until the stored chain head verifies against the last signed
  head, so a failover that lost committed journal events surfaces as a
  deployment that refuses readiness, not as a silently forked chain.

- **d5 — Probes are wired to `/healthz` and `/readyz`, and readiness carries the
  correctness checks.** Liveness is deliberately shallow so a Postgres outage
  does not put every pod in a restart loop; readiness is where `oml-server` runs
  the fork check and where the lake-writer runs a conditional-put self-test
  against a scratch key, so a MinIO upgrade outside the pin that dropped
  `If-None-Match` is caught at rollout, not at the first real commit. Probe
  semantics are declared in [deployment](../operations/deployment.md).

## Consequences

- Failover of the lake-writer is not instant: a new pod drains only after lease
  expiry, so **outbox frontier lag is the metric that pages**, not pod restarts
  ([observability](../operations/observability.md)). That is the cost of a fence
  that tolerates a wrongly-held lease instead of trusting it.
- After a failover that lost committed events, the service stays down until an
  operator resolves the fork — there is no automatic accept-and-fork path. Loud
  unavailability is chosen over silent divergence.
- The local profile is a correctness environment, not a durability one, and
  says so; anyone running `local` in production has been told what they gave
  up.
- Statelessness makes upgrades and rescheduling ordinary, but it concentrates
  operational risk in the two stores: Postgres backup/retention windows become
  terms in the erasure SLO, and a Postgres restore is an operator ceremony with
  a mandatory fork check and reconciliation pass
  ([deployment](../operations/deployment.md)).
- Resource sizing, ingress, TLS termination and the Postgres scale ceiling are
  deliberately not fixed by this record; the ceiling is
  [FR-7](../product/roadmap.md).

## Rejected alternatives

- **A StatefulSet of one whose identity is a volume.** Anchoring the deployment
  in a volume claim — the signing key minted onto the volume at first boot, a
  `Retain` storage class, `ReadWriteOncePod`, a chart obligated to guard the
  claim — makes losing the volume losing the deployment's identity, and makes
  every reschedule and upgrade a custody event for one PVC. OML's identity
  material belongs in a Secret or KMS and its state in Postgres and the object
  store; a volume would add a third custody domain and buy nothing the startup
  fork check (d4) does not already give.
- **Operator-pattern CRDs for v1.** An OML operator with `OmlTenant`/`OmlDeployment`
  custom resources would automate day-2 actions, at the price of building and
  maintaining a controller, a CRD schema lifecycle and an RBAC surface before a
  single deployment exists to automate. One umbrella chart with two profiles
  covers the committed shapes; an operator becomes worth revisiting when there
  is a fleet whose day-2 toil is observed, not predicted. Rejected for v1, not
  forever.
- **docker-compose as the local story.** Compose would start the containers on
  a laptop with less ceremony than k3d, and it would make the local environment
  a second deployment payload: its own service definitions, its own networking
  and secrets handling, drifting from the chart that production runs. k3d/kind
  run the **same chart** with a values file, so every local run rehearses the
  real deployment — including the probes, the bootstrap Job and the MinIO pin —
  and a defect in the chart is found on a laptop instead of at rollout. The
  cost (a local Kubernetes is heavier than Compose) is accepted for that.
  This rejection stands as written: it argued against compose *as a rehearsal
  of the Kubernetes deployment*. Compose as a first-class install target in
  its own right is a different requirement, decided later in
  [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md), where the drift
  risk this rejection priced is answered by a conformance suite that must pass
  on all three profiles ([FR-22](../product/roadmap.md)), not by refusing the
  payload.
