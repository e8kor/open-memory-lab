# Kubernetes deployment — the chart lives in deploy/helm/oml

There are no manifests in this directory, on purpose.

The kubernetes profile — the Helm umbrella chart with its `local` and `scaled`
value sets — is specified in [docs/operations/deployment.md](../../docs/operations/deployment.md)
and governed by [ADR-0016](../../docs/adr/ADR-0016-kubernetes-workloads.md).
Per the [critical path](../../docs/product/critical-path.md), the `local` value
set lands at **M3** and `scaled` at **M6**.

What runs on Kubernetes today is the first revision of that chart,
[deploy/helm/oml](../helm/oml/README.md): the current build's one lead,
decided in the proposed [ADR-0033](../../docs/adr/ADR-0033-interim-helm-chart.md)
and documented in [docs/operations/deployment.md, "The interim chart"](../../docs/operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes).

The other two profiles:

- **standalone** — one host, the supervised binaries (see
  [docs/operations/deployment.md](../../docs/operations/deployment.md)),
- **docker** — the reduced compose file at the repository root
  (`docker-compose.yml`), postgres + oml-server for the M1 slice.
