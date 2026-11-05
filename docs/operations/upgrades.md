<!--
id: operations/upgrades
tier: engineering
status: design draft for review · decided, not built
answers: who runs migrations and when; in what order the binaries move; how much version skew a rolling update may hold; what rollback does and does not mean
-->

# Upgrades

**Status:** design draft for review · decided, not built — no upgrade has ever
run, because no version has ever shipped; this spec is `0.5.8-design`
([openapi.yaml](../api/openapi.yaml)) and the page commits the shape of every
future N → N+1.

**Release:** forward-only migrations since v0.1.0; the role split since v0.3.1 ([releases](../product/releases.md)).

This page owns the upgrade discipline: who runs migrations, in what order the
binaries move, how much version skew a rolling update may hold, and what
rollback does and does not mean. The workloads being upgraded are placed in
[deployment](deployment.md); the escape hatch when an upgrade damages data —
not binaries — is [restore](restore.md).

## Migrations: forward-only, owner-role, bootstrap-run

The schema moves by **sqlx migrations, forward-only, numbered `NNNN_name`**,
and they are executed by **the bootstrap workload under the owner role — on
first install and on every upgrade**. The reasons, one per clause:

- *sqlx migrate* because the stack is already sqlx
  ([ADR-0006](../adr/ADR-0006-pure-rust.md)) and a second migration tool is a
  second thing to trust with DDL.
- *Forward-only* because down migrations are a fiction on a system of record:
  no `DROP` can un-happen against a journal, and a tested forward path is
  worth more than an untested reverse one. There are no down files to drift.
- *Numbered `NNNN_name`* so the applied set is an ordered, auditable list the
  bootstrap workload can verify, not a graph to resolve.
- *The bootstrap workload under the owner role* because C6's role split is a
  profile invariant ([deployment](deployment.md)): service roles never hold
  DDL, so the only process that can migrate is the one that already owns
  first-install schema creation — the Helm hook Job, the compose one-shot, or
  the standalone bootstrap step. Re-running it on upgrade is the same
  ceremony, idempotent over an installed deployment: migrations apply
  forward from the recorded position; the first-install-only acts (keyring
  mint, enrollment secret) are skipped when their artifacts exist. The
  standalone invocation that re-runs only the migration step has no named
  verb yet — performed by the bootstrap workload is the commitment; the verb
  is a CLI-reference question, tracked in the [roadmap](../product/roadmap.md).

The bootstrap workload is decided, not built. Until it is, the lead runs the
migrations at boot under `postgres.owner_url`, closes that pool and serves
under `postgres.url`, the serving role, which owns nothing; it refuses to
start, with `postgres.role_split`, when the two URLs log in as one role
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)
d4, d5). Step 2 below is then the lead's own boot at step 5.

The forward-only rule forces the **expand-contract discipline**: every
migration in version N+1 must leave a schema that version N binaries still
read and write correctly, because the skew policy below guarantees N binaries
will run against it during the roll. Additive changes land with N+1;
destructive cleanup of what N needed waits for N+2. This is stated as policy,
enforced by review until a conformance check exists.

## The upgrade order

Every upgrade, every profile, in this order:

1. **Back up first.** A pre-upgrade backup (and, in `kms-file` profiles, a
   keyring copy) is a requirement, not advice: rollback below is binaries
   only, so the backup plus [restore](restore.md) is the only path back from
   an upgrade that damaged data.
2. **Migrations** — the bootstrap workload re-runs (`helm upgrade` re-fires
   the hook Job; `docker compose up` re-runs the one-shot and every service
   gates on its completion; standalone runs the bootstrap step before the
   supervisor starts the new binaries). The schema is now N+1; N binaries
   keep working by the expand-contract rule.
3. **The asynchronous workloads: `oml-lakewriter`, `oml-reconciler`,
   `oml-indexer`.** First because nothing client-facing depends on their
   restart — a lake-writer restart is a frontier-lag blip, a reconciler
   restart delays a pass — and because they prove the N+1 binaries against
   the migrated schema before anything on the serving path moves.
4. **Followers.** Rolling; read capacity dips by one replica at a time and
   writes are untouched (followers forward writes,
   [deployment — the lead lease](deployment.md)). On the schema version that
   adds names, an N+1 follower reports not ready on `/readyz` and serves no
   request until it has folded `system.names_assigned` for every chain, which
   only the N+1 lead journals, at step 5
   ([rows created before names](../api/http-api.md#rows-created-before-names));
   `oml doctor` names the wait (`names_assigned`,
   [observability](observability.md#the-doctor-contract)).
5. **The lead, last.** Restarting the lead is a lease takeover: a
   write-refusal window ending in the fork check and a journalled
   `system.lead_acquired` ([deployment](deployment.md)). Last, so the one
   unavoidable write-refusal window of the upgrade lands after every other
   component is already proven on N+1 — if anything earlier went wrong, the
   upgrade aborts without ever having touched the write path. The window's
   size is unmeasured ([FR-24](../product/roadmap.md)); it counts against
   the write SLI ([observability — SLOs](observability.md#slos)), which is
   the honest accounting, not an exemption.

**The upgrade that adds names is outside the expand-contract rule.** Its
migration renames the PoC's `tenants.name` and `workareas.name` to
`prior_name` ([PostgreSQL schema](../data/postgres-schema.md#names-and-tenants)),
columns the N binaries read, so every N binary stops before step 2, and the
deployment serves nothing from step 2 until the N+1 lead has journalled
`system.names_assigned` at step 5 and the followers have folded it. The
write-refusal window of that one upgrade is the whole upgrade, stated
plainly.

In standalone there is no rolling anything — the supervisor stops all four
children and starts the new ones; the order above collapses to "bootstrap
step, then `oml standalone up`". The write-refusal window is the whole
restart, stated plainly: a one-host profile upgrades with a one-host outage.

## Splitting a single-role install (the upgrade to v0.3.1)

*Available since v0.3.1 · [releases](../product/releases.md)*

A build before v0.3.1 migrated and served under one Postgres role, which
owned every table. v0.3.1 refuses that role on both URLs, so an install made
before it is split once, in one outage, by moving ownership to a new owner
role ([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md)
d7). The old build cannot run once ownership moves, so it stays stopped from
the second step to the last. A superuser runs every statement, connected to
the install's database.

**Which role becomes which.** A role that is not a superuser keeps its URL,
`postgres.url`, and becomes the serving role; a new role, which the operator
names, becomes the owner. A superuser cannot serve (d5): it stays the
administrator, both roles are created, and `postgres.url` moves to the
serving one. The payloads that own their cluster call the roles `oml_owner`
and `oml_server` (d2).

1. **Back up, as a superuser.** `pg_dump` errors when its role cannot bypass
   row security ([pg_dump](https://www.postgresql.org/docs/17/app-pgdump.html)),
   so the backup step 1 of the upgrade order requires is taken as a
   superuser, with the keyring copy in `kms-file` profiles.
2. **Stop the old build.**
3. **Create the roles, each with `LOGIN` and no other attribute** (d2): not
   superuser, `CREATEDB`, `CREATEROLE`, `REPLICATION` or `BYPASSRLS`, and the
   serving role a member of no role. The lead checks the serving role at boot
   and refuses it, with `serving_role_privileged`, when it holds any of them.

   ```sql
   CREATE ROLE <owner> LOGIN PASSWORD '…';
   -- the kept role, when it holds an attribute d2 refuses:
   ALTER ROLE <role> NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
   ```

4. **Move ownership to the owner.** For a kept role that is not a superuser,
   check first which databases it owns, since `REASSIGN OWNED` moves every
   database the role owns on the cluster
   ([REASSIGN OWNED](https://www.postgresql.org/docs/17/sql-reassign-owned.html)):

   ```sql
   SELECT datname FROM pg_database WHERE datdba = '<role>'::regrole;
   REASSIGN OWNED BY <role> TO <owner>;
   -- the kept role connected as the database's owner; once ownership moves it
   -- holds CONNECT only if granted, and d2 grants the serving role CONNECT:
   GRANT CONNECT ON DATABASE <database> TO <role>;
   -- when the kept role was granted CREATE on the database, which d2 refuses it:
   REVOKE CREATE ON DATABASE <database> FROM <role>;
   ```

   For a superuser, which `REASSIGN OWNED` refuses as the bootstrap role, run
   `ALTER … OWNER TO <owner>` on the database and on each table, function and
   type in `public`, `ALTER DOMAIN oml_name OWNER TO <owner>` among them;
   indexes, TOAST tables and owned sequences follow their table. Skip the
   members of an extension (`pg_depend` `deptype` `'e'`): the `ltree`
   extension stays the superuser's with its members, since PostgreSQL has no
   `ALTER EXTENSION … OWNER TO`, and on such an install that superuser runs
   any `ALTER EXTENSION ltree UPDATE`, outside the migrations.
5. **Set the owner URL** beside the serving one:
   `OML_POSTGRES__OWNER_URL` (`postgres.owner_url`) logs in as the owner,
   and `OML_POSTGRES__URL` (`postgres.url`) as the serving role, both on the
   same database.
6. **Start v0.3.1.** The lead migrates under the owner URL, whose migration
   `0007_role_split.sql` grants the serving role its privileges, then serves.
   The split succeeded when its log carries `event` `role_split_booted` with
   both role names, `owner_role` and `serving_role`, and `/readyz` answers
   200; a refusal names its reason with `postgres.role_split` before any
   listener binds (d5).

Until v0.3.1 starts, rollback reverses the move (`REASSIGN OWNED BY <owner>
TO <role>`, or the same `ALTER … OWNER TO` back) and restarts the old build;
after it starts, the way back is the backup and [restore](restore.md). The
live host's split is not run; it waits for the maintainer, and its output is
recorded in ADR-0034 d7 with the date.

## Server, CLI and plugin together (the upgrade to v0.4.0)

*Available since v0.4.0 · [releases](../product/releases.md)*

v0.4.0 rewrites the agent and HTTP surfaces and leaves the data alone: keys, grants, stored
memories and the journal do not change
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d13). The schema only grows: two
forward-only migrations, `0012_connections` and `0013_oauth_clients`, add `api_keys.credential`,
with every existing key folded as `bearer`, make `api_keys.hash` nullable for connector keys,
and add the five connection tables, each under `FORCE ROW LEVEL SECURITY` and granted to the
serving role in 0007's form
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d7, d8, d10). The lead runs
them at start under `postgres.owner_url`, as every migration since v0.3.1, so the upgrade needs
no step of its own for them. Its clients change with it,
so the server, the `oml` CLI and the Claude Code plugin move in one window; the
[changelog](../changelog.md) lists every break with its migration.

1. **Back up**, as step 1 of the upgrade order requires.
2. **Upgrade the server** in the order above.
3. **Install the v0.4.0 CLI** on every host that runs `oml`, the session-start hook's host
   included. The v0.3 CLI sends request spellings v0.4.0 refuses, each refusal naming the new
   one. The CLI starts from `--url` and `--api-key`, then `OML_URL` and `OML_API_KEY`, then its
   credentials file; it read a v0.3 file's `server` and `key` through v0.5.0 and reads neither
   since v0.5.1, and `oml login <url>` rewrites the file with `url` and `api_key` once
   ([CLI reference](../cli/reference.md#start-settings)).
4. **Update and configure the plugin**: `claude plugin update oml@open-memory-lab`, then set
   its `url` and `api_key` with `/plugin configure oml@open-memory-lab` or the
   `claude plugin configure --values-stdin` pipeline
   ([ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md)). The plugin serves no tools
   until both are set; exported variables alone no longer reach it.
5. **Replace the permission rules.** A `permissions.allow` rule naming a v0.3 tool allows
   nothing. Allow `mcp__plugin_oml_oml__oml_read` and `mcp__plugin_oml_oml__oml_workarea`, or
   `mcp__plugin_oml_oml` for all three tools, the write included
   ([plugin README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md)).
6. **Check** with `oml doctor`, which names each setting's source and the key the server sees,
   and in a Claude Code session, where `oml_write`, `oml_read` and `oml_workarea` are the
   plugin's tools.
7. **Turn on connections**, only for Claude's hosted apps: set `OML_OAUTH__ISSUER` to the
   public origin, pass `/.well-known/oauth-*` and `/oauth/*` through the proxy, and keep the
   `Location` header of `POST /oauth/authorize` out of its access log
   ([deployment](deployment.md#connections-behind-a-reverse-proxy)). Unset, connections stay
   off and nothing else changes.

A raw MCP client or a script that still called a v0.3 tool name or CLI verb kept working
through v0.5.0, hidden, and each MCP call logged `mcp_alias_called` with the calling key's
`key_id` (ADR-0039 d12). v0.5.1 removed the old names: such a call is now JSON-RPC -32602
`Unknown tool`, logged as `mcp_call` with `tool` `unknown` under the calling key's `key_id`,
and an old CLI verb or flag is a usage error, exit `2`. Rolling back means the v0.3
server, CLI and plugin together; the two migrations stay, as every migration does, and no
v0.3 binary has been run on the v0.4.0 schema yet.

**The maintainer's deployment** ran these steps on 2026-11-04: the server, the CLI and the
plugin together, with the plugin configured and `oauth.issuer` set. The same day it moved from
`oml.doublegate.io` to `memory.doublegate.io`, with no redirect from the old name: the issuer moved
with it, and the clients were set to the new URL.

## Upgrading from v0.5.1 or earlier (the upgrade to v0.5.4)

*Available since v0.5.4 · [releases](../product/releases.md)*

**Install v0.5.4, not v0.5.3, over a database a v0.5.1 or earlier server ran.** v0.5.3 refuses
to start on a database whose `universe` root stores the status `development`:

```text
fatal: migrate on boot: while executing migration 17: error returned from database:
check constraint "workareas_universe_committed" of relation "workareas" is violated by some row
```

Migration `0017_workarea_transitions` adds `workareas_universe_committed`, which holds the root on
`committed`, and its header says every root was already stored that way. That was true from
v0.2.0 on and false before: v0.1.0 stored every node `development`, its root included, and no
later build read the column back, because through v0.5.1 a workarea's status was derived from
its `parent_id` (the root `committed`, every other node `development`). A deployment created
by v0.1.0, the maintainer's among them, still holds that root, and its API served `committed`
throughout. The failed start changes nothing: sqlx runs 0017 in one transaction, the failure
rolls it back, and the database stays at 0016, where a v0.5.1 server serves it again.

v0.5.4 stores, before 0017 runs, each workarea's status as v0.5.1 served it. The lead's boot,
as the owner role, checks that `_sqlx_migrations` records no version 17 or later; if so, it
enumerates the tenants under 0003's startup policy and, under each tenant's row-level-security
context, sets `status` to the derived value on every row whose stored value differs, all in one
transaction, then runs the migrations. It logs `stored the status v0.5.1 served on workareas whose
column disagreed, before 0017` with the count when it changed a row. A database past 0017, or a
fresh one, is left alone, and a second boot changes nothing. No step is the operator's: back
up, as step 1 of the upgrade order requires, and start v0.5.4.

**Why not edit 0017.** sqlx stores each applied migration's checksum and refuses to start when a
file's bytes change. A database created by v0.5.3 after its release, 2026-11-04, applied 0017
as released; an edited 0017 would make every such database refuse to boot, and repairing it
would need a hand-written `UPDATE _sqlx_migrations`, which is direct SQL on the owner's tables.
No migration number is free between 0016 and 0017, and sqlx runs every pending file in order, so
a correction cannot be a new file that runs first. Keeping 0017's bytes and running the
alignment in code before the migrator costs one function in the store, which applies only while
0017 is pending; its header's sentence about earlier roots stays wrong, and
[postgres-schema](../data/postgres-schema.md#workareas-and-edges) records the correction.

**Why nothing is journalled.** The journal records what a workarea read served, and that does
not change: every read before v0.5.2 served the derived status, and the alignment stores that same
value. No `workarea.created` event carries a status a later fold would contradict, and the chain
verifies as before.

**The other constraints, checked.** Each migration since v0.5.0 that adds a constraint over rows
already held validates against every state an earlier build could store:

| Migration | Constraint over existing rows | Why it holds | Test over earlier rows |
|---|---|---|---|
| `0014_user_roots` | `workareas_user_root_shape`, `workareas_user_name`, `grants_node_has_no_deny`, the shared-name index | `user_root` defaults to `false` and `namespace_root_id` to null; `covers` defaults to `subtree`; the shared-name index narrows 0006's tenant-wide unique name | `a_key_issued_before_users_folds_as_a_tenant_key` and the PoC migration test |
| `0015_web` | `api_keys_credential_check` widened, `api_keys_session_user` | every earlier key is `bearer` or `connection` (0012) with a null `session_for`; `web_emails` is new in the same file | the same two tests, and `a_migrated_v0_3_database_keeps_every_key_as_a_bearer_key` |
| `0016_guardrails` | `memories_guardrail_fields_check`, `guardrail_scope` | no earlier build accepted the kind `guardrail` (`MemoryKind::parse` refused it), and `severity` and `rule` are new and null | the PoC migration test, whose memories cross 0016 |
| `0017_workarea_transitions` | `workareas_universe_committed` | held only after the alignment above | `a_v0_5_1_database_whose_root_is_stored_development_upgrades_through_0017_serving_the_same_statuses` and the PoC migration test |
| `0018_claim_edges`, `0019_brief_edges` | none on an existing table | each creates its own table | — |

The PoC migration test is `a_migrated_poc_database_keeps_every_row_and_every_journal_event`; since
v0.5.4 its fixture stores every root `development`, as v0.1.0 did.

## Version skew: one minor, as policy

During a rolling update the deployment may hold **two adjacent minor
versions** — N and N+1 — and no more. This is a stated policy, not a measured
bound: it is exactly the skew the upgrade order above creates, and the
expand-contract rule is scoped to it. Consequences:

- A follower at N+1 forwarding writes to a lead at N (step 4 before step 5)
  is inside the policy; the internal forward channel is versioned with the
  API and additive within a minor.
- Skipping versions (N → N+2 binaries in one roll) is outside the policy:
  migrations apply in order and each expects the previous minor's binaries at
  most. Upgrade through N+1.
- A skew held longer than one upgrade window — half a fleet parked on N for
  a week — is outside the policy's intent even though nothing enforces it
  yet; the conformance suite that would ([FR-22](../product/roadmap.md))
  tests profiles, and a skew case belongs in it when upgrades become real.

## Rollback: binaries only, never schema

Rolling back means **re-deploying version N binaries against the N+1
schema** — permitted and expected to work, because the expand-contract rule
made the N+1 schema readable by N binaries. The sequence is the upgrade order
reversed: lead first this time (take the write-refusal window immediately,
while the incident is already open), then followers, then the asynchronous
workloads.

What rollback never does is move the schema: there are no down migrations,
deliberately. If the N+1 *data* is the problem — a migration corrupted rows,
a defective binary wrote bad state — binaries cannot fix it and the answer is
the pre-upgrade backup through the full [restore](restore.md) ceremony, with
everything that costs ([disaster recovery](disaster-recovery.md) states the
loss structurally). That cost is the reason step 1 is a requirement.

## What this page does not decide

Numeric upgrade cadences, maintenance windows, and how long two minors may
coexist in hours are deployment-site choices. The measured size of the lead's
write-refusal window is [FR-24](../product/roadmap.md); proof that the three
profiles upgrade as one product belongs to the same conformance effort as
[FR-22](../product/roadmap.md); the standalone migration verb is a
roadmap-tracked CLI gap.
