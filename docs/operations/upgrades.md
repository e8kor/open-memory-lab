<!--
id: operations/upgrades
tier: engineering
status: design draft for review · decided, not built
answers: who runs migrations and when; in what order the binaries move; how much version skew a rolling update may hold; what rollback does and does not mean
-->

# Upgrades

**Status:** design draft for review · decided, not built — no upgrade has ever
run, because no version has ever shipped; this spec is `0.3.0-design`
([openapi.yaml](../api/openapi.yaml)) and the page commits the shape of every
future N → N+1.

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
