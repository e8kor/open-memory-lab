---
status: Stable
verified: 2026-11-12
answers: "how to upgrade an OML server: how migrations run, the upgrade steps in order, what rollback does and does not mean, the upgrade from a pre-0.3.1 install, the v0.5.4 fix, and how to move from v0.3"
---

# Upgrade OML

An upgrade replaces the server binary. The new server applies the schema migrations itself at boot. You back up
first, because rollback restores binaries and never the schema.

## How migrations run

Migrations are numbered `NNNN_name` and run forward only. The server runs them at start under
`postgres.owner_url`, closes that connection pool, and serves under `postgres.url`, the serving role. The
serving role never holds DDL rights. The server refuses to start, with `postgres.role_split`, when the two URLs
log in as one role.

There are no down migrations. A down path on a system of record is untestable, because nothing can
un-happen against a journal. You run no migration step of your own.

## Upgrade in order

You need a recent backup and the release you are moving to.

1. **Verify the release.** Check the cosign signature of every archive you install
   ([deploy](deployment.md#install-the-server-archive)).
2. **Back up the database and the keyring together** ([Back up and restore](restore.md)). Take the dump
   as a superuser and name it for the version it precedes.
3. **Rehearse on a copy.** Restore the dump into a throwaway PostgreSQL cluster of the same major version.
   Give it the owner and serving roles and a copy of the keyring. Boot the new server against it on another
   loopback port. Check the migrations, `oml doctor` and a recall over the copied data. Delete the copy afterward. A release that fails here
   goes no further.
4. **Set `server.shared` first, on a server other people use.** Set `server.shared = true`
   (`OML_SERVER__SHARED`) before the new binary starts, or in the same deploy ([configuration](configuration.md)).
   A server that does not say it is shared gets no consent prompt from `oml sync`, and nothing warns you that
   the setting is missing: a CLI of this release syncs to it with no disclosure to its users until you set it.
   Open sign-up implies it, and a server only you use leaves it `false`.
5. **Swap and restart.** Keep the running binary as `<name>.v<old>`, install the new one and restart. The
   migrations run at boot. On Kubernetes, follow [Install OML on Kubernetes](../guides/kubernetes.md).
6. **Upgrade the lake writer after the server**, if you run one, so it meets the migrated schema
   ([Run the lake writer](lake.md)).
7. **Check.** `/readyz` names the new version, the log carries `role_split_booted`, and `oml doctor` answers
   `ok` for the agent and admin keys.

One server serves every request, so every upgrade is an outage. The server stops, starts, runs its
migrations and passes the fork check before it answers. The window is not measured.

## Migration 0044 fills each promoted version's approval

Migration 0044 adds `approver_held_by`, `approver_same_user` and `approved_at` to `memories`, then fills them
for every version promoted before it, from the verdict the journal's promotion event names. It runs at boot,
before the server serves, so the window of the outage grows with the number of promoted versions. Measured on a
loaded shared host with PostgreSQL 18:

| Promoted versions | Fill duration |
|---|---|
| 10,000 | 0.84 to 1.5 s |
| 100,000 | 13.4 to 21.6 s |

The fill rewrites each promoted row once, so `memories` grows by about 70% until vacuum reclaims the old row
versions. Rehearse on a copy (step 3) to time it on your data.

A version whose promotion event names no verdict the journal holds keeps `approved_at` null. It stays served,
with `approver` and `approved_at` null. After the upgrade, `oml doctor` run with an admin key names the count
as `approval_unmatched`: the served versions with no approval. Doctor reads it from `promoted_unapproved` on
`GET /v2/admin/rescan`, which counts it in the pass that already counts the scan backlog. It is a count; no command lists which versions
they are. To clear one, supersede it with `oml write --replaces`. Once a reviewer approves the successor, it
carries its own approval and the version it replaced is no longer counted. Leaving one as it is changes
nothing else.

## Roll back

Roll back by installing the previous binary only while no migration has run. After a migration, the way back is
the dump from step 2, restored through [Back up and restore](restore.md). The cost is every write since the
dump.

## Install the server, CLI and plugin together

The server, the `oml` CLI and the Claude Code plugin move in one window. An old CLI sends request
spellings that a new server refuses, and each refusal names the new spelling.

Update the plugin with `claude plugin update oml@open-memory-lab`. Then set its `url` and `api_key` with
`/plugin configure oml@open-memory-lab`. The plugin serves no tools until both are set.

## Upgrading from v0.3

v0.5.1 removed the eight v0.3 agent tools and the old CLI verbs. A raw MCP client that calls an old tool
name now gets JSON-RPC -32602 `Unknown tool`. An old CLI verb is a usage error with exit code `2`, and
nothing is sent.

| Removed | Use instead |
|---|---|
| MCP `oml_remember`, `oml_learn` | `oml_write`. Set `derives_from` to record what a memory derives from. |
| MCP `oml_supersede` | `oml_write` with `replaces` |
| MCP `oml_recall` | `oml_read` with `query`. `include_unreviewed` replaces `include_own_held`, and `workarea` replaces `scope`. |
| MCP `oml_get` | `oml_read` with `artifact_id` |
| MCP `oml_my_work` | `oml_read` with neither argument |
| MCP `oml_workarea_info` | `oml_workarea` |
| MCP `oml_events_tail` | `oml_read` with `state` for an agent's own writes. `GET /v2/events` for an audit. |
| `oml submit --claim <text>` | `oml write <text>` |
| `oml recall --query` | `oml read --query` |
| `oml supersede --target <id> --claim <text>` | `oml write <text> --replaces <id>` |
| `oml review verdict --memory <id> --approve\|--reject` | `oml review approve` or `oml review reject` |
| `--server`, `--key`, `OML_CLI_SERVER`, `OML_CLI_KEY` | `--url`, `--api-key`, `OML_URL`, `OML_API_KEY` |

A `permissions.allow` rule that names an old tool allows nothing. Allow `mcp__plugin_oml_oml__oml_read` and
`mcp__plugin_oml_oml__oml_workarea`, or `mcp__plugin_oml_oml` to allow all three tools, the write included.

The CLI reads `url` and `api_key` from its credentials file. Run `oml login <url>` once to rewrite a v0.3
file, which used `server` and `key`. The [changelog](../changelog.md) lists every break.

## Upgrading from v0.5.1 or earlier

Install v0.5.4 or later over a database that a v0.5.1 or earlier server ran. v0.5.3 refuses to start on
a database whose `universe` root stores the status `development`, with `check constraint
"workareas_universe_committed" ... is violated`. The failed start changes nothing, and a v0.5.1 server serves the
database again.

From v0.5.4, the server aligns each workarea's stored status with the status v0.5.1 served, before migration
`0017` runs. Nothing is journalled, because reads always served that status. You take no extra step. Back up
and start v0.5.4 or later.

## Splitting an install from before v0.3.1

A build before v0.3.1 migrated and served under one PostgreSQL role that owned every table. v0.3.1 and later
refuse that role on both URLs. You split it once, in one outage. A superuser runs every statement, connected to
the install's database. The old build stays stopped from step 2 to the end, because it cannot run once ownership moves.

1. **Back up as a superuser.** `pg_dump` fails when its role cannot bypass row security.
2. **Stop the old build.**
3. **Create the roles.** Each has `LOGIN` and none of `SUPERUSER`, `CREATEDB`, `CREATEROLE`, `REPLICATION` or
   `BYPASSRLS`. The serving role is a member of no role. The server refuses a privileged serving role at boot,
   with `serving_role_privileged`.

   ```sql
   CREATE ROLE <owner> LOGIN PASSWORD '...';
   -- the kept role, when it holds a refused attribute:
   ALTER ROLE <role> NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
   ```

4. **Move ownership to the owner.** A kept role that is not a superuser keeps its URL and becomes the serving
   role. Check which databases it owns first, because `REASSIGN OWNED` moves every database the role owns:

   ```sql
   SELECT datname FROM pg_database WHERE datdba = '<role>'::regrole;
   REASSIGN OWNED BY <role> TO <owner>;
   GRANT CONNECT ON DATABASE <database> TO <role>;
   REVOKE CREATE ON DATABASE <database> FROM <role>;
   ```

   `REASSIGN OWNED` refuses a superuser. Then run `ALTER ... OWNER TO <owner>` on the database and on each
   table, function and type in `public`, including `ALTER DOMAIN oml_name OWNER TO <owner>`. Skip the members of
   an extension. The `ltree` extension stays the superuser's.
5. **Set both URLs.** `OML_POSTGRES__OWNER_URL` logs in as the owner. `OML_POSTGRES__URL` logs in as the
   serving role. Both name the same database.
6. **Start the new server.** It migrates under the owner URL and then serves. The split worked when the
   log carries `role_split_booted` with `owner_role` and `serving_role`, and `/readyz` answers 200.

Until the new server starts, you can reverse the move with `REASSIGN OWNED BY <owner> TO <role>` and restart
the old build. After it starts, the way back is the backup.
