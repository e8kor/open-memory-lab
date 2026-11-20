---
status: Stable
verified: 2026-11-19
answers: "how to upgrade an OML server: how migrations run, the upgrade steps in order, what rollback does and does not mean, the upgrade from a pre-0.3.1 install, the v0.5.4 fix, how to move from v0.3, the v0.8.4 role rename and the v0.8.3 rename that clients take before the server, what the v0.8.3 migrations cost and what waits during them, and the storage.url to set before v0.8.3's uploads"
---

# Upgrade OML

An upgrade replaces the server binary. The new server applies the schema migrations itself at boot. The operator backs up
first, because rollback restores binaries and never the schema.

## How migrations run

Migrations are numbered `NNNN_name` and run forward only. The server runs them at start under
`postgres.owner_url`, closes that connection pool, and serves under `postgres.url`, the serving role. The
serving role never holds DDL rights. The server refuses to start, with `postgres.role_split`, when the two URLs
log in as one role.

There are no down migrations. A down path on a system of record is untestable, because nothing can
un-happen against a journal. No migration step is run by hand.

## Upgrade in order

A recent backup and the release to move to are required.

1. **Verify the release.** Check the cosign signature of every installed archive
   ([deploy](deployment.md#install-the-server-archive)).
2. **Back up the database and the keyring together** ([Back up and restore](restore.md)). Take the dump
   as a superuser and name it for the version it precedes.
3. **Rehearse on a copy.** Restore the dump into a throwaway PostgreSQL cluster of the same major version.
   Give it the owner and serving roles and a copy of the keyring. Boot the new server against it on another
   loopback port. Check the migrations, `oml doctor` and a recall over the copied data. Delete the copy afterward. A release that fails here
   goes no further.
4. **Set `server.shared` first, on a server other people use.** Set `server.shared = true`
   (`OML_SERVER__SHARED`) before the new binary starts, or in the same deploy ([configuration](configuration.md)).
   A server that does not say it is shared gets no consent prompt from `oml sync`, and nothing warns that
   the setting is missing: a CLI of this release syncs to it with no disclosure to its users until it is set.
   Open sign-up implies it, and a server used by one person leaves it `false`.
5. **Swap and restart.** Keep the running binary as `<name>.v<old>`, install the new one and restart. The
   migrations run at boot. On Kubernetes, follow [Install OML on Kubernetes](../guides/kubernetes.md).
6. **Upgrade the lake writer after the server**, if one runs, so it meets the migrated schema
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
versions. Rehearse on a copy (step 3) to time it on the data.

A version whose promotion event names no verdict the journal holds keeps `approved_at` null. It stays served,
with `approver` and `approved_at` null. After the upgrade, `oml doctor` run with an admin key names the count
as `approval_unmatched`: the served versions with no approval. A retirement tip is not served, so it is not counted.
Doctor reads it from `promoted_unapproved` on `GET /v2/admin/rescan`, which counts it in the pass that already counts the
scan backlog. It is a count; no command lists which versions they are. To clear one, supersede it with `oml write --replaces`. Once a reviewer approves the successor, it
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

## Upgrading to v0.8.4

v0.8.4 renames the writer role on every surface in one cut, with no alias
([ADR-0093](../adr/ADR-0093-contributor-and-submitter-replace-writer.md)). A key that recalls and
writes is a `contributor`, a key that writes without recall is a `submitter`, and the grant role
that confers writing is `submitter`. A v0.8.4 server reads `writer` as unknown input: as a role on
HTTP, MCP, the SDKs or the CLI it is `request.validation`, with no `details.renamed_to`, because it
has two successors. `oml keys issue-agent --write-only` is retired; `--role submitter` replaces it.

**Clients upgrade before the server.** A v0.8.3 CLI, plugin or SDK that sends `writer`, or that
sent `--write-only`, is refused and issues nothing. The notice to send before the date:

> OML v0.8.4 renames the writer role. A key that recalls and writes is a **contributor**. A key
> that writes without recall is a **submitter**. The grant that confers writing is `submitter`.
> Upgrade the CLI, plugins and SDKs before the server is upgraded. A v0.8.3 client that names
> `writer` is refused and issues nothing. Existing writer keys become contributor keys. One issued
> with `--write-only` keeps working, and `oml doctor` names it a contributor without recall. Issue a
> submitter key with `--role submitter` and cut the old one off. Memories, key values, grants and
> names are otherwise unchanged.

**The upgrade is not rolling** (ADR-0093 d8, following ADR-0091 d12's runbook):

1. Announce the window with the notice above.
2. Back up and verify that the backup restores ([restore](restore.md)): migration 0066 has no
   reverse.
3. Stop `oml-web`, the lead and the lake writer, and start v0.8.4 of all three from one tag.
   Migration 0066 rewrites `api_keys.role` from `writer` to `contributor` and `grants.role` from
   `writer` to `submitter` under each tenant's row security, and replaces the two role checks. It
   reads two small tables, and the stopped server leaves its locks free.
4. Check `oml doctor`, `oml verify` and migration 0066's law (ADR-0093 d5): no role check names
   `writer`, so no role column can hold it. As the database owner,
   `SELECT conname, pg_get_constraintdef(oid) FROM pg_constraint WHERE conname IN
   ('api_keys_role_check', 'grants_role_check');` lists the two checks, and neither names `writer`.
   Signed events keep `writer` and are read through the former-name map's rule 7; a `writer` the
   map does not cover fails `journal_unmapped_former_name`.
5. Upgrade the operator's own clients, and reissue any key `oml doctor` names under
   `key_contributor_without_recall`: issue a submitter key with
   `oml keys issue-agent <name> --role submitter` and cut the old one off.

## Upgrading to v0.8.3

v0.8.0 to v0.8.2 were never released: this section applies to an upgrade from v0.7.3 to v0.8.3.

v0.8.3 is one upgrade with four parts, each below: the rename that clients take in the same window
as the server, the migrations whose time adds to the outage, the `storage.url` to set before uploads
and imports are used (an export writes no file on the server), and the rescan the new scanner rule
set asks for. Read all four before the window is announced.

### Workspaces, reviewer, protected, frozen, used by: clients upgrade with the server

v0.8.3 renames five names on every surface in one cut, with no alias
([ADR-0091](../adr/ADR-0091-workareas-become-workspaces-and-the-dreamer-role-becomes-reviewer-on-every-surface-in-one-cut-and-the-journals-former-names-are-read-through-one-map.md)):
workareas become workspaces, the role `dreamer` becomes `reviewer`, the statuses `committed` and
`validated` become `protected` and `frozen`, and a key's `held_by` becomes `used_by`, its value
`human` becoming `person`. A v0.8.3 server reads a former name as unknown input: a former path
answers 404, a former field or enum value is `request.validation`, and a former query parameter or
environment variable is ignored. A v0.7 client that names its workspace only by the former query
parameter is therefore not refused, and its write lands, held, in its key's default workspace. The
header that v0.4.0 already retired is still refused naming its successor.

**Clients and the server upgrade in one window.** The cut breaks both ways: a v0.7 client fails
against a v0.8.3 server, and a v0.8.3 client fails against a v0.7 server. Announce one window to
every key holder, upgrade the server in it, and have every key holder upgrade the CLI, the plugins
and the SDKs in the same window. The notice to send before the date:

> OML v0.8.3 renames workareas to workspaces. A v0.7 CLI, plugin, SDK or console bookmark stops
> working against a v0.8.3 server, and a v0.8.3 one does not work against a v0.7 server: paths,
> fields, the MCP tool `oml_workarea` (now `oml_workspace`), the CLI noun, `OML_CHAT_WORKAREA` and
> the plugin setting `chat_workarea` all change. Upgrade the CLI, plugins and SDKs in the announced
> window: v0.8.3 ignores an old client's workarea query parameter, so a write it sends can land in
> the key's default workspace. Set `chat_workspace`, and update saved permission rules that name
> `oml_workarea`. Memories, keys, grants and workspace names are unchanged. The role `dreamer` is
> now `reviewer`: `--role reviewer` and grants such as `dev:reviewer`. The statuses `committed`
> and `validated` are now `protected` and `frozen`, and a key's `held_by` is now `used_by`, with
> `human` now `person`. Existing keys, grants and workspaces are converted by the upgrade. A read
> without a query now lists the catalog of what the key reads (`oml_read`'s `own` defaults to
> `false`, so an agent asks for its own work with `own: true`; `oml read` with no flags; the SDKs'
> iterators), `kind` is now `kinds` on recall, the listing and the admin search, `oml read --all`
> and `--kind` are removed, and a recall with no `kinds` also returns skill, guardrail and document
> hits; a test is never a recall hit. The Rust SDK's `RecallResponse.steering` is now an `Option`.

**Before the server.** Take a backup and verify that it restores
([restore](restore.md)): migration 0047 has no reverse, and only that verified dump undoes it.
Check that every function whose body names a former name belongs to the role the migrations run
as, since 0047 re-creates each one:

```sql
SELECT proname, proowner::regrole FROM pg_proc
 WHERE pronamespace = 'public'::regnamespace AND prosrc LIKE '%workarea%';
```

**The server.** The upgrade is not rolling: a v0.7 binary cannot read the renamed schema. Follow
[Upgrade in order](#upgrade-in-order) with every node stopped, the console and the lake writer with
them, since the lake writer reads the renamed tables too. At boot the server applies one migration
that renames the schema and converts the stored roles, statuses and holder values. Then check
`oml doctor`, which names `journal_unmapped_former_name` when a journal event spells a former name
the server cannot map, and `oml verify` for every tenant. The signed journal keeps the former names
in the events written before the upgrade; every reader maps them to the new ones after verifying the
signed bytes, so nothing signed is rewritten. An `Idempotency-Key` retried across the cut replays
the response the v0.7 server recorded, in the former names, for as long as the replay window keeps
it.

**After the server.** Set `chat_workspace` wherever `chat_workarea` was set, update permission
rules that name `mcp__plugin_oml_oml__oml_workarea` to `mcp__plugin_oml_oml__oml_workspace`, and
supersede every reviewed steering, guardrail or skill whose text names a former name, each as a
held version reviewed as usual. Bookmarks of the pages `use/workareas/` and
`architecture/workareas/` answer 404; the pages are now [workspaces](../use/workspaces.md) and
[how workspaces grant access](../architecture/workspaces.md).

### What the migrations cost

The server runs the migrations from 0047 on at boot, before it serves, so their time adds to the
outage of [Upgrade in order](#upgrade-in-order). The rest change the catalog, add empty tables or
columns with constant defaults, or touch a few small tables; the ones below read or write rows in
proportion to the stored data. Each waits at most 5 s for a lock, and 0060, 0061 and 0062 commit
after each step, so a run that fails resumes where it stopped.

- **0047**, the rename ([ADR-0091](../adr/ADR-0091-workareas-become-workspaces-and-the-dreamer-role-becomes-reviewer-on-every-surface-in-one-cut-and-the-journals-former-names-are-read-through-one-map.md)), updates the stored roles, holders and statuses tenant by tenant, every promoted version that carries an approver among them, in one transaction.
- **0049** runs `VALIDATE CONSTRAINT` twice on `memories`, a full scan of every version, under a lock that lets reads and writes run.

| Migration | What it does to each tenant | What waits, and for how long |
|---|---|---|
| 0060 | Builds the postings index that carries a document passage's ordinal on the tenant's partition, then refills the postings and counts of a tenant that holds a document | Reads never wait. Every claim write of that tenant waits for the whole build of its partition: a write, a verdict, a promotion and an erasure alike, because a write's postings are written in the same transaction as the version. The review of PR #475 measured about 5 s per 1.8M postings. Other tenants' writes do not wait. |
| 0061 | Records the author's person on every version written by a key that acts for a person | Reads never wait; writes wait only for the rows the update touches. Not measured. |
| 0062 | Builds the catalog index on the tenant's partition, then, for a tenant that holds a skill, a guardrail or a vocabulary kind, rewrites every posting and count of that tenant, not only the new kinds' | Measured at about 4.5 s for a tenant of 20,000 versions, about 225 s per million versions, during which that tenant's writes wait on the rows the refill rewrites. A tenant with none of those kinds is read by one probe and left as it is. |
| 0065 | Recomputes each account's `text_bytes` from its stored versions, so the proposals a person's review key wrote before the upgrade are charged to that person and the drift report finds none | One transaction for every tenant. It reads every version of each tenant twice, as 0045's fill read them once, which took 550 to 634 ms over 100,000 stored versions; 0065 itself is not measured. No version is rewritten, only one meter row per account. |

On one server nothing else writes while the migrations run, so these waits lengthen the outage
rather than stall requests. The work of 0060 and 0062 grows with each tenant's postings and
versions: rehearse on a copy ([step 3](#upgrade-in-order)) and time both on the data before the
window is announced. Before the window, also read [the rename](#workspaces-reviewer-protected-frozen-used-by-clients-upgrade-with-the-server),
and set `storage.url` before uploads or imports are used:
see [set `storage.url` before using uploads](#set-storageurl-before-using-uploads).

### Set `storage.url` before using uploads

v0.8.3 adds upload sessions, which keep their encrypted parts in the directory `storage.url` names
([ADR-0004](../adr/ADR-0004-bodies-and-attachments.md) d3). The default, `oml-objects`, is relative to
the server's working directory, which a systemd unit or a container often cannot write. The server
starts without it: it logs `upload_store_unavailable` once with the reason, serves every other route,
and refuses each upload and import with `upload.storage_failed` (503) until the directory can be
created. Before uploads or imports are used, set `storage.url` (`OML_STORAGE__URL`) to an absolute
path the server's user can write, for example `/var/lib/oml/objects` under a unit whose
`StateDirectory=oml`. `oml doctor` warns `upload_store_unavailable` while the directory cannot be
opened, and the next sweep, within five minutes, or the next upload opens it once it can. The release image
sets `OML_STORAGE__URL=/var/lib/oml/objects`, and an environment variable wins over the TOML file, so a
`[storage] url` in a configuration file is ignored under the image unless the variable is set too. The
Compose file mounts a writable volume at that directory, and the Helm chart sets
`/var/lib/oml/objects/store`, a directory the server creates below its volume, because the volume's own
root belongs to root and the server never changes the mode of a directory it does not own.

### Rescan held versions under `oml-scan/1.3`

v0.8.3 moves the scanner's rule set from `oml-scan/1.2` to `oml-scan/1.3`, which vetoes an age identity
([ADR-0065](../adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md) d1, d2),
so the release notes carry the marker `<!-- oml-deployer: post-step admin-rescan-record-held -->`,
which an installer may read. After the server is up, run
`oml admin rescan --record-held` under an admin key. Until it has run, `oml doctor` warns
`scan_held_unrescanned` and `scan_rescan_due` for the versions scanned under `oml-scan/1.2`; nothing is
demoted or deleted meanwhile.

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
| MCP `oml_my_work` | `oml_read` with `own: true` |
| MCP `oml_workarea_info` | `oml_workarea` |
| MCP `oml_events_tail` | `oml_read` with `own: true` and `state` for an agent's own writes. `GET /v2/events` for an audit. |
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
`0017` runs. Nothing is journalled, because reads always served that status. No extra step is needed. Back up
and start v0.5.4 or later.

## Splitting an install from before v0.3.1

A build before v0.3.1 migrated and served under one PostgreSQL role that owned every table. v0.3.1 and later
refuse that role on both URLs. The split happens once, in one outage. A superuser runs every statement, connected to
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

Until the new server starts, the move can be reversed with `REASSIGN OWNED BY <owner> TO <role>` and restart
the old build. After it starts, the way back is the backup.
