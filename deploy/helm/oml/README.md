# oml — the Helm chart

The current build on Kubernetes: the web console, `oml-web`, as a Deployment of its own beside
one `oml-server-lead` under the `Recreate` strategy, its
keyring on a ReadWriteOnce claim that `helm uninstall` keeps, Postgres external through an
existing Secret, and the first admin key claimed through `kubectl port-forward` before any
Ingress exists. The Secret holds two URLs: `OML_POSTGRES__OWNER_URL`, for the role that owns the
database and runs the migrations, and `OML_POSTGRES__URL`, for the role the lead serves under,
which owns nothing. The lead refuses to start when both log in as one role. The followers, the lake-writer, the reconciler, the indexer and the bootstrap
Job of the designed chart are decided, not built.

- Install, the claim ceremony and the failure modes:
  [Deployment, "The interim chart"](https://e8kor.github.io/open-memory-lab/operations/deployment/#the-interim-chart-the-current-build-on-kubernetes).
- Why the chart holds what it holds, and what it sets aside of the designed chart:
  [ADR-0033](https://e8kor.github.io/open-memory-lab/adr/ADR-0033-interim-helm-chart/).
- The chart's version and `appVersion` are the workspace version; `scripts/check_chart.py`
  holds that and the rendered pod's properties, and `ci/kind-values.yaml` is what CI installs
  on kind.
