---
status: Stable
verified: 2026-11-20
answers: "how to upgrade an OML server: how migrations run, the upgrade steps in order, what rollback does and does not mean, the upgrade from a pre-0.3.1 install, the v0.5.4 fix, how to move from v0.3, the v0.8.4 role rename that clients take in the server's window, with no key issued or listed until both run v0.8.4, what v0.8.4's migrations 0063, 0064 and 0066 cost, the admin key links, links and sign-in emails that wait on their person, and the reset of a database that ran a draft of 0063 or 0064, the v0.8.3 rename that clients take before the server, what the v0.8.3 migrations cost and what waits during them, and the storage.url to set before v0.8.3's uploads"
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

v0.8.4 runs three migrations, renames the writer role, folds `oml_withdraw` into `oml_write`, and
makes links and sign-in emails wait on their person.

### Contributor and submitter replace writer: clients upgrade in the server's window

v0.8.4 renames the writer role on every surface in one cut, with no alias
([ADR-0093](../adr/ADR-0093-contributor-and-submitter-replace-writer.md)). A key that recalls and
writes is a `contributor`, a key that writes without recall is a `submitter`, and the grant role
that confers writing is `submitter`. A v0.8.4 server reads `writer` as unknown input: as a role on
HTTP, MCP, the SDKs or the CLI it is `request.validation`, with no `details.renamed_to`, because it
has two successors. `oml keys issue-agent --write-only` is retired; `--role submitter` replaces it.

**Clients upgrade in the same window as the server, and no key is issued or listed until both
run v0.8.4** (ADR-0093 d7 and d8, the operator's decision of 2026-11-24). Recall and write work
across the two versions. Key issuance and key listings do not: a v0.8.4 server refuses a v0.8.3
CLI, plugin or SDK that sends `writer` or `--write-only`, and a v0.8.3 server refuses a v0.8.4
client that sends `contributor` or `submitter`; each issues nothing. The notice to send before
the date:

> OML v0.8.4 renames the writer role. A key that recalls and writes is a **contributor**. A key
> that writes without recall is a **submitter**. The grant that confers writing is `submitter`.
> The server and the CLI, plugins and SDKs upgrade in one window. Recall and write keep working
> while one side runs v0.8.3 and the other v0.8.4. No key is issued or listed until both run
> v0.8.4: a v0.8.4 server refuses a v0.8.3 client's `writer`, and a v0.8.3 server refuses a v0.8.4
> client's `contributor`. Existing writer keys become contributor keys. One issued with
> `--write-only` keeps working, and `oml doctor` names it a contributor without recall. Issue a
> submitter key with `--role submitter` and cut the old one off. Memories, key values, grants and
> names are otherwise unchanged.

### What v0.8.4's migrations cost

The server runs 0063, 0064 and 0066 at boot, in that order, before it serves, as [Upgrade in order](#upgrade-in-order) says. Each waits at most 5 s for a lock. On a v0.8.3 database, which holds 0065 already, 0063 and 0064 run after 0065: sqlx applies every migration the database has not recorded, in number order, so a version lower than the newest applied is expected here and is no error. 0066 runs last.

| Migration | What it does to each tenant | What waits, and for how long |
|---|---|---|
| 0063 | Creates the empty `admin_investigations` table with its policy, its update guard and column grants | Catalog changes only; nothing waits beyond the 5 s lock bound. |
| 0064 | Creates the empty `admin_fixes` table, adds four nullable columns to `memories`, `link_pending_user_id` to `api_keys` and `pending_since` to `web_emails`, all with no default, moves every key's link but the bootstrap admin key's to `link_pending_user_id`, adds the `api_keys_link_guard` insert and update guard, replaces the `api_keys_linked_user` check with a looser one and adds `api_keys_link_pending`, both validated in the same transaction | Every lock 0064 takes is held until it commits. The link move and the two VALIDATE scans of `api_keys`, a row per key, run under the ACCESS EXCLUSIVE lock the check swap takes, so key lookups wait for all three: the fourth review of #498 measured about 400 ms at 1,000,000 keys of which 10,000 are linked. A tenant holds hundreds of keys. `memories` and `web_emails` are locked for catalog changes that rewrite no row, so their reads and sign-ins wait until the commit too. |
| 0066 | Rewrites `api_keys.role` from `writer` to `contributor` and `grants.role` from `writer` to `submitter` under each tenant's row security, and replaces the `api_keys_role_check` and `grants_role_check` checks | Reads two small tables, a row per key and per grant. The upgrade stops the lead, `oml-web` and the lake writer first, so nothing waits on its locks. |

### The upgrade is not rolling

The steps follow ADR-0093 d8 and ADR-0091 d12's runbook:

1. Announce the window with the notice above.
2. Back up and verify that the backup restores ([restore](restore.md)): migration 0066 has no
   reverse.
3. Stop `oml-web`, the lead and the lake writer, and start v0.8.4 of all three from one tag.
   The server runs 0063, 0064 and 0066 as [What v0.8.4's migrations cost](#what-v084s-migrations-cost)
   says.
4. Check `oml doctor`, `oml verify` and migration 0066's law (ADR-0093 d5): no role check names
   `writer`, so no role column can hold it. As the database owner,
   `SELECT conname, pg_get_constraintdef(oid) FROM pg_constraint WHERE conname IN
   ('api_keys_role_check', 'grants_role_check');` lists the two checks, and neither names `writer`.
   Signed events keep `writer` and are read through the former-name map's rule 7; a `writer` the
   map does not cover fails `journal_unmapped_former_name`.
5. In the same window, upgrade the operator's own clients. Only once the server and the
   clients run v0.8.4, issue or list keys, and reissue any key `oml doctor` names under
   `key_contributor_without_recall`: issue a submitter key with
   `oml keys issue-agent <name> --role submitter` and cut the old one off.

### `oml_withdraw` is now `oml_write {withdraw}`

v0.8.4 serves three MCP tools, `oml_write`, `oml_read` and `oml_workspace`
([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md),
amendment of d13 of 2026-11-24). A held write is withdrawn with `oml_write` and
`withdraw: <artifact_id>`, an optional `reason` and an `idempotency_key`; the answer carries
`mode: "withdraw"`. `oml_withdraw` is no longer listed, and a call to it is refused as an
unknown tool (JSON-RPC -32602), with no alias. The HTTP route
`POST /v1/memories/{artifact_id}/withdraw`, `oml withdraw` and the console's **Withdraw** are
unchanged. A client that lists tools at each session start, as the plugin and the claude.ai
connector do, finds the new mode without a change to its tool list; a script or an agent prompt
that names `oml_withdraw` changes before the upgrade, and so do the client's permission rules. A
permission rule that names the old tool, such as a deny or ask rule on
`mcp__plugin_oml_oml__oml_withdraw`, protects nothing after the upgrade: no tool has that name. A
rule that allows `mcp__plugin_oml_oml__oml_write` without a prompt, or `mcp__plugin_oml_oml` for
all three tools, now lets withdraw through without one too. To keep a prompt before a withdrawal,
put `oml_write` under an ask rule. The notice to send with the role-rename notice:

> OML v0.8.4 folds the `oml_withdraw` MCP tool into `oml_write`. Withdraw a held write with
> `oml_write` and `withdraw` set to its artifact id, with an optional `reason` and an
> `idempotency_key`. After the upgrade a call to `oml_withdraw` is refused as an unknown tool.
> A permission rule that names `mcp__plugin_oml_oml__oml_withdraw`, a deny or ask rule included,
> protects nothing, and a rule that allows `oml_write` without a prompt now lets withdraw through
> without one too. The HTTP route, `oml withdraw` and the console's Withdraw are unchanged.

### Admin fixes, links and sign-in emails

**Link the admin key before a fix, and have its person accept the link.** From v0.8.4 an admin fix (ADR-0089) is taken only by an admin key that acts for its person; one with no link, or with a link its person has not accepted, is refused `admin.fix_requires_person` with `details.reason: unlinked`. A new server links its first admin key at setup: `oml bootstrap claim <key-name> --token-stdin --tenant-name <tenant> --principal <name> --person <name> --email <sign-in email>` creates that person's user root, links the key and records the email as the person's accepted admin sign-in, all at once because that person runs the setup. An upgraded server's admin keys are unlinked: a console admin links each on the admin Keys page (`/web/admin/keys`): show the person, then name the key in the **Link key** form; the person is one other than the console admin's own, since a self-link is refused. The link then waits until that person signs in and chooses **Accept link** on their own Keys page (`/web/keys`); any key's link waits the same way (ADR-0085 d16). A console admin session key acts for its own person and needs no link. `oml doctor` (admin key) warns `key.link_pending` with the count of links that wait.

**Links made before v0.8.4 wait on their person.** 0064 moves the link of every linked key to pending, except the bootstrap admin key's, the first admin key the setup issued, which no key issues. Before v0.8.4 only a tenant key that is not an admin key could be linked. Each person accepts their links with **Accept link** on their Keys page before the keys act for them again. Production runs v0.8.3, which links keys from the console (ADR-0085 d16), so it may hold links that 0064 moves. From v0.8.4 the database refuses an update of `api_keys.linked_user_id` that is not an unlink, an acceptance of the pending link, or the setup's link of its own admin key while that key is the tenant's only one, and an insert of a key that carries a link or a pending link. It does not guard who sets `link_pending_user_id`, a key's `hash` or `web_emails.pending_since`: the serving role writes those, as [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)'s limits record.

**An address added for an existing person waits on that person.** From v0.8.4 an email an admin adds with `oml web allow`, `POST /v2/web/emails` or the console's allowlist page, for an existing person, or an entry changed to name one, is pending: it signs in to nothing, refused `auth.invalid` with `details.reason: email_pending`, until that person signs in with an address they already use and chooses **Accept sign-in email** on their Keys page (`/web/keys`), where each pending address shows whether it signs in as an admin and the admin key that added it. An existing person is one a sign-in entry or a session key names, or whose user root a key names: a user key issued in it (`oml keys issue --user`), a key linked to it, or a link to it that waits. The person or an admin removes a pending address. A person no key and no entry names takes a first address at once, as before. The person the setup's `--person` names takes the `--email` the setup sends; an address added for them later waits. `oml doctor` (admin key) warns `web.email_pending` with the count. A person whose root holds keys but who has no sign-in email cannot get console access to that root, and this lasts: an address added for them waits on a session they cannot open, and the old root has no console route of its own. Such a person signs in under a new user name, and the keys issued in the old root count as the issuing admin's person (ADR-0089, the operator's decision of 2026-11-20). To restore console access for an admin who lost every provider account, add the new address under a new user name with `--admin`; an address added for the old person would wait on a session they no longer have.

**A database that applied an earlier draft of 0063 or 0064 needs a reset.** Both were edited in place while #498 was open (v0.8.4). A stage or development database that ran a draft of either records the draft's checksum, and the server refuses to start against it with a checksum mismatch for version 63 or 64. Drop and recreate such a database and let the server migrate it from empty; production at v0.8.3 never ran either and needs nothing.

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
