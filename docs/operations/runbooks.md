---
status: Stable
verified: 2026-11-06
answers: "the steps to take for each operational alert: a failed fork check, no available reviewer, a stalled review queue, a lagging lake, repeated lead takeovers, rate limits and key compromise"
---

# Operational alerts

Use this page when an alert fires or `oml doctor` shows a failing line. Each section gives the steps, one command per step. Where OML has no command, the step says what you do instead.

Two rules hold throughout. Never edit a store by hand. Capture ids, digests and counts as evidence, never memory text.

## Fork check failing

Use this when `/readyz` fails on every replica after a restart and writes return `system.unavailable`. The stored chain head did not verify against the last signed head. The server refuses writes rather than fork the chain. There is no override.

1. Read the chain state.

   ```bash
   oml doctor
   ```

2. Verify the chain from genesis. The output names the last sequence that verifies.

   ```bash
   oml audit verify-chain --genesis --signing-key <64-hex-public-key> --tenant <tenant>
   ```

3. If someone restored Postgres, continue the [restore](restore.md) steps instead of this page.
4. If a Postgres failover came first, the standby that won was behind the signed head. Acknowledged writes are lost. Treat it as a recovery event: see [disaster recovery](restore.md).
5. If neither happened, suspect corruption or tampering. Keep the output of steps 1 and 2 and follow [security](../architecture/security.md) before any recovery act.

Do not force readiness, re-point at another Postgres or delete the chain-head row.

## No reviewer can approve

Use this when writes to a workarea stay held and nobody can approve them. A memory needs a reviewer key (role `dreamer`) that did not write it.

1. Check the workarea and its crew.

   ```bash
   oml workareas show <workarea>
   ```

2. Check the keys that review there.

   ```bash
   oml keys show <reviewer-key>
   ```

3. If there is no reviewer key with a grant on the workarea, issue one.

   ```bash
   oml keys issue <key-name> --role dreamer --grant <workarea>:dreamer
   ```

4. Confirm the reviewer sees the queue.

   ```bash
   oml review list
   ```

An admin key cannot review. The server refuses it with `access.role_exclusive`.

## Held memories are waiting too long

Use this when `oml_review_wait_seconds` climbs or agents report memories that never appear.

1. List what the reviewer key may judge, oldest first.

   ```bash
   oml review list
   ```

2. If reviewers are available but outpaced, approve or reject in bulk. See [review held memories](../guides/reviewing.md).
3. If the queue is mostly one source, deny that key from the whole tree. A denial wins over every grant.

   ```bash
   oml deny set <key> universe
   ```

4. If no reviewer can see the queue, go to [no reviewer can approve](#no-reviewer-can-approve).

## The lake falls behind

Use this when `oml_lake_lag_seconds` grows without stopping. Nothing client-facing degrades. Analytics age with the lag.

1. Read the lag and the failure counters on the writer's listener.

   ```bash
   curl -s http://127.0.0.1:9466/metrics | grep -E 'oml_lake_(lag|commit_failures|dead_letters)'
   ```

2. If the writer is not running, restart it. See [the lake](lake.md).
3. If `oml_lake_commit_failures_total` grows, read the writer's log for the failing table and fix the object store or its credentials.
4. If `oml_lake_dead_letters_total` grows, the writer skipped rows after its retry budget. Read the log lines for the failing event. Re-enqueueing a dead letter is not available. Fix the cause, then restart the writer.
5. If batches complete but slowly, check `oml_lake_maintenance_runs_total`. Overdue compaction slows commits.

## Lead takeovers repeat

Use this when writes fail with `system.unavailable` outside a deploy. Every takeover refuses writes until the new lead holds the lease and passes the fork check.

1. Count the refused writes in the request log.

   ```bash
   journalctl -u oml-server -o cat | jq -c 'select(.error_code=="system.unavailable")'
   ```

2. Check Postgres health first. A failing lease heartbeat is the usual cause.
3. If a rolling update is running, wait for it to finish. See [upgrades](upgrades.md).
4. If two lead-eligible processes run, stop the extra one. They cannot fork the chain, only contend.
5. While writes fail, do not restart the fleet. Each restart opens another refusal window.

## Rate limits for one tenant

Use this when clients get `limit.rate` (429) repeatedly.

1. Find the route in the request log.

   ```bash
   journalctl -u oml-server -o cat | jq -c 'select(.status==429) | {route, key_name, client_ip}'
   ```

2. If the traffic is legitimate, raise the limit in `oml.toml` and restart. See [configuration](configuration.md).
3. If one key floods the server, deny it.

   ```bash
   oml deny set <key> universe
   ```

## Rotate a signing key or a keyring

Planned and emergency rotation are deployment steps. OML has no command for either. The server journals `system.signing_key_rotated` when a new signing key starts. See the [event catalog](../data/event-catalog.md).

1. For a compromise, stop the lead first. Writes refuse everywhere. Reads continue where followers run.
2. Rebuild the lead's host or pod from a trusted image.
3. Rotate every credential the lead held: the Postgres role, the key-store grant and the object-store credentials.
4. Replace the signing key at its custody point (the Kubernetes Secret or the mounted file) and start the lead.
5. Verify the chain with the new public key.

   ```bash
   oml audit verify-chain --genesis --signing-key <new-64-hex-public-key> --tenant <tenant>
   ```

Rotation does not heal the past. Anything the attacker signed still verifies under the old key. The rotation event and the last head you stored outside the deployment bound the span to review.

## Revoke a key

OML has no `revoke` or `rotate` command. To stop a key, deny it, then issue a new one.

1. Deny the key across the whole tree.

   ```bash
   oml deny set <key> universe
   ```

2. Issue the replacement.

   ```bash
   oml keys issue <key-name> --role <role> --grant <workarea>:<role>
   ```

3. Check the old key's denial.

   ```bash
   oml keys show <old-key>
   ```
