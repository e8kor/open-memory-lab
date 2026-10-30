# Kubernetes deployment — not here yet

There are no manifests in this directory, on purpose.

The kubernetes profile — the Helm umbrella chart with its `local` and `scaled`
value sets — is specified in [docs/operations/deployment.md](../../docs/operations/deployment.md)
and governed by [ADR-0016](../../docs/adr/ADR-0016-kubernetes-workloads.md).
Per the [critical path](../../docs/product/critical-path.md), the chart and the
Kubernetes local profile land at **M3**; nothing is published before that.

The PoC ships two profiles only:

- **standalone** — one host, the supervised binaries (see
  [docs/operations/deployment.md](../../docs/operations/deployment.md)),
- **docker** — the reduced compose file at the repository root
  (`docker-compose.yml`), postgres + oml-server for the M1 slice.
