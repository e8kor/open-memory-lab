---
status: Stable
verified: 2026-11-06
answers: "the steps to take for each operational alert: a failed fork check, no available reviewer, a stalled review queue, a lagging lake, repeated lead takeovers, repeated 429 answers, a usage counter that drifts from its rows and key compromise"
---

# Operational alerts

Use this page when an alert fires or `oml doctor` shows a failing line. Each section gives the steps, one command per step. Where OML has no command, the step says what you do instead.

Two rules hold throughout. Never edit a store by hand. Capture ids, digests and counts as evidence, never memory text.

## Fork check failing

Use this when `/readyz` fails after a restart and writes return `system.unavailable`. The stored chain head did not verify against the last signed head. The server refuses writes rather than fork the chain. There is no override.

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

## Repeated 429 answers

Use this when clients get `limit.rate` (429) repeatedly. This build has two caps that answer it: unused OAuth client registrations (10,000) and recall feedback labels (1,000 a UTC day). It has no per-key or per-tenant rate limit.

1. Find the route in the request log.

   ```bash
   journalctl -u oml-server -o cat | jq -c 'select(.status==429) | {route, key_name, client_ip}'
   ```

2. If the route is `/oauth/register`, the cap is on unused registrations; they lapse after 24 hours.
3. If one key floods the server, deny it.

   ```bash
   oml deny set <key> universe
   ```

## A form answers idempotency.in_progress

Use this when a person's resubmitted web console form answers `409 idempotency.in_progress`, or `oml doctor` warns `idempotency_unsettled`. The act claimed its `Idempotency-Key` and no outcome was recorded: the act may have committed, so the server never lets that key act again. Nothing needs repairing. A fresh form acts under a fresh key.

1. Find each claim the lead left, with its act, key and claim id.

   ```bash
   journalctl -u oml-server -o cat | jq -c 'select(.event=="idempotency_unsettled") | {act, key_id, claim_id, reason}'
   ```

2. List the claims still held and how old each is, as the database owner (the table's row security binds the tenant, so set it per tenant).

   ```sql
   SET oml.tenant_id = '<tenant id>';
   SELECT key, response->>'claim' AS claim_id, response->>'claimed_at' AS claimed_at
   FROM idempotency WHERE committed_at IS NULL ORDER BY response->>'claimed_at';
   ```

   The `key` column is `<key id>:<nil workarea>:<act>:<Idempotency-Key>`. A claim older than a minute belongs to an attempt that stopped.

3. To let that form act again, first check the journal for the act's event under the key id, at the claim's time. Delete the row only if the act did not commit, because a deleted claim lets the resubmission act again.

   ```sql
   DELETE FROM idempotency WHERE committed_at IS NULL AND key = '<key>' AND response->>'claim' = '<claim id>';
   ```

No sweeper removes held claims yet. Each holds one form's key, so leaving them costs nothing but the row.

## A usage counter drifts from its rows

Use this when `oml doctor` under an admin key prints `warn usage: <n> accounts drift from their rows (…)`. An account's counter in `account_meters` or `account_activity` differs from what its stored rows count ([ADR-0071](../adr/ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md) d3). Only a bug or a hand edit causes it. No code repairs a counter, because a silent repair would hide the bug. While it stands, an account whose counter reads high is refused `limit.quota` before its real limit, one whose counter reads low may pass it by the difference, and the ledger records the counter as it reads.

1. Read the report and keep it: each account, meter, counter and row count. They are ids and counts.

   ```bash
   curl -fsS -H "Authorization: Bearer $OML_API_KEY" "$OML_URL/v2/admin/usage/drift"
   ```

2. Read it again a minute later. A difference that grows names a fold that still miscounts. A difference that stays is one past event or a hand edit.
3. Report it as an issue with both reads and the server's version, before you change anything. The cause is the finding.
4. If an account is refused before its real limit, give it room with an override, which is journalled. Take it away with `DELETE` on the same route once the counter is right.

   ```bash
   curl -fsS -X PUT -H "Authorization: Bearer $OML_API_KEY" -H 'Content-Type: application/json' \
     -d '{"profile": "<its profile>", "overrides": {"<quota>": <limit>}}' \
     "$OML_URL/v2/admin/accounts/<account>/profile"
   ```

5. Set the drifted stocks to what the rows count, as the database owner, once the cause is recorded. The lock makes every commit that moves a meter wait for this transaction, so none is lost.

   ```sql
   BEGIN;
   SET LOCAL oml.tenant_id = '<tenant id>';
   LOCK TABLE account_meters IN SHARE ROW EXCLUSIVE MODE;
   UPDATE account_meters m
      SET memories = s.memories, text_bytes = s.text_bytes, keys = s.keys, connections = s.connections
     FROM oml_account_source('<tenant id>') s
    WHERE m.tenant_id = '<tenant id>' AND m.user_root = s.user_root AND m.user_root = '<account>';
   COMMIT;
   ```

6. Read the report once more. It lists nothing for that account's stocks.

A drift on `writes` is the month's alone and ends with the month. `recalls` has no rows to compare with: a count the queue dropped or the store refused is logged as `usage_recall_counts_lost` and stays lost.

## Rotate a signing key or a keyring

Planned and emergency rotation are deployment steps. OML has no command for either. The server journals `system.signing_key_rotated` when a new signing key starts. See the [event catalog](../data/event-catalog.md).

1. For a compromise, stop the lead first. Writes refuse everywhere.
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
