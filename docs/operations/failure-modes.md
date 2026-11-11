---
status: Stable
verified: 2026-11-06
answers: "what the server does, what you see and what to do when each dependency or role fails"
---

# Failure modes

When the server cannot establish that an act is permitted and durable, the act does not happen. It refuses with a typed code from the [error list](../api/errors.md). Each row below is one failure on its own. For steps, see [operational alerts](runbooks.md). For the signals, see [Monitor an OML server](observability.md).

| Failure | What the server does | What you see | What to do |
|---|---|---|---|
| Postgres primary unreachable | Refuses every write and read, because the serving store is the store that is down. | `system.unavailable` (503, retryable). `/readyz` fails. | Restore Postgres. Clients retry on their own. |
| Stored chain head does not verify at start (fork check) | Refuses writes and stays down. It never signs on top of a fork. | `/readyz` fails. Writes return `system.unavailable`. | Follow [fork check failing](runbooks.md#fork-check-failing). |
| Lead takes over | Refuses writes until a new lead holds the lease and passes the fork check. | A short burst of `system.unavailable` on writes. | Retry. If it repeats, see [lead takeovers repeat](runbooks.md#lead-takeovers-repeat). |
| Keyring file (`kms.keyring_path`) unreadable | Refuses writes, because nothing can be encrypted. Reads of cached keys keep working until the cache expires. | `system.unavailable`. | Restore the keyring file. |
| Scan rule set fails to load | The server reports not ready and serves nothing. It never admits an unscanned write. | `/readyz` fails. `oml doctor` shows `scan_ruleset` or `scan_rules`. | Fix the rule set the server names, then restart. |
| Scan fails on one write | Refuses that write. | `system.internal`. | Retry. If it repeats, send the `request_id` to the operator. |
| A held version has a stored scan finding but no findings list | Serves it as before; the review card shows the stored finding alone. A server older than migration 0043, during an upgrade or after a rollback, wrote it ([ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d23). | `oml doctor` warns `scan_findings_unlisted: n`. | Run `oml admin rescan --record-held`, which lists the findings of each held version. |
| Search index behind | Recall misses memories approved after the backlog. Get by id is unaffected. | Fewer hits. `oml_sparse_backlog` rises when the sparse encoder is on. | Wait for the backlog to drain. Check `oml doctor` search lines. |
| Reranker slow or failing | Skips reranking and returns the unreranked order. | `oml_reranks_total{outcome="skipped"}` rises. `oml doctor` warns. | See [troubleshooting the server](../help/troubleshooting-server.md#search-model-files-are-missing). |
| Lake writer stopped or behind | Nothing client-facing changes. The lake serves no client read. | `oml_lake_lag_seconds` grows. | Follow [the lake falls behind](runbooks.md#the-lake-falls-behind). |
| No reviewer available | Accepts writes and holds them. It does not refuse them. | Memories stay `held`. `oml doctor` reports review capacity. | Follow [no reviewer can approve](runbooks.md#no-reviewer-can-approve). |
| `oml-web`'s secret weak, missing, set twice, or in a file others can read | `oml-web` (or the server, for its own setting) refuses to start, naming the setting and, for a file, its path and mode, never the value. | The process exits. `oml-web --doctor` prints a `fail` line naming the setting. | Set one secret of at least 32 visible ASCII characters, 8 of them distinct, in an owner-only file or the environment ([Run the web console](web-console.md#store-the-client-secret)). |
| The server refuses `oml-web`'s secret: different values, a rotation done out of order, or a previous secret past `web.client_secret_previous_until` | Refuses every capacity credential `oml-web` presents as `auth.invalid` `session_outside_web`. | Every page sends people back to sign-in. `oml-web --doctor` prints `fail api: ... refuses oml-web's web.client_secret`. A server start with a closed window logs `web_client_previous_closed`. | Set the same value on both, or keep the old one on the server as `web.client_secret_previous` while `oml-web` restarts ([Rotating the client secret](web-console.md#rotating-the-client-secret)). |
| Metrics listener not set | Serves no `/metrics`. | `oml doctor` says where the counts are. | Set `server.metrics_listen`. |

We have not measured how long each refusal window lasts.
