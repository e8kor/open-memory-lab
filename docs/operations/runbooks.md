---
status: Stable
verified: 2026-11-14
answers: "the steps to take for each operational alert: a failed fork check, no available reviewer, a stalled review queue, a lagging lake, repeated lead takeovers, repeated 429 answers, a usage counter that drifts from its rows, an upload store that cannot be written or lost parts, and key compromise"
---

# Operational alerts

Use this page when an alert fires or `oml doctor` shows a failing line. Each section gives the steps, one command per step. Where OML has no command, the step says what to do instead.

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

Use this when writes to a workspace stay held and nobody can approve them. A memory needs a reviewer key (role `reviewer`) that did not write it.

1. Check the workspace and its crew.

   ```bash
   oml workspaces show <workspace>
   ```

2. Check the keys that review there.

   ```bash
   oml keys show <reviewer-key>
   ```

3. If there is no reviewer key with a grant on the workspace, issue one.

   ```bash
   oml keys issue <key-name> --role reviewer --grant <workspace>:reviewer
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

## Uploads answer storage_failed or a part goes missing

Use this when `oml doctor` warns `upload_store_unavailable`, or clients get `upload.storage_failed` (503) or an unexpected `upload.incomplete` (409).

1. Read the reason the lead logged at start.

   ```bash
   journalctl -u oml-server -o cat | jq -c 'select(.event=="upload_store_unavailable") | .reason'
   ```

2. Set `storage.url` to an absolute directory the server's user can write, or one it can create below a directory it can write, and restart. The server never changes the mode of a directory it does not own, and refuses one that others can write or that it cannot write itself.
3. When `complete` answers `upload.incomplete` for parts the client already sent, the store lost them, for example when a pod with an emptyDir was replaced: the client sends the named parts again with `PUT /v2/uploads/{id}/parts/{n}` and completes again. `GET /v2/uploads/{id}` lists the same parts as missing.

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

   The `key` column is `<key id>:<nil workspace>:<act>:<Idempotency-Key>`. A claim older than a minute belongs to an attempt that stopped.

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
3. Report it as an issue with both reads and the server's version, before changing anything. The cause is the finding.
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

A drift on `writes` is the month's alone and ends with the month. `recalls` has no rows to compare with: a count the queue dropped, the store refused or a stop left unwritten is logged as `usage_recall_counts_lost`, counted in `oml_usage_recall_counts_lost_total`, and stays lost.

## Rotate a signing key or a keyring

Planned and emergency rotation are deployment steps. OML has no command for either. The server journals `system.signing_key_rotated` when a new signing key starts. See the [event catalog](../data/event-catalog.md).

1. For a compromise, stop the lead first. Writes refuse everywhere.
2. Rebuild the lead's host or pod from a trusted image.
3. Rotate every credential the lead held: the Postgres role, the key-store grant and the object-store credentials.
4. Replace the signing key at its custody point (the Kubernetes Secret or the mounted file) and start the lead.
5. Verify the chain with the new public key, by the [chain format](../data/chain-format.md) procedure run against the journal table. `oml audit verify-chain --genesis --signing-key <new-64-hex-public-key> --tenant <tenant>` checks its arguments and then refuses, because the server serves no chain export yet.

Rotation does not heal the past. Anything the attacker signed still verifies under the old key. The rotation event and the last head stored outside the deployment bound the span to review.

## Revoke a key

Use this when a key leaked, or its holder no longer needs it. `oml keys revoke` ends one key on every path: its value, its OAuth tokens and their refresh, and its connect codes. Nothing cascades, so the keys it issued keep working ([ADR-0097](../adr/ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md) d14). Revoking is narrowing and takes effect at once; revoking a person's key is loud, a notice to every admin person (d24).

1. Revoke the key, with the reason the journal keeps.

   ```bash
   oml keys revoke <key> --reason "<why>"
   ```

2. When the key's holder still needs access, issue a replacement. An issuance is widening: outside `sole_admin` it waits until a second admin person confirms it with `oml admin confirm <act> --reason "<why>"`.

   ```bash
   oml keys issue <key-name> --role <role> --grant <workspace>:<role> --reason "<why>"
   ```

3. Check that the old key is revoked: `oml keys show <old-key>` names its revocation.

To give a key a new value and keep its grants instead, use `oml keys rotate <key>`, with `--grace` (at most 24 hours) when its holder needs time to switch. A session key, the server's reserved key and a revoked key are refused by name.

## A dispute between two admins is frozen, or a tenant's admins are dormant

Use this when `oml doctor` raises `admin.dispute_open` (critical) or `operator.settlement_due`, or when no admin person of a tenant has signed in for 30 days. [ADR-0097](../adr/ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md) d19 gives the operator two host verbs for these cases. Both are built in v0.8.6. Recovery's dormancy is read from the server's authentication table, one row per key and UTC day kept 30 days (answer 24 of 2026-11-28).

### Settle a freeze from the host

A freeze is an act against an admin person, an un-flag, removal, disable or entry change, left open in a tenant with fewer than three admin persons. Both parties lose widening until it is settled. Once it has been open 24 hours, a placeholder, doctor raises `operator.settlement_due`, and the operator settles it from the host.

1. Read the act's id: an admin person of the tenant lists it with `oml admin acts --state effective`, and doctor counts it. Read the tenant's id from that person's `oml whoami`.
2. On the server's host, as root, run:

   ```sh
   oml operator settle <tenant id> <act id> --confirm --token-dir <operator.token_dir> --url <server>
   ```

   Use `--reverse` in place of `--confirm` to reverse the act. `--token-dir` reads `OML_OPERATOR__TOKEN_DIR` when it is not given.
3. Read the answer. `--confirm` revokes the target's suspended admin keys, `cause: settlement`. `--reverse` lifts the suspension and restores the entry. The answer names the persons notified and what the mail relay did: it took the email, no relay is configured, or it refused it, with the SMTP stage and code. The settlement stands in every case.

The verb reads the tenant's token from `<operator.token_dir>/<tenant id>.token`. Each server start writes one token per tenant there, mode 0600 in a 0700 directory, so only root and the server's own user read it. A restart replaces every token, and a tenant created after the start has no token until the next start. The token is never printed, logged or journalled, and no admin key is involved (answer 13 of 2026-11-27). The route refuses a request that also sends a key (`auth.invalid`, `operator_route_takes_no_key`) and a token that is not the tenant's of the current start (`operator_token`); a request with no token is `auth.missing`. It refuses an act that is not a freeze (`not_frozen`) and a freeze younger than 24 hours (`freeze_too_recent`).

One commit journals the settled `admin.act` with no settler, `operator.settlement`, the effect's events, and a `settlement` notice to every admin person and both parties. The settlement issues no key and reads no content.

When `/readyz` reports `operator_tokens: failed`, doctor raises `operator.token_unwritten`. Read the start log's `operator_tokens_unwritten` line, fix `operator.token_dir` or its permissions, and restart the server.

### Recover a dormant tenant

Use this when no admin person of a tenant has signed in, and no admin key of it has authenticated, for 30 days, a placeholder, and the tenant needs an admin again. The dormant admins keep their flags, so the new admin cannot widen alone until one of them returns, or, with one dormant admin, until the operator settles the new admin's un-flag of that admin (above). With two or more dormant admins that un-flag opens no freeze, so the new admin's widening waits for one of them to return: an accepted limit (d19).

1. Check the coverage. `oml doctor` reports `auth.log_coverage`: the whole days the server's authentication table reaches back, deployment wide. Under 30 it warns, and the verb refuses `log_too_short`. A fresh install or an upgrade to v0.8.6 covers nothing on its first day and reaches 30 days after 30 days of traffic. A day on which no key of any tenant authenticated leaves a gap at the table's oldest end, so a quiet deployment can read short; the verb then refuses rather than reading the gap as dormancy. This is a known limit, tracked for v0.8.7.
2. Read the tenant's id. With no admin person to run `oml whoami`, it is the name of the tenant's token file in `operator.token_dir`, `<tenant id>.token`.
3. On the server's host, as root, run:

   ```sh
   oml operator recover <tenant id> --person <name> --email <address> --token-dir <operator.token_dir> --url <server>
   ```

   `<name>` is the admin person the recovery adds, a name the tenant does not hold as a workspace, key or entry, and `<address>` the address they sign in with. `--token-dir` reads `OML_OPERATOR__TOKEN_DIR` when it is not given, as `oml operator settle` does.
4. Read the answer. It names the recovery's id, when it completes, the persons notified and what the mail relay did. Every admin person gets a `recovery` notice in To do and an email, which is how a dormant admin learns of it: configure the relay first (below).
5. Wait. Any admin person's sign-in, or any admin key's authentication, in the 72 hours, a placeholder, cancels the recovery: the server's sweep, once a minute, journals `operator.recovery_cancelled`. With none, the sweep completes it once the 72 hours pass: in one commit every admin key of the tenant is revoked, `cause: recovery`, the named person's entry is added as an admin person, and `operator.recovery` records the window and the revoked keys. The new admin person signs in to the console with that address, and the server issuer issues their session keys.

The verb is refused `operator.recover_refused` with `details.reason` `log_too_short` (step 1), `log_write_failed` (this server process could not write some authentication rows since it started), `admin_active` (an admin authenticated in the 30 days), `pending` (a recovery already waits), `dispute_open` (an act against an admin is open: settle it first), `person_taken` or `email_taken`. The route refuses a key and a wrong token as the settle route does.

While a recovery waits, doctor raises `admin.recovery_pending` (critical). Past its 72 hours and not completed, it raises `admin.recovery_overdue` (critical): the sweep is failing for that tenant. Read the server log's `recovery_sweep_failed` lines. A name or address taken while the request waited is refused by name there, and the request stays; ask the tenant's other users which entry holds it. The sweep's state is the database's, so a restart resumes it, and each request completes once whichever replica's sweep reaches it first. When doctor raises `auth.log_write_failed`, the server could not write some authentication rows since it started. The key's next authentication writes the row again, but a row that was never written would read as dormancy, so while that process counts any such failure it refuses the verb `log_write_failed`. The count is each process's own: another replica that counted none admits the request, so run the verb against a replica whose doctor reads `auth.log_write_failed: 0`, or fix the write path and restart (answer 26 of 2026-11-28).

Do not edit the database by hand: the journal's tamper evidence is the control against the host.

### Notice email

Recovery and settlement notices are emailed to every admin person, since a dormant admin sees nothing in-product (d20). Set `mail.host` and `mail.from`, and `mail.port` when the relay is not on 25 ([configuration](configuration.md)). The relay is a local one, such as Postfix on the host, which handles TLS and authentication. The server speaks plain SMTP to it and holds no mail credential (answer 14). With no relay set, doctor warns `notice.mail_unconfigured`. When the relay refuses or cannot be reached, doctor raises `notice.mail_delivery_failed` with the last reason, and the start log has a `notice_mail_failed` line. The count is the process's own, and a restart clears it.
