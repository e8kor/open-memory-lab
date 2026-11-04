<!--
id: guides/kubernetes
tier: engineering
status: draft for review · describes the v0.4.0 chart; installed by CI on kind, on no other cluster
answers: how to install OML v0.4.0 on Kubernetes with the release's chart: prerequisites, the Postgres role pair and its Secret, verifying the chart, a minimal values file, helm install, claiming the first admin key before any Ingress, oauth.issuer for claude.ai and mobile, probes, upgrades, uninstall and what survives it, and what the interim chart does not give
-->

# Guide: Kubernetes

**Status:** draft for review · describes the v0.4.0 chart; CI installs it on kind, and no other
cluster is recorded running it

**Release:** the chart since v0.3.0, the owner and serving role split since v0.3.1, connections
for claude.ai and mobile since v0.4.0 ([releases](../product/releases.md))

The chart, `deploy/helm/oml`, runs one `oml-server-lead` against a PostgreSQL you bring, with its
signing keyring on a volume claim that `helm uninstall` keeps. Its decisions are in
[ADR-0033](../adr/ADR-0033-interim-helm-chart.md) (proposed) and its operation on
[deployment](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes); this
page is the order. Every value named here is a key of the chart's `values.yaml`.

## 1. Prerequisites

- **Kubernetes 1.23 or later.** The chart's `kubeVersion` is `>=1.23.0-0`, because the keyring
  depends on `fsGroupChangePolicy`, stable since 1.23.
- **Helm 3.** The release job packages the chart with Helm v3.22.1 and CI installs it with the
  same version.
- **A default StorageClass**, or one named in `keyring.storageClassName`, that provisions a
  `ReadWriteOnce` claim and keeps file modes. The keyring file must stay `0600`.
- **The `oml` CLI of the same release** on your machine, for the claim
  ([install](../start/install.md)), and `cosign` to verify the chart.
- **PostgreSQL, external.** The chart bundles no database: the designed `local` value set with
  CloudNativePG is not built. CI runs `pgvector/pgvector:pg17` in the cluster; a managed service
  works if it lets you create the two roles below, which no recorded run has tried.
- **An ingress controller and a TLS certificate**, only if the API leaves the cluster. The chart
  renders an Ingress and passes `ingress.tls` and `ingress.annotations` through; it issues no
  certificate.

**The role pair.** The lead needs two Postgres roles, each with `LOGIN` and no other attribute: an
owner, which owns the database and runs the migrations, and a serving role, which owns nothing
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d2, d4). The lead
refuses to start, with `postgres.role_split`, when both URLs log in as one role. As a superuser,
the same statements CI runs:

```sql
CREATE ROLE oml_owner LOGIN PASSWORD '...';
CREATE ROLE oml_server LOGIN PASSWORD '...';
ALTER DATABASE oml OWNER TO oml_owner;
```

The migrations grant the serving role its privileges; you grant it nothing by hand.

## 2. Get the chart and verify it

Each release attaches `oml-X.Y.Z.tgz` and its signature bundle `oml-X.Y.Z.tgz.sigstore.json`,
signed keyless by the release workflow, the same identity as the CLI archives
([install, by hand](../start/install.md#by-hand-with-cosign)):

```sh
v=0.4.0
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-$v.tgz" -O "$base/oml-$v.tgz.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-$v.tgz.sigstore.json" "oml-$v.tgz"
```

Install only a chart that verifies. The release's `SHA256SUMS` does not list the chart: the chart
job signs and uploads it separately, so the cosign bundle is its one check. The release notes
print the image's index digest; pin it with `image.digest` so the chart runs the image the
release signed, which `cosign verify` on `ghcr.io/e8kor/open-memory-lab@sha256:…` checks with
the same two flags.

## 3. The Secret and a minimal values file

The chart reads both URLs from one Secret you create, and never takes a URL or a password as a
value:

```sh
kubectl create namespace oml
kubectl --namespace oml create secret generic oml-postgres \
  --from-literal=OML_POSTGRES__URL='postgres://oml_server:PASSWORD@HOST:5432/oml' \
  --from-literal=OML_POSTGRES__OWNER_URL='postgres://oml_owner:PASSWORD@HOST:5432/oml'
```

`postgres.secretName`, `postgres.secretKey` and `postgres.ownerSecretKey` rename the Secret and
its keys; the defaults are the names above. A minimal `values.yaml`, with the Ingress off until
the claim:

```yaml
image:
  digest: "sha256:..."        # from the release notes
networkPolicy:
  enabled: true               # API port in; DNS and Postgres out
  postgres:
    port: 5432
    to: []                    # e.g. an ipBlock for a managed Postgres
```

`resources` ships empty: no number for it is measured ([capacity](../operations/capacity.md)).
The replica count, the strategy, the security context and the probes are not values.

## 4. Install

```sh
helm install oml ./oml-0.4.0.tgz --namespace oml --values values.yaml
kubectl --namespace oml rollout status deployment/oml-server-lead
```

A release named `oml` gives `deployment/oml-server-lead`, `service/oml-server` and the claim
`oml-keyring`; a release name without `oml` in it prefixes them with `<release>-oml`. The lead
applies the migrations under the owner URL before its listener binds, then serves under the
serving URL. Without the Secret the pod waits in `CreateContainerConfigError`; the other failures
and their remedies are on [deployment](../operations/deployment.md#failure-modes).

## 5. Claim the first admin key before any Ingress

While no tenant exists, the lead prints a single-use enrollment token in its log at every boot,
and whoever presents it first receives the first admin key and the tenant
([the bootstrap race](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race)).
The token also sits in whatever store your cluster ships container logs to until it is used. So
claim through a port-forward, while the Service is cluster-internal:

```sh
TOKEN="$(kubectl --namespace oml logs deployment/oml-server-lead | grep -o 'enr_[0-9a-f]\{48\}' | tail -1)"
kubectl --namespace oml port-forward service/oml-server 8080:8080 &
printf '%s\n' "$TOKEN" | oml --url http://127.0.0.1:8080 bootstrap claim <key-name> \
  --token-stdin --tenant-name <tenant> --principal <you>
```

The key is printed once and stored in `~/.config/oml/credentials`. A restart before the claim
prints a new token and the old one stops working, so read it from the running pod, never with
`--previous`. The install notes warn when an install enables the Ingress; the chart cannot see
whether the claim happened, so it does not refuse one.

## 6. Expose the API

Only after the claim, add the Ingress to `values.yaml` and upgrade with it:

```yaml
ingress:
  enabled: true
  className: nginx            # your controller's class
  hosts:
    - host: oml.example.com
      paths:
        - path: /
          pathType: Prefix
  tls:
    - secretName: oml-tls
      hosts: [oml.example.com]
```

```sh
helm upgrade oml ./oml-0.4.0.tgz --namespace oml --values values.yaml
```

Give `ingress.hosts` whole, paths included: `--set 'ingress.hosts[0].host=…'` alone replaces the
list and renders a rule with no paths. With the NetworkPolicy on, add the ingress controller's namespace to
`networkPolicy.ingressFrom` first: the default admits pods in the release's namespace alone. The
default path `/` also routes `/readyz`, whose body names the build; narrowing
`ingress.hosts[].paths` keeps it off the public route, and also keeps `oml doctor` from reading
the issuer there (section 7).

## 7. claude.ai and mobile: `oauth.issuer`

Connections from Claude's hosted apps need `oauth.issuer` set to the public origin, `https` with
no path and no trailing slash, and the proxy passing `/mcp`, `/.well-known/oauth-*` and
`/oauth/*` through unchanged
([connections behind a reverse proxy](../operations/deployment.md#connections-behind-a-reverse-proxy)).
Unset, connections are off and nothing else changes.

**The v0.4.0 chart has no value for it.** The Deployment's environment holds the two Postgres URLs
alone, and no configuration file is mounted. Until the chart carries it, set it on the Deployment
outside the chart, which restarts the lead:

```sh
kubectl --namespace oml set env deployment/oml-server-lead OML_OAUTH__ISSUER=https://oml.example.com
```

Whether a later `helm upgrade` keeps a variable set this way is not verified; check with
`kubectl --namespace oml set env deployment/oml-server-lead --list` after each one.

**The access log.** The ingress controller's access log must not record the `Location` header of
`POST /oauth/authorize`, nor the bodies of `POST /oauth/authorize` and `POST /oauth/token`: they
carry the codes and tokens. The chart sets no logging; configure it in your controller.
`oml doctor` then checks the route and reports `oauth` as passing, failing with the URL that
failed, or `off` ([claude.ai and mobile](claude-ai-and-mobile.md)).

## 8. Probes

Not values ([probes](../operations/deployment.md#probes)): a startup probe on `/healthz` every 2
seconds, up to 60 failures, which covers the migrations at boot; liveness on `/healthz` every 10
seconds; readiness on `/readyz` every 5 seconds, answering 503 until Postgres is reachable and the
startup fork check has passed. The timings are chosen, not measured. A `/readyz` 503 with
`fork_check: failed` means the lead loaded a different signing key than the one that signed the
chain ([fork check failing](../operations/runbooks.md#fork-check-failing)).

## 9. Upgrade

1. **Back up** the database and a snapshot of the keyring claim together
   ([the upgrade order](../operations/upgrades.md#the-upgrade-order)).
2. **Verify the new chart** as in section 2.
3. **Upgrade**, replacing the image digest: `--reuse-values` would otherwise keep the old
   release's `image.digest` and run the old image.

    ```sh
    helm upgrade oml ./oml-X.Y.Z.tgz --namespace oml --reuse-values --set image.digest=sha256:...
    ```

The lead runs the new migrations at boot under the owner URL; there is no migration step of your
own. `Recreate` stops the old pod first, so every upgrade and restart stops the API, reads
included, until the new lead is ready, for a window that is unmeasured. Rollback is binaries only,
never schema. From v0.3.x, the server, the CLI and the plugin move together
([the upgrade to v0.4.0](../operations/upgrades.md#server-cli-and-plugin-together-the-upgrade-to-v040));
an install still on one Postgres role splits it first
([the upgrade to v0.3.1](../operations/upgrades.md#splitting-a-single-role-install-the-upgrade-to-v031)).

## 10. Uninstall, and what survives

`helm uninstall oml --namespace oml` removes the Deployment, the Service, the ServiceAccount, the
Ingress and the NetworkPolicy. It leaves:

- **the keyring claim**, `oml-keyring`, annotated `helm.sh/resource-policy: keep`. A reinstall
  under the same release name and namespace takes it back; under another name,
  `--set keyring.existingClaim=oml-keyring` attaches it;
- **the database**, which the chart never touches, and the `oml-postgres` Secret, which you
  created;
- **the pod logs** your cluster shipped elsewhere.

Deleting the claim while keeping the database loses the deployment's identity: the lead mints a
new key and refuses readiness at the fork check. Any copy of the claim is a usable key copy
([backups](../operations/deployment.md#backups-retention-windows-and-the-erasure-slo)).

## What the interim chart does not give

One lead under `Recreate`, with no lead lease, no read followers and no rolling update; no
lake-writer, reconciler, indexer or bootstrap Job, which are decided, not built; no bundled
Postgres or object store, and no `local` or `scaled` value set; the signing key on a volume rather
than in a Secret or a KMS; the enrollment token in the pod log rather than in a Secret; no
`oauth.issuer` value and no resource numbers. The comparison with the designed chart is on
[deployment](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes).

## Next

- Tenants, workareas, agent and reviewer keys, connect codes: [administering](administering.md).
- Connecting Claude Code with an agent key: [Claude Code](claude-code.md).
- Reviewing what agents write: [reviewing memories](reviewing.md).
- When something fails: [troubleshooting](troubleshooting.md).
