---
status: Stable
verified: 2026-11-06
answers: "where an operator starts: administer, deploy, monitor, upgrade, back up, respond to alerts"
---

# Operate

Pages for the person who runs an OML server. First time: deploy, then administer, then monitor, then back up.

| Task | Page |
|---|---|
| Claim the server, create workareas, issue keys | [Administer a server](../guides/administering.md) |
| Choose how to run it | [Deploy](deployment.md), [Kubernetes](../guides/kubernetes.md) |
| See that it is healthy | [Monitor](observability.md), [Metrics](../reference/metrics.md) |
| Move to a new release | [Upgrade](upgrades.md) |
| Protect the data | [Back up and restore](restore.md) |
| Respond to a page | [Operational alerts](runbooks.md), [Failure modes](failure-modes.md) |
| Run the web console as its own process | [Run the web console](web-console.md) |
| Run the lake | [Run the lake writer](lake.md) |
| Plan capacity: how many memories and clients one server serves | [Capacity and SLOs](operating.md) |
| Check that a build ranks as the release does, or compare search pipelines | [Measure recall on your build](recall-eval.md) |

Something failing? Start with `oml doctor`, then [troubleshoot a server](../help/troubleshooting-server.md).
