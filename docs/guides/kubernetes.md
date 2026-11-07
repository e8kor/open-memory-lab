---
status: Stable
verified: 2026-11-06
answers: "how to install OML on Kubernetes with the release's Helm chart: prerequisites, the PostgreSQL role pair and its Secret, verifying the chart, a minimal values file, claiming the first admin key before any Ingress, oauth.issuer, upgrades, uninstall, and what the chart does not deploy"
---

# Install OML on Kubernetes

The chart, `deploy/helm/oml`, runs one `oml-server-lead` against a PostgreSQL you bring. The signing keyring sits on a volume claim that `helm uninstall` keeps. The chart does not deploy the optional lake writer; run it as [Run the lake writer](../operations/lake.md) describes.

## Before you start

- **Kubernetes 1.23 or later** (`kubeVersion >=1.23.0-0`, for `fsGroupChangePolicy`), **Helm 3** and `cosign`.
- **A StorageClass** that provisions a `ReadWriteOnce` claim and keeps file modes (the keyring file must stay `0600`). Name one in `keyring.storageClassName` or use the default.
- **The `oml` CLI of the same release** ([install](../start/install.md)).
- **An external PostgreSQL.** CI runs `pgvector/pgvector:pg17`. A managed service needs the two roles below; none has been tried.
- **An ingress controller and TLS certificate**, only if the API leaves the cluster. The chart passes `ingress.tls` and `ingress.annotations` through and issues no certificate.

The server needs two PostgreSQL roles with `LOGIN` and no other attribute: the owner (owns the database, runs migrations) and a serving role that owns nothing. The server refuses to start with `postgres.role_split` if both URLs log in as one role. As a superuser:

```sql
CREATE ROLE oml_owner LOGIN PASSWORD '...';
CREATE ROLE oml_server LOGIN PASSWORD '...';
ALTER DATABASE oml OWNER TO oml_owner;
```

## 1. Get the chart and verify it

Each release attaches `oml-X.Y.Z.tgz` and `oml-X.Y.Z.tgz.sigstore.json`, signed keyless like the CLI archives ([install, by hand](../start/install.md)). `SHA256SUMS` does not list the chart, so the cosign bundle is its one check.

```sh
v=X.Y.Z
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-$v.tgz" -O "$base/oml-$v.tgz.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-$v.tgz.sigstore.json" "oml-$v.tgz"
```

Pin `image.digest` to the index digest in the release notes.

## 2. Create the Secret and a values file

The chart reads both URLs from one Secret you create, never from a value.

```sh
kubectl create namespace oml
kubectl --namespace oml create secret generic oml-postgres \
  --from-literal=OML_POSTGRES__URL='postgres://oml_server:PASSWORD@HOST:5432/oml' \
  --from-literal=OML_POSTGRES__OWNER_URL='postgres://oml_owner:PASSWORD@HOST:5432/oml'
```

`postgres.secretName`, `postgres.secretKey` and `postgres.ownerSecretKey` rename the Secret and keys. A minimal `values.yaml` keeps the Ingress off until the claim:

```yaml
image:
  digest: "sha256:..."        # from the release notes
networkPolicy:
  enabled: true               # API port in; DNS and PostgreSQL out
  postgres:
    port: 5432
    to: []                    # e.g. an ipBlock for a managed PostgreSQL
```

`resources` ships empty. Replica count, strategy, security context and probes are not values.

## 3. Install

```sh
helm install oml ./oml-X.Y.Z.tgz --namespace oml --values values.yaml
kubectl --namespace oml rollout status deployment/oml-server-lead
```

A release named `oml` gives `deployment/oml-server-lead`, `service/oml-server` and the claim `oml-keyring`; any other name prefixes them `<release>-oml`.

## 4. Claim the first admin key before any Ingress

While no tenant exists, the server prints a single-use enrollment token in its log at every boot, and whoever presents it first gets the first admin key. Cluster log shipping also stores it, so claim through a port-forward:

```sh
TOKEN="$(kubectl --namespace oml logs deployment/oml-server-lead | grep -o 'enr_[0-9a-f]\{48\}' | tail -1)"
kubectl --namespace oml port-forward service/oml-server 8080:8080 &
printf '%s\n' "$TOKEN" | oml --url http://127.0.0.1:8080 bootstrap claim <key-name> \
  --token-stdin --tenant-name <tenant> --principal <you>
```

The CLI prints the key once and stores it in `~/.config/oml/credentials`. A restart before the claim voids the old token, so read it from the running pod, never with `--previous`.

## 5. Expose the API

After the claim, add the Ingress and upgrade:

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

- Give `ingress.hosts` whole, paths included; `--set 'ingress.hosts[0].host=...'` alone renders a rule with no paths.
- With the NetworkPolicy on, add the controller's namespace to `networkPolicy.ingressFrom` first. The default admits the release's namespace alone.
- Path `/` also routes `/readyz`, whose body names the build. Narrow `ingress.hosts[].paths` to keep it off the public route; `oml doctor` then cannot read the issuer there.

## 6. Connect claude.ai and mobile

Set `oauth.issuer` to the public origin, and pass `/mcp`, `/.well-known/oauth-*` and `/oauth/*` through unchanged ([connect claude.ai](../operations/deployment.md#connect-claudeai-and-the-mobile-apps)). Unset, connections are off and nothing else changes. The chart has no value for it and mounts no configuration file, so set it on the Deployment, which restarts the server:

```sh
kubectl --namespace oml set env deployment/oml-server-lead OML_OAUTH__ISSUER=https://oml.example.com
```

Whether a later `helm upgrade` keeps it is not verified; check with `kubectl --namespace oml set env deployment/oml-server-lead --list`. Keep the `Location` header of `POST /oauth/authorize` and the bodies of `POST /oauth/authorize` and `POST /oauth/token` out of the controller's access log.

## 7. Upgrade

1. Back up the database and a snapshot of the keyring claim together ([Upgrade OML](../operations/upgrades.md)).
2. Verify the new chart as in step 1.
3. Upgrade with a new digest; `--reuse-values` would otherwise keep the old `image.digest`.

    ```sh
    helm upgrade oml ./oml-X.Y.Z.tgz --namespace oml --reuse-values --set image.digest=sha256:...
    ```

The server runs new migrations at boot. `Recreate` stops the old pod first, so every upgrade and restart stops the API, reads included, until the new pod is ready (window unmeasured). Rollback is binaries only, never schema.

## 8. Uninstall, and what survives

`helm uninstall oml --namespace oml` removes the Deployment, Service, ServiceAccount, Ingress and NetworkPolicy. It keeps:

- **The keyring claim** `oml-keyring` (`helm.sh/resource-policy: keep`). A reinstall under the same release name and namespace takes it back; under another name use `--set keyring.existingClaim=oml-keyring`.
- **The database** and the `oml-postgres` Secret.

Deleting the claim but keeping the database fails the fork check. Any copy of the claim is a usable key copy, so guard snapshots ([back up](../operations/deployment.md#back-up-the-database-and-the-keyring-together)).

Troubleshooting: [pod states and probes](../help/troubleshooting-server.md#on-kubernetes). Next: [Administer an OML server](administering.md), [Claude Code](claude-code.md), [reviewing memories](reviewing.md).

Design record: [ADR-0033](../adr/ADR-0033-interim-helm-chart.md), [ADR-0016](../adr/ADR-0016-kubernetes-workloads.md).
