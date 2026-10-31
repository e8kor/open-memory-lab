<!--
id: adr/ADR-0033-interim-helm-chart
tier: engineering
status: proposed
answers: what the Helm chart holds while the build has one server binary; why one replica under Recreate stands in for the lead lease; why the keyring sits on a kept ReadWriteOnce claim and what that sets aside of ADR-0016 d4; the pod's user, fsGroup policy and hardening; Postgres through an existing Secret; the probes; the bootstrap race and the claim through port-forward before any Ingress; the chart's version; how CI and the release carry the chart
-->

# ADR-0033 — The Helm chart starts as the current build's one lead: Recreate, a kept keyring claim, and the claim before any Ingress

**Status:** proposed · 2026-10-31

**Amends:** ADR-0016 (d4), ADR-0023 (d16) — for the interim chart only; the designed chart returns to both

## Context

The designed Kubernetes profile is one umbrella chart with a `local` and a `scaled` value set
([ADR-0016](ADR-0016-kubernetes-workloads.md) d2) running the whole workload set (d1): a
lease-guarded `oml-server-lead`, `oml-server-follower` at zero replicas, the lake-writer, the
reconciler CronJob, the indexer and a bootstrap hook Job, with the signing key in a Secret or KMS
(ADR-0016 d4; [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d16). The
[critical path](../product/critical-path.md) lands the `local` value set at M3 and `scaled` at M6.

The build is the M1 slice and part of M2. It has one server binary, `oml-server-lead`, and four
facts about it (`crates/bins/oml-server-lead/src/main.rs` and `lib.rs` at v0.2.2) decide what any
chart can hold today:

- Its only KMS adapter is `kms-file` (ADR-0023 d4). On first start it mints the deployment signing
  key into a 0600 file at `kms.keyring_path`, and it refuses to start when that file's mode is
  looser: `fatal: keyring: keyring file permissions are not 0600` (observed on 2026-10-31 against
  the installed lead, keyring set to 0660).
- It applies migrations itself at boot, under the one `postgres.url` it is given (the
  Dockerfile's migrate-on-boot shortcut). There is no bootstrap workload and no owner/service
  role split.
- On every boot of a deployment that has no tenant, it prints a fresh single-use enrollment token
  on stdout and holds it in memory until a claim consumes it. There is no enrollment Secret.
- It holds no lead lease; its boot log calls it "always the lead of a cluster of one".

The image `ghcr.io/e8kor/open-memory-lab:0.2.2` runs it as the user `oml`, uid and gid 999 (read
from the image's `/etc/passwd`, fetched from ghcr.io on 2026-10-31), with
`OML_KMS__KEYRING_PATH=/var/lib/oml/keyring/signing.key` and `OML_SERVER__LISTEN=0.0.0.0:8080`
(the repository's `Dockerfile`).

Running that image on Kubernetes today means writing manifests that answer the questions below
without the design. This record answers them once, for a chart that holds exactly what the build
has. Everything the designed chart holds beyond that stays decided, not built.

## Decision

- **d1 — `deploy/helm/oml` is the first revision of the one chart ADR-0016 d2 names, and it holds
  only the lead.** Same chart name, `oml`, at the path the designed chart will occupy: the
  designed workloads and the `local` and `scaled` value sets join this chart at their milestones,
  never a second chart beside it. Today it renders one Deployment `<fullname>-server-lead`; one
  Service `<fullname>-server` that selects every server pod by `app.kubernetes.io/component:
  server` — the one client-facing Service of ADR-0023 d16, with the role in a label of its own,
  `oml.e8kor.github.io/role: lead`, so a follower Deployment can join without changing an
  immutable selector; the keyring claim (d3); a ServiceAccount (d5); and, when asked, an Ingress
  (d8) and a NetworkPolicy (d5). For a release named `oml` the names are `oml-server-lead` and
  `oml-server`, the ones the [deployment](../operations/deployment.md) page uses. Not in it,
  decided, not built: the follower Deployment, the lake-writer, the reconciler CronJob, the
  indexer, the bootstrap hook Job, MinIO or S3, CloudNativePG, and the two value sets.

- **d2 — One replica under the `Recreate` strategy stands in for the lead lease, and the chain
  lock stays the fence.** The build has no lead lease (ADR-0023 d14, decided, not built). A
  `RollingUpdate` starts the new pod before the old one stops, so every upgrade would run two
  processes that load the signing key and accept writes; `Recreate` stops the old pod first. Like
  the lease, it only keeps the common case quiet: it does not count to one across a node
  partition, and two live leads then contend on the per-tenant chain lock
  (`pg_advisory_xact_lock`, ADR-0002 d3, built in `oml-pg`), which serializes every append, so
  they cannot fork a chain. The cost: every upgrade and every restart stops the API, reads
  included, until the new pod is ready. The length of that window is unmeasured; lead
  replacement is what [FR-24](../product/roadmap.md) measures. `replicas` is not a value.

- **d3 — The keyring lives on a ReadWriteOnce volume claim mounted at `/var/lib/oml/keyring`, and
  `helm uninstall` leaves the claim.** This sets ADR-0016 d4 ("deployment identity replaces volume
  identity") aside for this chart: `kms-file` mints the signing key into a file the lead writes,
  nothing in the build can mint or import it elsewhere, and so the claim holds the deployment's
  identity — the custody event ADR-0016 rejected. The chart carries that cost in the open. The
  claim carries `helm.sh/resource-policy: keep`, so an uninstall does not destroy the key, and
  `keyring.existingClaim` attaches a claim that already holds one. A lost claim with the database
  kept is loud, not a fork: the lead mints a new key, its startup fork check fails ("signature of
  seq 1 verifies under no key in the set"), `/readyz` answers 503 with `fork_check: failed`, and no
  enrollment token is printed (observed on 2026-10-31 against the installed lead). A snapshot or
  backup of the claim is a copy of the keyring — today, the deployment signing key — and the
  keyring row of the deployment page's backup table names it. ReadWriteOnce lets two pods on one
  node mount the claim ([Kubernetes, persistent volumes](https://kubernetes.io/docs/concepts/storage/persistent-volumes/#access-modes),
  fetched 2026-10-31); d2's fence, not the access mode, is what holds then. The designed chart,
  with the key in a Secret or KMS, ends this decision.

- **d4 — The pod runs as the image's user, uid and gid 999, with `fsGroup: 999` under
  `fsGroupChangePolicy: OnRootMismatch`.** The ids are set as numbers because the image names its
  user `oml`, and the kubelet cannot check `runAsNonRoot` against a name. `fsGroup` makes a
  provisioner's root-owned volume writable by the lead. The policy is load-bearing: under the
  default `Always`, the kubelet changes the ownership and permissions of the volume's contents on
  every mount
  ([Kubernetes, security context](https://kubernetes.io/docs/tasks/configure-pod-container/security-context/#configure-volume-permission-and-ownership-change-policy-for-pods),
  fetched 2026-10-31), ORing 0660 into each regular file's mode (`changeFilePermission` and
  `rwMask` in Kubernetes'
  [`pkg/volume/volume_linux.go`](https://github.com/kubernetes/kubernetes/blob/master/pkg/volume/volume_linux.go),
  fetched 2026-10-31). That turns the 0600 keyring into 0660, and the lead stops at its next boot.
  `OnRootMismatch` changes nothing once the volume's root already matches. Not covered: a CSI
  driver that applies fsGroup itself (`VOLUME_MOUNT_GROUP`), where the policy has no effect (the
  same Kubernetes page) — unverified here; on such a volume, the lead's keyring refusal is how a
  changed mode surfaces.

- **d5 — The pod is hardened by template constants, not values.** `runAsNonRoot`;
  `readOnlyRootFilesystem`, because the lead writes only the keyring and its migrations are
  compiled in (`sqlx::migrate!` in `oml-pg`); `allowPrivilegeEscalation: false`; every capability
  dropped; seccomp `RuntimeDefault`; and a ServiceAccount with `automountServiceAccountToken:
  false` on the account and on the pod, because the lead calls no Kubernetes API. Resource
  requests and limits ship as commented hints in `values.yaml`, never as values: no number for
  them is measured ([capacity](../operations/capacity.md)). An optional NetworkPolicy, off by
  default because its peers are site facts, admits the API port from listed peers (by default,
  pods in the release's Kubernetes namespace) and lets the lead open connections to DNS and to
  the Postgres port alone.

- **d6 — Postgres is external and reaches the lead through an existing Secret's key.** The lead's
  `OML_POSTGRES__URL` comes from a `secretKeyRef` (`postgres.secretName`, default `oml-postgres`;
  `postgres.secretKey`, default `OML_POSTGRES__URL`), and the chart never takes the URL or a
  password as a value. A missing Secret is a named failure: the pod stays in
  `CreateContainerConfigError`, naming it. Because the lead migrates on boot, that one URL needs
  the rights to migrate; C6's owner/service split arrives with the bootstrap Job, decided, not
  built.

- **d7 — `/healthz` drives the startup and liveness probes and `/readyz` the readiness probe.**
  ADR-0016 d5 is unchanged: liveness is shallow, and readiness carries Postgres reachability and
  the fork check ([HTTP API](../api/http-api.md#health)). The startup probe exists because the
  lead applies migrations before it binds its listener, so a slow first boot must not count as a
  hung process. The probe timings are chosen, not measured.

- **d8 — The enrollment token is claimed from the pod log through `kubectl port-forward` before
  any Ingress exists: the bootstrap race.** Until the first claim, the token sits in the pod log,
  and in whatever store the cluster ships container logs to; whoever presents it first receives
  the first admin key and the tenant. While the Service is cluster-internal, presenting it takes
  access to the cluster network or the `pods/portforward` right; an Ingress opens it to anyone on
  the Ingress's network who has seen the token. So the Ingress is off by default, and the install
  notes and the [deployment](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes)
  page give the ceremony: read the token from the running pod's log, port-forward the Service,
  run `oml bootstrap claim` on the operator's machine, and only then
  `helm upgrade … --set ingress.enabled=true`. The notes warn when an install enables the
  Ingress; the chart does not refuse one, because it cannot see whether the claim happened. A
  restart before the claim mints a new token and the old one stops working, since the token lives
  only in the process.

- **d9 — The chart's `version` and `appVersion` are the workspace version, and the image defaults
  to `ghcr.io/e8kor/open-memory-lab:<appVersion>`, pinned by digest when `image.digest` is set.**
  One version names the binaries, the image, the agent plugin and the chart, so a tag names one
  release. `scripts/check_chart.py` fails a pull request on a mismatch, and on the loss of any
  property d2–d8 decide in the rendered chart.

- **d10 — CI installs the chart on kind, and the release attaches it, signed.** A new `ci.yml`
  job creates a kind cluster, runs `pgvector/pgvector:pg17` in it, builds the image from the
  checkout and loads it, installs the chart, waits for readiness, claims the token through
  port-forward and runs `scripts/quickstart.sh`; then it restarts the lead, and later uninstalls
  and reinstalls the chart, requiring each time the same public key and no new token. New
  `release.yml` jobs check the chart's version against the tag, package it, sign the `.tgz`
  keyless with `cosign sign-blob` like every other release artifact
  ([CI and toolchain](../engineering/ci-and-toolchain.md#release-artifacts-sbom-signing-provenance)),
  and attach both files to the GitHub release; `scripts/check_release.py` refuses a release
  workflow that stops doing so. No chart repository is published: the `helm repo add` URL of the
  deployment page's quickstart stays a mockup.

## Consequences

- The [deployment](../operations/deployment.md) page gains the interim chart's section — what it
  runs, how it differs from the designed chart, the claim ceremony and its failure modes — and
  its keyring backup row covers this chart's claim; the designed Kubernetes quickstart stays a
  mockup. `deploy/k8s/README.md` points to the chart.
- [CI and toolchain](../engineering/ci-and-toolchain.md) lists the chart among the release
  artifacts and the kind job among the merge gates.
- The critical path is unchanged: the `local` value set still joins at M3 and `scaled` at M6, and
  this chart is neither.
- The costs, in one place: every upgrade is an outage of the one lead, unmeasured (FR-24); the
  deployment's identity is a volume until the designed chart lands; the token is in the pod log
  until claimed; and the one Postgres URL holds migration rights.
- No HTTP operation, error code, event type or claim state changes, and `info.version` stays
  `0.2.0-design`. No roadmap row opens.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **No chart until M3** | Every Kubernetes user then writes manifests that answer d2–d8 alone, and two plausible answers — a second replica, or the default `fsGroupChangePolicy` — fail for causes outside the lead's log: two signing processes during every rollout, or a keyring the kubelet loosened, which the lead refuses at its second boot. |
| **A StatefulSet of one with a volume claim template** | The same volume identity d3 accepts, in a workload kind the designed chart does not have (ADR-0023 d16 names Deployments), so the designed chart would have to move the claim out of it. ADR-0016 rejected it for the designed chart on the identity ground. |
| **The signing key in a Secret, minted by Helm or by the operator** | The build mints its own key and has no import ceremony, so an operator-minted key has no specified shape; a Helm-minted key would sit in every release revision's Secret; and a Secret volume's modes are the kubelet's — with an fsGroup it ORs 0440 into them (`roMask`, applied through `NewVolumeOwnership` in Kubernetes' `pkg/volume/secret/secret.go`, fetched 2026-10-31), which the lead's 0600 check refuses. It is ADR-0016 d4's end state, through the bootstrap Job. |
| **`RollingUpdate` with `maxSurge: 0`** | For one replica it also stops the old pod first, but a later edit of either field silently allows two leads; `Recreate` states the intent in one field, which `scripts/check_chart.py` asserts. |
| **`ReadWriteOncePod`** | It stops a second pod on the same node from mounting the claim, but only CSI volumes support it (the Kubernetes persistent-volumes page, fetched 2026-10-31), and kind's default provisioner is `rancher.io/local-path` (kind's `pkg/build/nodeimage/const_storage.go`, fetched 2026-10-31), so CI could not run the chart; and the fence two leads need is the chain lock either way. |
| **`postgres.url` as a chart value** | The password would live in every Helm release revision and in `helm get values` output. |
| **Refusing an install that enables the Ingress** | The chart cannot see whether the claim happened, so the refusal would block a reinstall over a claimed database and let through an upgrade before the claim. The notes' warning and the documented ceremony carry it. |
| **A chart repository now: an `index.yaml` on Pages, or an OCI push to ghcr.io** | A second publication path, with its own index or registry permissions to keep in step; the release page already carries the signed artifacts, and `helm install` takes the archive's URL. Rejected for this chart; a later record may reopen it once someone asks for `helm repo add`. |
