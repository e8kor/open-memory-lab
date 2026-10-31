<!--
id: operations/deployment
tier: engineering
status: design draft for review · decided, not built
answers: what runs where and how many of each; what state and credentials each workload holds; how the three install profiles differ
-->

# Deployment

**Status:** design draft for review · decided, not built

This page is the single declaration site for D15: what runs where, how many of
each, what state and credentials each workload holds, and how the three install
profiles differ. The guarantees the workloads implement are declared elsewhere
and only *placed* here: the commit point and chain in
[storage](../architecture/storage.md), key custody and tenancy controls in
[security](../architecture/security.md), the reconciler's job in
[reconciliation](../architecture/reconciliation.md), the erasure inventory this
page's retention windows feed in
[audit and lineage](../architecture/audit-and-lineage.md), and the per-profile
guarantee differences in [commitments](../product/commitments.md). The
deployment principle ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)):
**the service is the same four processes wherever they run; the profile chooses
their supervisor and their stores.**

## The three install profiles

Three first-class profiles, one system:

- **`standalone`** — one host, no orchestrator. The four binaries run as child
  processes of one supervisor (`oml standalone up`); Postgres is a host service
  you bring; the object store is a local data directory; the KMS port is the
  `kms-file` software keyring.
- **`docker`** — one compose file. Each binary is a compose service; Postgres
  and MinIO are containers; the KMS port is the same software keyring, scoped
  by container mounts.
- **`kubernetes`** — the Helm umbrella chart, with its two existing value sets
  `local` and `scaled`. External or in-cluster Postgres, MinIO or S3, external
  KMS.

Identical in every profile: the four binaries, the Postgres schema and journal,
the commit protocol, the constitution, the bootstrap ceremony, the CLI, and the
conformance suite ([FR-22](../product/roadmap.md) — the same suite must be
green on all three). Profiles differ only in who supervises the processes, what
backs the object-store and KMS ports, and which guarantees weaken. Every
weakening is stated in the profile-dependent bounds of
[commitments](../product/commitments.md), never discovered. One commitment is
made explicit here because two of the profiles might be tempted to skip it:
C6's three deployment commitments — `FORCE ROW LEVEL SECURITY`, the
owner/migration role split, `SET LOCAL` — bind in **all three** profiles.

A profile is a deployment payload, not a runtime flag: the binaries are
profile-blind and read [one configuration surface](configuration.md) (file + env) naming their
adapters per [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md)
— Postgres URL, object-store URI (`s3://…` | `file:///…`), KMS adapter
(`aws` | `file`), scheduler mode, node role. The payloads are Helm values, the
compose file, and `oml-standalone.toml`; there is no `--profile` flag to drift
from them. Journalled settings apply unchanged: every effective configuration
is recorded by digest (`system.settings_applied`), so "which adapters, which
profile, when" is always answerable from the journal — in all three profiles
identically.

## Workloads

Everything durable lives in Postgres and the object store. The OML binaries are
deliberately stateless processes: any of them can be killed and restarted, and
recovery is a matter of reading the system of record, not of recovering a
volume. Identity belongs to the deployment, not to any volume or process: the
deployment signing key attests the deployment
(see [storage](../architecture/storage.md)) — and in cluster mode it is loaded
by exactly one process, the lead.

| Workload | Runs as (kubernetes / docker / standalone) | Replication | State it holds | Credentials it holds |
|---|---|---|---|---|
| `oml-server` (lead) | Deployment `oml-server-lead`, replicas 1, lease-guarded / compose service / supervised process | Exactly one (the lead lease, see below) | None in the process. All state is Postgres; chain-head assignment is serialized in Postgres, not in any process. | The **deployment signing key** — the only process in the deployment that loads it. A Postgres application role (RLS-governed, write). KMS decrypt and generate-data-key (admission and serving paths). Object-store credentials for the quarantine and bodies prefixes. In the non-KMS profiles, the keyring file mount. |
| `oml-server` (follower) | Deployment `oml-server-follower`, replicas N (default **0**), HPA on the read path / compose service under `--profile followers` / not available | Scale-out for reads; forwards every write verb to the lead | None in the process. | **Never the signing key.** KMS decrypt for the serving path only — never generate-data-key. An RLS-scoped **read** role plus read-log append. No keyring mount beyond serving-path unwrap. |
| `oml-lakewriter` | Deployment, 1 replica (lease + store fence, see below) / compose service, 1 replica / supervised process, 1 child | Singleton | None in the process. Recovery authority is the Delta log's per-table transaction versions; the Postgres frontier column is a cache. | A SET-tenant, RLS-scoped Postgres session role (no BYPASSRLS). A KMS grant limited to generate-data-key / encrypt — **never decrypt** (in the non-KMS profiles the keyring is simply not mounted / not readable). Object-store write credentials for the per-tenant lake prefixes. Never the signing key. |
| `oml-reconciler` | CronJob (`--once`) / compose service (`--every`) / supervised process (`--every`) — three schedulers, one binary (below) | One pass at a time, by scheduler contract or by construction | Its persisted cursor (last reconciled seq + running chain hash) lives in Postgres and is itself journalled — nothing in the process. | Read-only Postgres credentials. Object-store read access — it verifies ciphertext digests and needs no key. A service credential to `oml-server` for journalled repair re-enqueues. No KMS decrypt, no signing key. |
| `oml-indexer` | Deployment / compose service / supervised process | Per load | None in the process; embeddings and tsvector rows live in Postgres and are in the erasure inventory. | Its own per-tenant credentials, never the signing key; the exact KMS grant is declared in [security — key custody](../architecture/security.md). |
| Postgres | CloudNativePG cluster (`local`) or managed service (`scaled`) / `pgvector/pgvector:pg17` container / host service, bring-your-own | Per profile | Everything: journal, fold tables, outbox, indexes, read-event log. | n/a (it *is* the store); application roles are granted per workload as above, in all three profiles. |
| Object store | MinIO StatefulSet at the pinned tag (`local`) or S3 (`scaled`) / MinIO container at the **same pinned tag** / local data directory (`LocalFileSystem`) | Per profile | Quarantine objects, ciphertext bodies, Delta tables. | n/a; per-tenant storage credentials when externalized, per [security](../architecture/security.md). |
| bootstrap | Helm hook Job / one-shot compose service (`restart: "no"`) / `oml standalone init` | Runs on first install and re-runs on every upgrade ([upgrades](upgrades.md)) | None | A one-time Postgres credential (the owner role). It runs migrations, mints the keyring where the profile uses one, and mints the single-use, expiring **enrollment secret**; its logs carry progress only — no key or secret material. The root operator key is claimed from that secret with `oml bootstrap claim` ([CLI reference](../cli/reference.md)), shown once in the operator's terminal and journalled as issued-without-value ([roles and keys](../architecture/roles-and-keys.md)); the enrollment secret is consumed on first use. |

There is no web UI workload; administration is the [CLI](../cli/reference.md),
which talks to the same `oml-server` endpoint from outside, in every profile.

The bootstrap workload is not first-install-only: it re-runs on every version
upgrade to apply forward-only migrations under the owner role, and the order
in which the binaries then move — asynchronous workloads first, followers
next, the lead last — is specified in [upgrades](upgrades.md).

## The Kubernetes profile

One Helm umbrella chart, two value sets — `local` and `scaled`
([ADR-0016](../adr/ADR-0016-kubernetes-workloads.md), which governs this
profile). The same workload set runs in both; the value sets differ only in
what backs Postgres and the object store, and in the durability caveat that
difference creates.

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

The local value set exists so the whole system — gate, chain, lake,
reconciliation — runs end to end on a laptop under the real chart. It is a
correctness environment, not a durability one.

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
- `oml-server` scales by **followers**, not by signing replicas
  ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d13): the
  lead Deployment stays at one replica and is the only process that loads the
  signing key; `oml-server-follower` scales the read path
  (`--set server.followers.replicas=N`, default 0). Chain serialization stays
  in Postgres, so followers add read capacity without touching ordering.
  Per-tenant write throughput is bounded by the per-tenant chain lock as
  before; cross-tenant aggregate write throughput now funnels through the one
  lead — stated, unmeasured, tracked as [FR-24](../product/roadmap.md).

Where the scale ceiling actually sits is unmeasured — tracked as
[FR-7](../product/roadmap.md).

### Kubernetes quickstart

!!! warning "Design mockup"
    Nothing below exists. No chart is published, no image is built, no CLI is
    compiled. This transcript is the committed shape of the experience — an
    exit criterion for the deployment work, not a record of it.

```console
$ k3d cluster create oml
$ helm repo add oml https://e8kor.github.io/open-memory-lab/charts
$ helm install oml oml/oml --namespace oml --create-namespace \
    --values values-local.yaml
$ kubectl --namespace oml wait --for=condition=Ready pod --all --timeout=300s
$ kubectl --namespace oml port-forward svc/oml-server 8080:8080 &
$ kubectl --namespace oml get secret oml-enrollment -o jsonpath='{.data.token}' \
    | base64 -d \
    | oml bootstrap claim root-operator --url http://localhost:8080 --token-stdin
enrollment secret accepted — consumed (single-use)
root operator key (shown once, journalled as issued-without-value):
  oml_3fJ9***************************************
```

The enrollment secret the Helm job minted is exchanged, over the connection the operator opened,
for the root operator key — the key value appears only in the operator's terminal, never in a pod
log, and the consumed secret cannot be claimed twice
([CLI reference](../cli/reference.md#oml-bootstrap-claiming-the-root-operator-key)).

From here the ceremony continues in [cli/sessions.md](../cli/sessions.md):
create a tenant, issue its first admin key, create workareas, issue crew keys.

### The interim chart: the current build on Kubernetes

**Built** — `deploy/helm/oml`, decided in the proposed
[ADR-0033](../adr/ADR-0033-interim-helm-chart.md). CI job `chart` installs it on kind; no other
cluster is recorded running it. The chart holds the one server binary the build has,
`oml-server-lead`, and nothing else. It is the first revision of the umbrella chart above, not a
second chart: the designed workloads and the `local` and `scaled` value sets join it at their
milestones ([critical path](../product/critical-path.md)). Where it differs from the designed
chart:

| | Designed chart ([ADR-0016](../adr/ADR-0016-kubernetes-workloads.md), [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d16) | Interim chart |
|---|---|---|
| Server | `oml-server-lead`, lease-guarded, and `oml-server-follower`, default 0 | `oml-server-lead` alone: one replica under `Recreate`, which stops the old lead before the new one starts; the chain lock stays the fence (ADR-0033 d2) |
| Other workloads | lake-writer, reconciler CronJob, indexer, bootstrap hook Job | none — decided, not built |
| Signing key | a Secret or KMS; no volume holds the deployment's identity | the `kms-file` keyring on a ReadWriteOnce claim that `helm uninstall` keeps (ADR-0033 d3) |
| Migrations | the bootstrap Job, under the owner role | the lead, at boot, under its one Postgres URL (ADR-0033 d6) |
| Enrollment token | a Secret the bootstrap Job mints | the pod log: the lead prints a fresh token at every boot while no tenant exists (ADR-0033 d8) |
| Postgres | CloudNativePG (`local`) or a managed service (`scaled`) | external, through an existing Secret's key (ADR-0033 d6) |
| Object store | MinIO at the pinned tag (`local`) or S3 (`scaled`) | none: the build has no attachments |
| Value sets | `local` and `scaled` | one `values.yaml`, whose comments carry each value's meaning |
| Publication | a chart repository (the mockup above) | the `.tgz` and its signature bundle on each release page, from the first tag that runs the chart jobs ([CI and toolchain](../engineering/ci-and-toolchain.md#release-artifacts-sbom-signing-provenance)) |

The pod runs as the image's `oml` user, uid and gid 999, with `fsGroup: 999` under
`fsGroupChangePolicy: OnRootMismatch`, a read-only root filesystem, every capability dropped,
seccomp `RuntimeDefault` and no service account token (ADR-0033 d4, d5). Resource requests and limits
ship as comments in `values.yaml`, not values; no number for them is measured
([capacity](capacity.md)).

#### Install

Postgres comes first, as a Secret the chart reads and never writes:

```console
$ kubectl create namespace oml
$ kubectl --namespace oml create secret generic oml-postgres \
    --from-literal=OML_POSTGRES__URL='postgres://USER:PASSWORD@HOST:5432/DATABASE'
$ helm install oml deploy/helm/oml --namespace oml
$ kubectl --namespace oml rollout status deployment/oml-server-lead
```

From a release that carries the chart, the archive's URL replaces the checkout path:
`helm install oml https://github.com/e8kor/open-memory-lab/releases/download/vX.Y.Z/oml-X.Y.Z.tgz`,
with `--set image.digest=sha256:…` from that release's notes to pin the image. The lead applies
migrations at boot with that URL, so its role needs the rights to migrate.

#### Claiming the first admin key: the bootstrap race

While no tenant exists, the lead prints a single-use enrollment token on stdout at every boot.
The token sits in the pod log, and in whatever store the cluster ships container logs to, until a
claim consumes it, and whoever presents it first receives the first admin key and the tenant.
While the Service is cluster-internal, presenting it takes access to the cluster network or the
`pods/portforward` right; an Ingress opens it to anyone on the Ingress's network who has seen the
token. So the claim comes before any Ingress, through a port-forward, with the `oml` CLI on the
operator's machine (the release tarball's `oml`, or a `cargo build` of the checkout):

```console
$ TOKEN="$(kubectl --namespace oml logs deployment/oml-server-lead | grep -o 'enr_[0-9a-f]\{48\}' | tail -1)"
$ kubectl --namespace oml port-forward service/oml-server 8080:8080 &
$ oml --server http://127.0.0.1:8080 bootstrap claim --token "$TOKEN" \
    --tenant-name <tenant> --principal <you>
```

The key is printed once, and the CLI stores it in `~/.config/oml/credentials`
([CLI reference](../cli/reference.md)). To claim and set up a crew in one step instead, run the
quickstart through the same port-forward, as CI does:
`OML_URL=http://127.0.0.1:8080 OML_CLI=oml OML_ENROLLMENT_TOKEN="$TOKEN" bash scripts/quickstart.sh`.
A restart before the claim prints a new token, and the old one stops working: read the token
from the running pod, not from `--previous`.

Only then expose the API, if it needs to leave the cluster:
`helm upgrade oml deploy/helm/oml --namespace oml --reuse-values --set ingress.enabled=true
--set 'ingress.hosts[0].host=<host>'`. The install notes warn when an install enables the
Ingress; the chart does not refuse one, because it cannot see whether the claim happened. The
default Ingress path `/` also routes `/readyz`, whose body names the build
([HTTP API](../api/http-api.md#health)); narrow `ingress.hosts[].paths` to keep it off the
public route. The optional NetworkPolicy (`networkPolicy.enabled`) admits the API port from
`networkPolicy.ingressFrom` alone — by default, pods in the release's Kubernetes namespace; add
the ingress controller's namespace before enabling the Ingress — and lets the lead open
connections to DNS and the Postgres port alone.

#### Upgrades, restarts and uninstall

Every upgrade and every restart stops the API, reads included, until the new lead is ready:
`Recreate` stops the old pod first. The window is unmeasured; lead replacement is
[FR-24](../product/roadmap.md)'s to measure. `helm uninstall` leaves the keyring claim
(`oml-keyring` for a release named `oml`), and a reinstall under the same release name and
namespace takes it back; under another name, `--set keyring.existingClaim=<claim>` attaches it.
Losing the claim while the database is kept — deleting it, or losing the volume behind it —
loses the deployment's identity, and the lead then refuses readiness (the fork-check row below).

#### Failure modes

| What the operator sees | Cause | What to do |
|---|---|---|
| The pod waits in `CreateContainerConfigError`, naming the Secret | The Postgres Secret or its key is missing | Create it (Install, above); the pod starts on its own once it exists |
| `CrashLoopBackOff`; the log ends `fatal: postgres connect: …` | The URL in the Secret does not reach Postgres | Fix the Secret's URL or the network path; with the NetworkPolicy on, check `networkPolicy.postgres` |
| `CrashLoopBackOff`; the log ends `fatal: keyring: keyring file permissions are not 0600 (ADR-0023 d4 kms-file shape)` | Something changed the keyring file's mode: the pod's `fsGroupChangePolicy` set back to `Always`, or a volume that applies an fsGroup itself or maps modes (ADR-0033 d4) | Restore `OnRootMismatch`, or move the claim to a volume that keeps file modes; set the file back to 0600 from a pod that mounts the claim as uid 999. Never replace the file: it holds the signing key |
| `/readyz` answers 503 with `fork_check: failed`; the log says `STARTUP FORK CHECK FAILED`; no token is printed | The lead loaded a different signing key than the one that signed the chain — the keyring claim was replaced or emptied while the database was kept | Reattach the claim that holds the deployment's key ([Fork check failing](runbooks.md#fork-check-failing)); never delete the chain head |
| No `enr_` line in the pod log | A tenant exists — the deployment was claimed — or the fork check failed (row above) | A claimed deployment prints no token; issue further keys with the admin key |

## The standalone profile

One host, no orchestrator
([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d2–d5).
`oml standalone up` is a small supervisor that forks the four real binaries —
`oml-server`, `oml-lakewriter`, `oml-reconciler`, `oml-indexer` — as child
processes, restarts them with backoff, and propagates shutdown. The binaries
are not merged (the OS-process custody boundary is the point) and not left to
hand-managed units; `oml standalone systemd` *generates* unit files with
per-role `User=` lines for operators who want init-system supervision —
offered, not default.

Backing choices:

- **Postgres:** bring-your-own host service, pinned major version, pgvector
  required. `oml doctor` verifies the version, the extension, the C6 role
  split and `FORCE ROW LEVEL SECURITY` ([observability](observability.md)).
- **Object store:** `object_store`'s `LocalFileSystem` rooted at one data
  directory — `<data-dir>/quarantine`, `<data-dir>/bodies`,
  `<data-dir>/lake/<tenant>/…`, the same per-tenant path shapes as S3
  prefixes. Delta tables live on local disk; commits use native put-if-absent.
  **Hard constraint, enforced, not advised: the data directory must be a local
  POSIX filesystem.** The atomicity the Delta fence rests on is a
  local-filesystem property, so startup runs the conditional-put scratch-key
  self-test (the same one the lake-writer's readiness runs in Kubernetes) and
  refuses NFS/SMB mounts.
- **KMS:** the `kms-file` software keyring — a file-held KEK (0600, optionally
  passphrase-protected) behind the existing KMS port. What that collapses is
  stated in [security](../architecture/security.md) and bounded in
  [commitments](../product/commitments.md); the mechanism is
  [RS-12](../product/roadmap.md). The keyring file is a copy class in the
  [erasure inventory](../architecture/audit-and-lineage.md) — see the backup
  table below.

The full feature set runs: lake, reconciler, indexer, simulations, erasure,
evidence export. Nothing is excluded; only guarantees weaken, and the
weakenings are the standalone column of the profile-dependent bounds in
[commitments](../product/commitments.md). Durability is one host's disk — the
`local` value-set wording applies verbatim: a correctness environment, not a
durability one. Cluster roles are not exposed here: a standalone node is
always the lead of a cluster of one.

### Standalone quickstart

!!! warning "Design mockup"
    Nothing below exists. No binary is built, no CLI is compiled. This
    transcript is the committed shape of the experience — an exit criterion
    for the deployment work, not a record of it.

Host Postgres with pgvector already running:

```console
$ oml standalone init --config oml-standalone.toml
  migrations applied (owner role) · keyring minted · data dir verified (local fs, conditional-put OK)
  enrollment secret written to ./oml-enrollment.token (single-use)
$ oml standalone up --config oml-standalone.toml &
$ oml bootstrap claim root-operator --url http://localhost:8080 --token-stdin < oml-enrollment.token
root operator key (shown once, journalled as issued-without-value): oml_8Wkd***************************************
```

## The docker profile

One compose file
([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d7). The
services:

- `postgres` — `pgvector/pgvector:pg17` (the pgvector project's image, pinned
  to a major tag; Postgres 18 images changed the data-directory layout, so the
  pin matters for volume mounts), volume-backed, healthcheck `pg_isready`. Two
  roles created at init: the migration/owner role and the service role — C6's
  split is a profile invariant.
- `minio` — the **same pinned tag the Helm chart pins**: the `If-None-Match`
  pin is one fact declared once, consumed by chart and compose. MinIO is the
  default because it rehearses the real store path, keeps the lake-writer's
  conditional-put self-test identical to production, and keeps per-prefix
  credentials expressible. A documented `file:` variant (shared volume +
  `LocalFileSystem`) exists for minimal installs and inherits the standalone
  profile's local-fs constraint and honesty text.
- `oml-bootstrap` — one-shot (`restart: "no"`): migrations under the owner
  role, keyring mint, enrollment secret to a named volume; every other service
  gates on `service_completed_successfully`. The compose twin of the Helm hook
  Job — same ceremony, same show-once claim.
- `oml-server` — the only service that mounts the keyring and the signing key;
  healthcheck on `/readyz` (fork check included, unchanged).
- `oml-server-follower` — optional, under `--profile followers`: no key
  mounts, serving-path unwrap credential only, `role: follower`.
- `oml-lakewriter` — no keyring mount, no signing-key mount, its own Postgres
  credentials. One replica; the conditional-put fence backs it exactly as in
  Kubernetes.
- `oml-reconciler` — `--every` mode, read-only credentials, no key-material
  mounts.
- `oml-indexer` — its own credentials, no key mounts.

Resource hints ship as compose `deploy.resources` **comments, not claims** —
no number in them is measured.

The middle-profile honesty, stated here because this is where the compose file
is documented: custody separation exists *between containers* — the
lake-writer container genuinely cannot read the keyring, because it is not
mounted — but one docker host is one trust domain. Root on the host reads
every volume, every secret, and the keyring. The docker column of the
profile-dependent bounds in [commitments](../product/commitments.md) carries
the consequences.

### Docker quickstart

!!! warning "Design mockup"
    Nothing below exists. No image is built, no compose file is published.
    This transcript is the committed shape of the experience — an exit
    criterion for the deployment work, not a record of it.

```console
$ curl -LO https://e8kor.github.io/open-memory-lab/compose/docker-compose.yaml
$ docker compose up --detach --wait
$ docker compose cp oml-bootstrap:/run/oml/enrollment.token - \
    | oml bootstrap claim root-operator --url http://localhost:8080 --token-stdin
```

Follower scale-out, when read load calls for it:
`docker compose --profile followers up --detach --scale oml-server-follower=3`.

## The lake-writer singleton and its two fences

Exactly one lake-writer drains the outbox (declared in
[storage](../architecture/storage.md)). No profile's supervisor can promise
"exactly one" by itself: a Kubernetes Deployment with `replicas: 1` still
produces two live pods during node partitions and rolling updates; a compose
restart or a second accidental `oml standalone up` can do the same on one
host. The design therefore uses two fences, and only the second one is
load-bearing:

1. **The supervisor-level singleton**: in Kubernetes, a
   `coordination.k8s.io` leader lease — a pod drains the outbox only while it
   holds the lease; in docker, one service at one replica with a restart
   policy; in standalone, the supervisor forks exactly one child. This keeps
   the common case quiet — one writer, no conflicts — but it is an
   optimization, because a lease can be held by a pod the API server believes
   dead, and a host can run two supervisors.
2. **Delta conditional put + transaction-id conflict**: the fence that is
   allowed to be the last line, on every backend. Every Delta commit is a
   put-if-absent of the next log entry — native on S3 and pinned MinIO, and
   native on local disk too, where `LocalFileSystem` stages and renames
   atomically (which is why the standalone data directory must be a local
   POSIX filesystem). Every batch records appId `oml-lakewriter` with
   version = last applied outbox seq per table. A split-brain second writer
   either loses the conditional put or finds the txn version already
   advanced and skips the batch. Either way the outcome is a retried,
   deterministic batch — never a duplicate application.

The cost of this arrangement: failover is not instant (lease expiry or restart
before a new process drains), so outbox frontier lag is the metric that pages,
not process restarts ([observability](observability.md)). Whether delta-rs
exposes transaction ids exactly as this relies on is verified at
implementation time — the conformance test is [FR-3](../product/roadmap.md),
run per object-store backend (`file://`, pinned MinIO, S3) under
[FR-22](../product/roadmap.md).

## The lead lease

Cluster mode gives `oml-server` two roles
([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d13–d17):
the **lead** owns the write path and is the only process in the deployment
that loads the signing key; **followers** serve reads and forward every write
verb to the lead. The lead singleton is fenced exactly like the lake-writer,
and the subsection above is the template:

1. **The lead lease** (a Postgres row: holder id, address, epoch, expiry,
   renewed on heartbeat): a lead-eligible node takes traffic only while it
   holds the lease, and followers discover the lead's address from the same
   row — no DNS tricks, no gossip. An optimization, exactly like the
   lake-writer's lease.
2. **The per-tenant chain lock** (`pg_advisory_xact_lock` on the chain head,
   C2 in [commitments](../product/commitments.md)): the fence that is allowed
   to be the last line. Every journal append already serializes on it, so two
   split-brain leads cannot fork a chain — they merely contend on the lock.
   C2's chain lock is now load-bearing for two fences: the lake-writer fence
   and the lead fence.

Failover is lease expiry, takeover by a lead-eligible node, the startup fork
check (unchanged), then a journalled `system.lead_acquired` event signed by
the new lead as its first act — the journal always answers "which process was
lead when". During the takeover window, writes are refused and reads keep
flowing wherever followers exist; in-flight writes are atomic by construction
(one Postgres transaction, C1) and retried safely under the existing
idempotency machinery. Lease TTL, heartbeat and the measured takeover window
ship as unmeasured profile defaults — [FR-24](../product/roadmap.md).

Defaults per profile — lead-only everywhere; followers are scale-out, never a
prerequisite:

| Profile | Default | Followers |
|---|---|---|
| standalone | lead-only, always | Not exposed — `role` does not appear in `oml-standalone.toml`; a follower on the same host adds custody theater, not capacity |
| docker | lead-only | `--profile followers`, `deploy.replicas: N`; no key mounts |
| kubernetes | `oml-server-lead` replicas 1 (lease-guarded; rolling updates briefly yield two lead-eligible pods and the lease picks one), signing-key Secret mounted here only | `oml-server-follower` replicas N, default **0**, HPA on the read path |

The Service shape: **one client-facing Service selecting both Deployments**.
Followers forward writes internally, so clients keep a single endpoint and no
client ever routes by role.

Read-log writes split by role: followers append read-log rows directly (a
non-journal table, their own RLS-scoped credentials); anchoring a batch digest
into the journal is the lead's act, on the lead's cadence. The
unanchored-tail window therefore also covers follower-written rows awaiting
the lead's anchor — the exact split is [RS-13](../product/roadmap.md).

## The reconciler: three schedulers, one binary

The reconciler's work is a bounded incremental pass
([reconciliation](../architecture/reconciliation.md)) that must never overlap
itself and must die rather than hang. The binary carries both modes — `--once`
and `--every <interval> --pass-deadline <d>` — and each profile schedules it
natively:

- **kubernetes** — a CronJob running `--once`, unchanged:

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

- **docker and standalone** — the long-lived `--every` mode. Sequential passes
  in one process give the Forbid semantics by construction; the internal
  `--pass-deadline` gives the deadline semantics. There is no cron sidecar and
  no host crontab to keep in sync — the scheduling payload is the binary's own
  flag, declared once in the compose file or `oml-standalone.toml`.

In every scheduler the same two properties hold: the cursor (last reconciled
seq + running chain hash) is single-writer, so a slow pass delays the next one
instead of overlapping it; and a pass killed at its deadline redoes work,
never corrupts it, because the cursor advances only at journalled checkpoints.
The occasional full-genesis verification is a separately named Job in
Kubernetes and a one-off invocation of the same binary in the other two
profiles, with its own, larger deadline. The
deadline values themselves are unmeasured; they ship as profile defaults to be
revised when reconciliation has numbers (the measurement belongs to the same
effort as [FR-7](../product/roadmap.md)).

## Probes

`oml-server` exposes `/healthz` and `/readyz`
([HTTP API](../api/http-api.md)); the other binaries expose the same pair on a
local port. The kubelet consumes them in Kubernetes, compose healthchecks in
docker, and the supervisor in standalone — same endpoints, three consumers.

- **Liveness (`/healthz`)**: process is up and its event loop responds.
  Deliberately shallow — a Postgres outage must not make the supervisor (or
  kubelet) restart every process in a loop.
- **Readiness (`/readyz`)**: the process may receive traffic. For `oml-server`
  this is where the **startup fork check** lives: a lead is not ready until
  the stored chain head verifies against the last signed head
  ([storage](../architecture/storage.md)). A failover that lost committed
  journal events therefore surfaces as a deployment that refuses to become
  ready — loud, before any write is accepted — rather than as a silently
  forked chain. The cost is accepted: after such an event the service stays
  down until an operator resolves it; there is no automatic
  "accept-and-fork" path.
- The lake-writer's readiness includes object-store reachability and a
  conditional-put self-test against a scratch key — in Kubernetes and docker
  it catches a MinIO upgrade outside the pin; in standalone it is the check
  that refuses an NFS/SMB data directory.

## Backups, retention windows, and the erasure SLO

Backups are a deployment concern with a correctness consequence: every backup
is a copy class in the [erasure inventory](../architecture/audit-and-lineage.md),
and the windows chosen here become terms in the erasure SLO. What the backups
exist to recover, and what recovery can never get back, is
[disaster recovery](disaster-recovery.md)'s to state.

| Store | Mechanism | Retention window's role |
|---|---|---|
| Postgres (kubernetes) | Base backups + WAL archiving (CloudNativePG-managed locally; the managed service's facility when scaled) | Plaintext-derived rows — pgvector embeddings, tsvector entries — are removed from live tables at erase time but **persist in base backups and WAL until the retention window expires**. The window is therefore an upper bound on how long erased-memory derivatives survive, and it is stated, per tenant policy, in the evidence export. |
| Postgres (docker, standalone) | The host's own facility: `pg_dump` / base backups + WAL archiving against the host or containerized Postgres | Same shape, same window arithmetic — the erasure SLO follows the backup facility, not the profile. A profile with **no** backups has a trivially short window and a trivially absent restore story; both facts belong in the operator's declaration, not in silence. |
| **Keyring file (`kms-file` profiles: docker, standalone, and the [interim chart](#the-interim-chart-the-current-build-on-kubernetes))** | Whatever backs up the keyring path or its volume — host backups, volume snapshots, compose-volume copies, snapshots of the interim chart's keyring claim | **A new copy class.** The keyring holds the KEK itself: any backup of it is a usable key copy, exactly like wrapped-DEK rows in Postgres backups, and it extends the erasure SLO the same way. Retired-KEK destruction is file deletion by the same host admin, journalled; the bound is only as real as the operator's file hygiene, and the inventory says so ([audit and lineage](../architecture/audit-and-lineage.md), [RS-12](../product/roadmap.md)). |
| Delta tables | Table history until `VACUUM` + checkpoint cleanup, run inside the lake-writer — identical in all three profiles | Old parquet versions and checkpoints hold **ciphertext** of erased bodies until vacuumed. The DEK is already destroyed, so the bytes are unreadable, but the inventory still names them, and the vacuum cadence is the stated bound on their lifetime. |
| Object store | Versioning (if enabled) on quarantine and bodies prefixes; in standalone, any host-level backup of the data directory plays the same role | Same shape: ciphertext versions survive until lifecycle rules (or host backup expiry) remove them; the window is the stated bound. Quarantine objects of refused submissions get a short, named expiry of their own. |
| Read-event log | Partition drops on the retention window set by tenant policy ([retrieval](../architecture/retrieval.md)) | Not a backup, but the same discipline: the window is declared, and exports scope to it. |
| Postgres `artifact_keys` wrapped DEKs | Base backups + WAL (same facility as the Postgres rows above) | **Usable key copies survive the retention window**: a restored backup plus a live unwrap could recover a destroyed DEK. Bounded by scheduled KEK rotation — rotation re-wraps live DEKs only, and a retired KEK is destroyed once no retained backup predates its retirement — so the erasure SLO term for this class is backup retention + KEK rotation period ([audit and lineage](../architecture/audit-and-lineage.md)). In the `kms-file` profiles "destroyed" means the journalled deletion of a key file, by the host admin — the same honesty as the keyring row. |

The composition rule: **the erasure SLO for a copy class is the longest window
that can still contain it.** For plaintext-derived data that is the Postgres
backup retention window; for wrapped-DEK copies it is backup retention plus
the KEK rotation period; for the keyring file it is the keyring backup's own
lifetime, which only the operator controls; for ciphertext copies it is vacuum
cadence and lifecycle rules — and for those, key destruction already provides
the security property, so the windows bound tidiness, not exposure. The
profiles ship defaults for every window; none of the defaults is a measured or
regulator-sourced number, and the sanctioned claim stays what
[audit and lineage](../architecture/audit-and-lineage.md) states: a journalled
erase event whose effect on each copy class is listed.

Restores are the dual hazard: restoring a Postgres backup resurrects erased
plaintext-derived rows and rewinds the chain head. A restore is therefore an
operator ceremony, not a routine act, and the ceremony — per-profile store
restore, keyring ordering, the fork check, erase replay, a full-genesis
reconciliation pass before traffic — is specified in [restore](restore.md).
What each profile can lose and what recovery costs, stated structurally and
never as a number, is [disaster recovery](disaster-recovery.md)'s. The
profile migration path
([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) d10,
rehearsed as [FR-23](../product/roadmap.md)) inherits the restore ceremony
wholesale.

## What this page does not decide

Resource requests and limits (in compose files they appear only as comments),
node counts, ingress and TLS termination, and the Prometheus stack that
scrapes the metrics are deployment-site choices the payloads parameterize but
this design does not fix. The configuration surface those payloads feed is
specified in [configuration](configuration.md), and what can honestly be said
about sizing before anything is measured is [capacity](capacity.md)'s. Postgres sizing beyond "the journal domain is
synchronous" is open until [FR-7](../product/roadmap.md) has a measured
ceiling; lease TTLs and takeover windows are open until
[FR-24](../product/roadmap.md) has numbers; the keyring mechanism is
[RS-12](../product/roadmap.md).
