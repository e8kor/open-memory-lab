---
status: Stable
verified: 2026-11-06
answers: "how to install OML on Kubernetes with the release's Helm chart: prerequisites, the PostgreSQL role pair and its Secret, verifying the chart, a minimal values file, claiming the first admin key before any Ingress, oauth.issuer, upgrades, uninstall, and what the chart does not deploy"
---

# Install OML on Kubernetes

The chart, `deploy/helm/oml`, runs one `oml-server-lead` against a PostgreSQL you bring. Its signing
keyring sits on a volume claim that `helm uninstall` keeps. Every value named here is a key of the
chart's `values.yaml`.

The chart deploys the server and nothing else. It does not deploy the lake writer, which is optional. Run
that process yourself as [Run the lake writer](../operations/lake.md) describes.

## Before you start

- **Kubernetes 1.23 or later.** The chart's `kubeVersion` is `>=1.23.0-0`, because the keyring depends on
  `fsGroupChangePolicy`, stable since 1.23.
- **Helm 3** and `cosign`, to install and verify the chart.
- **A StorageClass** that provisions a `ReadWriteOnce` claim and keeps file modes. The default class
  works, or name one in `keyring.storageClassName`. The keyring file must stay `0600`.
- **The `oml` CLI of the same release** on your machine, for the claim ([install](../start/install.md)).
- **An external PostgreSQL.** The chart bundles no database. CI runs `pgvector/pgvector:pg17` in the
  cluster. A managed service works if it lets you create the two roles below. No recorded run has tried one.
- **An ingress controller and a TLS certificate**, only if the API leaves the cluster. The chart renders an
  Ingress and passes `ingress.tls` and `ingress.annotations` through. It issues no certificate.

The server needs two PostgreSQL roles, each with `LOGIN` and no other attribute. The owner owns the
database and runs the migrations. The serving role owns nothing. The server refuses to start, with
`postgres.role_split`, when both URLs log in as one role. As a superuser, run the statements CI runs:

```sql
CREATE ROLE oml_owner LOGIN PASSWORD '...';
CREATE ROLE oml_server LOGIN PASSWORD '...';
ALTER DATABASE oml OWNER TO oml_owner;
```

The migrations grant the serving role its privileges. You grant it nothing by hand.

## 1. Get the chart and verify it

Each release attaches `oml-X.Y.Z.tgz` and its signature bundle `oml-X.Y.Z.tgz.sigstore.json`. The release
workflow signs both keyless, with the same identity as the CLI archives
([install, by hand](../start/install.md)).

```sh
v=X.Y.Z
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-$v.tgz" -O "$base/oml-$v.tgz.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-$v.tgz.sigstore.json" "oml-$v.tgz"
```

Install only a chart that verifies. The release's `SHA256SUMS` does not list the chart, so the cosign
bundle is its one check.

The release notes print the image's index digest. Pin it with `image.digest` so the chart runs the image
the release signed.

## 2. Create the Secret and a values file

The chart reads both URLs from one Secret you create. It never takes a URL or a password as a value.

```sh
kubectl create namespace oml
kubectl --namespace oml create secret generic oml-postgres \
  --from-literal=OML_POSTGRES__URL='postgres://oml_server:PASSWORD@HOST:5432/oml' \
  --from-literal=OML_POSTGRES__OWNER_URL='postgres://oml_owner:PASSWORD@HOST:5432/oml'
```

`postgres.secretName`, `postgres.secretKey` and `postgres.ownerSecretKey` rename the Secret and its keys.
A minimal `values.yaml` keeps the Ingress off until the claim:

```yaml
image:
  digest: "sha256:..."        # from the release notes
networkPolicy:
  enabled: true               # API port in; DNS and PostgreSQL out
  postgres:
    port: 5432
    to: []                    # e.g. an ipBlock for a managed PostgreSQL
```

`resources` ships empty, because no sizing number is measured. The replica count, the strategy, the
security context and the probes are not values.

## 3. Install

```sh
helm install oml ./oml-X.Y.Z.tgz --namespace oml --values values.yaml
kubectl --namespace oml rollout status deployment/oml-server-lead
```

A release named `oml` gives `deployment/oml-server-lead`, `service/oml-server` and the claim
`oml-keyring`. Any other release name prefixes them with `<release>-oml`. The server applies the
migrations under the owner URL before its listener binds. Then it serves under the serving URL.

## 4. Claim the first admin key before any Ingress

While no tenant exists, the server prints a single-use enrollment token in its log at every boot.
Whoever presents it first receives the first admin key and the tenant. The token also sits in whatever
store your cluster ships container logs to. So claim through a port-forward, while the Service is
cluster-internal:

```sh
TOKEN="$(kubectl --namespace oml logs deployment/oml-server-lead | grep -o 'enr_[0-9a-f]\{48\}' | tail -1)"
kubectl --namespace oml port-forward service/oml-server 8080:8080 &
printf '%s\n' "$TOKEN" | oml --url http://127.0.0.1:8080 bootstrap claim <key-name> \
  --token-stdin --tenant-name <tenant> --principal <you>
```

The CLI prints the key once and stores it in `~/.config/oml/credentials`. A restart before the claim
prints a new token and voids the old one. Read the token from the running pod, never with `--previous`.

The chart cannot see whether the claim happened, so it does not refuse an Ingress before it. The install
notes warn when an install enables one.

## 5. Expose the API

After the claim, add the Ingress to `values.yaml` and upgrade with it:

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
helm upgrade oml ./oml-X.Y.Z.tgz --namespace oml --values values.yaml
```

Give `ingress.hosts` whole, paths included. `--set 'ingress.hosts[0].host=...'` alone replaces the list
and renders a rule with no paths.

With the NetworkPolicy on, add the ingress controller's namespace to `networkPolicy.ingressFrom` first.
The default admits pods in the release's namespace alone.

The default path `/` also routes `/readyz`, whose body names the build. Narrow `ingress.hosts[].paths` to
keep it off the public route. That also stops `oml doctor` from reading the issuer there.

## 6. Connect claude.ai and mobile

Connections from Claude's hosted apps need `oauth.issuer` set to the public origin. The ingress must pass
`/mcp`, `/.well-known/oauth-*` and `/oauth/*` through unchanged
([connect claude.ai and the mobile apps](../operations/deployment.md#connect-claudeai-and-the-mobile-apps)).
Unset, connections are off and nothing else changes.

The chart has no value for `oauth.issuer`. The Deployment's environment holds the two PostgreSQL URLs
alone, and no configuration file is mounted. Set it on the Deployment outside the chart, which restarts the server:

```sh
kubectl --namespace oml set env deployment/oml-server-lead OML_OAUTH__ISSUER=https://oml.example.com
```

Whether a later `helm upgrade` keeps a variable set this way is not verified. Check with
`kubectl --namespace oml set env deployment/oml-server-lead --list` after each upgrade.

Configure your ingress controller's access log to omit the `Location` header of `POST /oauth/authorize`
and the bodies of `POST /oauth/authorize` and `POST /oauth/token`. The chart sets no logging.

## 7. Upgrade

1. Back up the database and a snapshot of the keyring claim together ([Upgrade OML](../operations/upgrades.md)).
2. Verify the new chart as in step 1.
3. Upgrade, replacing the image digest. `--reuse-values` would otherwise keep the old `image.digest` and run the old image.

    ```sh
    helm upgrade oml ./oml-X.Y.Z.tgz --namespace oml --reuse-values --set image.digest=sha256:...
    ```

The server runs the new migrations at boot under the owner URL. You run no migration step.

`Recreate` stops the old pod first. Every upgrade and restart stops the API, reads included, until the
new pod is ready. The window is not measured. Rollback is binaries only, never schema.

## 8. Uninstall, and what survives

`helm uninstall oml --namespace oml` removes the Deployment, the Service, the ServiceAccount, the Ingress
and the NetworkPolicy. It leaves three things:

- **The keyring claim**, `oml-keyring`, annotated `helm.sh/resource-policy: keep`. A reinstall under the
  same release name and namespace takes it back. Under another name, `--set keyring.existingClaim=oml-keyring` attaches it.
- **The database**, which the chart never touches, and the `oml-postgres` Secret you created.
- **The pod logs** your cluster shipped elsewhere.

Deleting the claim while keeping the database loses the deployment's identity. The server mints a new
key and refuses readiness at the fork check. Any copy of the claim is a usable key copy, so guard
snapshots of it ([back up](../operations/deployment.md#back-up-the-database-and-the-keyring-together)).

## What the chart does not give

- One server under `Recreate`: no read followers, no rolling update, and every upgrade is an outage.
- No lake writer, and no bundled PostgreSQL.
- The signing key on a volume, not in a Secret or a key-management service.
- The enrollment token in the pod log, not in a Secret.
- No `oauth.issuer` value and no resource numbers.

## Troubleshooting

The startup probe checks `/healthz` every 2 seconds for up to 60 failures, which covers the migrations at
boot. Liveness checks `/healthz` every 10 seconds. Readiness checks `/readyz` every 5 seconds and answers
503 until PostgreSQL is reachable and the startup fork check has passed. The timings are chosen, not measured.

| You see | Cause | Do this |
|---|---|---|
| The pod waits in `CreateContainerConfigError`, naming the Secret | The Secret or its key is missing | Create it as in step 2. The pod starts on its own. |
| `CrashLoopBackOff`; the log ends `fatal: postgres connect: ...` | The URL in the Secret does not reach PostgreSQL | Fix the URL or the network path. With the NetworkPolicy on, check `networkPolicy.postgres`. |
| `CrashLoopBackOff`; the log ends `fatal: keyring: keyring file permissions are not 0600 (kms-file shape)` | Something changed the keyring file's mode, such as `fsGroupChangePolicy` set back to `Always` or a volume that maps modes | Restore `OnRootMismatch`, or move the claim to a volume that keeps file modes. Set the file back to 0600 from a pod that mounts the claim as uid 999. Never replace the file: it holds the signing key. |
| `/readyz` answers 503 with `fork_check: failed`; no token is printed | The server loaded a different signing key than the one that signed the chain | Reattach the claim that holds the deployment's key ([runbooks](../operations/runbooks.md)). Never delete the chain head. |
| No `enr_` line in the pod log | A tenant exists, so the deployment was claimed, or the fork check failed | A claimed deployment prints no token. Issue further keys with the admin key. |

For client-side problems see [troubleshooting](../help/troubleshooting-server.md).

## Next

- Tenants, workareas, agent and reviewer keys: [Administer an OML server](administering.md).
- Connecting Claude Code with an agent key: [Claude Code](claude-code.md).
- Reviewing what agents write: [reviewing memories](reviewing.md).
