<!--
id: operations/runbooks
tier: engineering
status: design draft for review · decided, not built
answers: how to diagnose and act on each named operational condition — what it means, the doctor fields and metrics to read, the decision tree, and the evidence to capture first
-->

# Runbooks

**Status:** design draft for review · decided, not built — every command below
is a designed verb that has never run, and every metric is a declared name no
binary emits yet.

This is the one runbook page. `oml doctor` names its next action as a command
or an anchor on this page ([CLI reference](../cli/reference.md)); the alerting
table in [observability](observability.md) links each condition here; nothing
runbook-shaped lives anywhere else. Each condition section carries the same
four parts: what the condition **means**, how to **diagnose** it (doctor
fields and metrics by their exact names), the **decision** tree, and the
**evidence** to capture before acting — captured first, because by the time a
condition is resolved the state that explained it is usually gone.

Two standing rules apply to every section. First, **no store is ever edited
by hand**: every repair travels a journalled channel
([reconciliation](../architecture/reconciliation.md)), and a condition whose
fix seems to need a manual table edit is a design gap to raise, not a
procedure to improvise. Second, the plaintext rule binds evidence capture as
it binds logs ([observability](observability.md)): evidence is ids, digests,
seqs and counts — never body content.

## Fork check failing

**Meaning.** `/readyz` is red on every `oml-server` replica after a restart:
the stored chain head did not verify against the last signed head. A failover
or restore lost committed tail events, and the deployment is refusing writes
rather than forking the chain — staying down is the designed behavior, and
there is no accept-and-fork override ([deployment — probes](deployment.md)).
Requests are refused as `system.unavailable` (503, retryable)
([errors](../api/errors.md)).

**Diagnose.** `oml doctor --json`: the `chain_head` field per tenant (head
seq, fork-check result) and `lead_lease`. `oml audit verify-chain` against
the live server names the last seq that verifies. Correlate:
`oml_lead_takeovers_total` (did a takeover precede this),
the scaled-profile doctor check (were the quorum-synchronous settings
actually in force), and whether anyone restored Postgres.

**Decide.**

- A restore is in progress or just happened → this is step 7 of the
  [restore ceremony](restore.md); continue that ceremony, not this runbook.
- An unplanned Postgres failover preceded it → the standby that won was
  behind the signed head: acknowledged writes are gone. Treat as a
  recovery-point event ([disaster recovery](disaster-recovery.md)); verify
  the quorum settings that should have made this impossible before trusting
  the store again.
- Neither → suspect the stored head itself (corruption or tampering);
  capture evidence and treat under the security posture
  ([security](../architecture/security.md)) before any recovery act.
- Never: force readiness, re-point at a different Postgres mid-diagnosis, or
  delete the chain-head row. All three convert a loud failure into a silent
  fork.

**Evidence.** Doctor JSON; the last verifying (tenant, seq, hash) from
`verify-chain`; the Postgres failover timeline from the store's own logs; the
most recent retained evidence export's verified head for comparison.

## Review unavailable

**Meaning.** `review_availability: false` for a workarea: its review policy
cannot currently be satisfied — no eligible, unexpired, non-author
dreamer-class key is in scope. Admission fails closed: every write to that
workarea holds until `held_item_expiry` and then dead-letters
([admission](../architecture/admission.md)).

**Diagnose.** `oml doctor` per workarea;
`oml keys list --role dreamer --expiring <duration>` for expiries;
`oml grants list --workarea <workarea>` (the node's id or name) for scope;
`oml workareas show <workarea>` for the policy in force. Two named variants
([CLI reference](../cli/reference.md)): a vocabulary workarea with no active
dreamer grant (flagged orphaned), and a policy naming
`human_required_classes` with no attested-human dreamer key in scope.

**Decide.**

- Keys expired → `oml keys rotate <key> <new-key-name>` or issue fresh dreamer keys
  (`oml keys issue <key-name> --role dreamer --grant <workarea>:dreamer`).
- Keys exist but lack scope → `oml grants add <key> <workarea>:dreamer`.
- Orphaned vocabulary workarea → the named next action is propose freeze
  (`oml workareas status propose`), not a synthetic reviewer.
- The unavailability is intended (a crew wound down) → this is
  [tenant offboarding](#tenant-offboarding) or a deliberate freeze; make it
  journalled rather than letting writes dead-letter by neglect.
- Never: grant the admin key review capability to unblock a queue — refused
  by `access.role_exclusive` anyway (admin ≠ reviewer), and the refusal is
  the constitution working.

**Evidence.** The doctor field, the key and grant listings, the policy
snapshot — enough to answer later why the workarea went dark and for how
long.

## Oldest held approaching expiry

**Meaning.** `oml_review_queue_oldest_age_seconds` is closing on the
policy's `held_item_expiry`: items are about to dead-letter not because they
are poison but because review capacity lagged arrivals — the failure mode
[FR-2](../product/roadmap.md) exists to quantify, arriving before FR-2 has
numbers.

**Diagnose.** `oml doctor`: `queue_depth_per_step` and `oldest_held_age` vs
`held_item_expiry`. `oml review queue --workarea <subtree>` for per-workarea
depth and ages; the drain rate is `oml_review_verdicts_total` over the same
window; `review_availability` to rule out the harder condition above.

**Decide.**

- Reviewers available but outpaced → add review capacity: more dreamer keys
  or crews on the workarea; the ratio of arrival to drain is the number to
  watch, not the depth.
- Review unavailable → that is [its own runbook](#review-unavailable); fix
  eligibility first, capacity second.
- One source is flooding → `oml bans add --source <selector>` (the default
  `--dry-run` reach preview, then `--apply`) is the admission-side valve.
- The expiry is simply too tight for an honest crew → the policy change is a
  tenant act, journalled with its snapshot; expiry extension to absorb a
  permanent capacity shortfall is a decision to make visibly, not a dial to
  quietly turn.

**Evidence.** Queue ages and depths per workarea, the verdict rate over the
window, the policy's expiry value — the FR-2-shaped numbers this incident
produced for free.

## Dead letters present

**Meaning.** `oml_dead_letter_count > 0` (or doctor's `dead_letter_count`):
the lake-writer abandoned rows after their retry budget. Each is already a
journalled materialize-skipped event and a standing `dead-lettered`
reconciliation finding — nothing is lost silently; something is waiting for a
human decision ([reconciliation](../architecture/reconciliation.md)).

**Diagnose.** Doctor's `dead_letter_count` with oldest age;
`oml_dead_letter_moves_total` for the arrival rate; the lake-writer's log
lines joined by journal `event_id` for the per-row failure;
`oml_outbox_frontier_lag_seconds` to see whether the queue around the dead
letters is otherwise healthy.

**Decide.**

- The failure was transient (object-store outage spanning the retry budget)
  → re-enqueue. The re-enqueue is a journalled act through the repair
  channel; no operator verb fronts it yet — roadmap-tracked
  ([roadmap](../product/roadmap.md)) — so today it is requested through the
  reconciler's repair path, never by editing the dead-letter table.
- The failure reproduces (a poison payload) → diagnose before re-enqueueing;
  a re-enqueued poison row just dead-letters again and burns a second retry
  budget. A payload the writer can never apply is a software defect with a
  journalled trail.
- The count is growing → the writer is failing broadly, not per-row; treat
  as [frontier lag](#frontier-lag-growing).

**Evidence.** The dead-lettered (artifact_id, version) set, each row's
journalled skip event id, the per-row error class from logs, oldest age.

## Frontier lag growing

**Meaning.** `oml_outbox_frontier_lag_seconds` rising monotonically: the lake
is falling behind the journal. Brief lag is micro-batching and failover
working as designed; monotonic growth means the lake-writer is down,
conflicted, or poisoned. Nothing client-facing degrades — the lake serves no
client read — but reconciliation, lineage analytics and the erasure vacuum
cadence all age with the frontier.

**Diagnose.** Doctor's `outbox_frontier_lag` (events and seconds, per
tenant) and `lease_holder`; `oml_delta_txn_conflicts_total` (second fence
firing), `oml_dead_letter_moves_total` (poison), `oml_lake_batch_duration_seconds`
(slow but moving), and the lake-writer's own `/readyz` — its conditional-put
self-test catches an object store changed under the pin
([deployment](deployment.md)).

**Decide.**

- No lease holder → the supervisor layer failed (CronJob suspended, compose
  service exited, k8s lease stuck); restart the writer and watch the lag
  drain.
- Readiness failing on the conditional-put self-test → the store moved under
  the pin: a MinIO image outside the declared pin, or an NFS/SMB data
  directory in standalone. Restore the pinned backing; do not disable the
  self-test — it is the fence's early warning.
- Txn conflicts climbing → two writers are alive; go to
  [txn conflicts](#txn-conflicts-in-steady-state).
- Batches completing but slow → capacity, and Delta commit cost grows with
  version-history depth; check `oml_lake_maintenance_duration_seconds` —
  overdue OPTIMIZE/checkpoint work is the usual suspect. Throughput numbers
  are unmeasured, tracked as [FR-16](../product/roadmap.md).
- A `stale-frontier` finding with a healthy writer → the cached frontier
  column is behind, not the lake; the reconciler corrects it
  ([observability — doctor](observability.md)).

**Evidence.** Lag in both units per tenant over time, lease transitions,
self-test result, batch duration trend, maintenance-run recency.

## Txn conflicts in steady state

**Meaning.** `oml_delta_txn_conflicts_total` increasing outside failover and
deploy windows: two live lake-writers, and the second fence — conditional put
plus transaction-id skip — is absorbing what the lease should have prevented
([deployment — the two fences](deployment.md)). Correctness is not at risk;
that is the fence's whole job. The urgency is lag and wasted work, plus the
question of *why* two writers exist.

**Diagnose.** `oml_leader_lease_transitions_total` (is the lease bouncing),
doctor's `lease_holder`, then a process inventory per profile: a second
supervisor on the standalone host, a duplicated compose project, two pods
alive across a node partition the API server has not resolved.

**Decide.**

- Find and stop the second writer; the conflicts stop with it.
- The lease is churning → [lease churn](#lease-churn) is the root condition.
- Conflicts without a findable second writer → suspect clock or lease-expiry
  misconfiguration; the lease is an optimization and may be mistuned, but
  the fence holding is why this is a ticket and not a data incident.

**Evidence.** Conflict counts per table over time, lease transition
timeline, the process/pod inventory with start times.

## Lease churn

**Meaning.** `oml_leader_lease_transitions_total` rate is climbing: the
lake-writer lease keeps changing hands. Churn predicts frontier-lag spikes
(each handover is a drain pause) and usually means node or kubelet trouble,
not OML trouble.

**Diagnose.** Transition timestamps correlated with node events (evictions,
memory pressure, restarts), the writer's restart count, and
`oml_lake_batch_duration_seconds` — a writer whose batches outlive its lease
renewals loses the lease honestly.

**Decide.**

- Node-level cause → fix placement, resources, or the failing node; the
  lease is reporting infrastructure weather.
- Batches outliving renewals → the lease TTL and batch sizing are mistuned
  relative to each other; both are unmeasured profile defaults, revised on
  evidence, never tightened blind.
- Churn with txn conflicts → two writers; see
  [txn conflicts](#txn-conflicts-in-steady-state).

**Evidence.** Transition timeline against node events; batch duration
distribution; the lease TTL in effect (`oml settings show --effective`).

## Lead churn

**Meaning.** `oml_lead_takeovers_total` climbing in steady state: the lead
keeps losing its lease, and **every takeover is a write-refusal window** —
writes refuse as `system.unavailable` until a lead-eligible node takes the
lease and passes the fork check ([deployment — the lead lease](deployment.md)).
Nonzero during failover or rolling updates is the lease working; a climbing
steady-state rate is this condition. The window's size is unmeasured
([FR-24](../product/roadmap.md)).

**Diagnose.** `oml_lead_lease_age_seconds` (a sawtooth is churn), doctor's
`lead_lease` (holder, address, epoch — a climbing epoch counts fights), the
journalled `system.lead_acquired` events for the takeover record, and the
client-visible 503 rate on write routes over the same window. The lease row
lives in Postgres, so check Postgres health before anything else: heartbeat
writes failing is the most common cause of a lost lease.

**Decide.**

- Postgres slow or contended → the lease is a casualty, not the disease;
  treat the store first.
- A rolling update in flight → expected; verify it converges and the rate
  returns to zero ([upgrades](upgrades.md)).
- Two lead-eligible nodes trading the lease (epoch climbing fast) → a
  supervisor misconfiguration is running more lead-eligible processes than
  intended; the chain lock means they cannot fork, only contend — find and
  stop the extra.
- Writes failing sustained → page-level; consider holding the fleet still
  (no restarts) while diagnosing, since every restart is another window.

**Evidence.** Lease epochs/holders over time, takeover event ids, write-path
503 rate, Postgres latency over the window.

## RLS denials

**Meaning.** `oml_rls_denied_total > 0`. The expected value is zero in
perpetuity: the service roles' statements are written to pass their own RLS
policies, so any denial is a service bug or a tenant-isolation probe —
and both deserve attention ([security](../architecture/security.md)).

**Diagnose.** The `service_role` label says which workload; log lines joined
by `request_id` give the denied statement's context (ids and digests only —
the plaintext rule). Correlate with deploy times: a denial burst that starts
at a rollout is a bug in the new binary.

**Decide.**

- Correlates with a deploy → treat as a defect in the new version: roll the
  binaries back ([upgrades](upgrades.md)) and file it; the denials were RLS
  doing its job against a buggy query.
- No deploy correlation → treat as a probe: identify the key behind the
  requests (`oml keys show <key>`), and revoke on reasonable suspicion
  (`oml keys revoke <key> --reason …`) — revocation is journalled and
  reversible by reissue; a tenant-isolation breach is neither.
- A spike → page. Zero-to-many is categorically different from
  zero-to-one.

**Evidence.** Denial counts per role over time, the request ids and key id
involved, the deploy timeline — captured before any revocation, so the
export can tell the story in order.

## Repair halted

**Meaning.** `oml_reconcile_repair_halts > 0`: a reconciliation finding
recurred past its attempt budget and auto-repair stopped **for that
artifact, on purpose** — a finding that survives N honest repairs means the
fold, the writer, or the comparison itself is wrong, and looping would
convert a defect into unbounded journalled churn
([reconciliation](../architecture/reconciliation.md)).

**Diagnose.** Doctor's `reconciler` field: open findings by class, the
halted artifact id, attempt count. The finding names its class (the closed
seven-word vocabulary) and the pinned Delta version it was observed at —
re-read that exact snapshot, not the live table.

**Decide, by finding class.**

- `digest-mismatch` → stored bytes disagree with the signed ciphertext
  digest. Possible tampering: run `oml audit verify-chain` for the tenant,
  capture an evidence export, and move under the security posture before any
  repair.
- `state-mismatch`, `duplicate`, `missing`, `orphan` recurring → a writer or
  fold defect; the halt stays until the software is fixed. The halt clears
  itself when the finding stops recurring — there is no "clear halt" verb,
  deliberately.
- Any class → never reconcile by hand-editing the lake or the fold tables;
  the journal is the only fold source, and a hand edit creates the next
  finding.

**Evidence.** The finding identity (class + comparison key), attempt count,
the pinned Delta version, the chain-verification result, the repair events
already journalled for this artifact.

## Reconciler stale

**Meaning.** `oml_reconcile_last_success_timestamp_seconds` is older than a
few schedules. `concurrencyPolicy: Forbid` (and the `--every` loop's
sequential-by-construction equivalent) means slow or stuck passes queue up
silently — staleness is how that surfaces ([deployment](deployment.md)).
While stale, drift detection is paused: the deployment is not more broken,
but it is blinder.

**Diagnose.** `oml_reconcile_pass_duration_seconds` against the configured
deadline; whether the scheduler is alive (CronJob status, compose service,
supervisor child); `oml_reconcile_cursor_lag_events` for how far behind the
cursor is; the reconciler's logs for a cursor that no longer verifies — which
is itself a finding, not a starting point.

**Decide.**

- Passes hitting the deadline → a killed pass redoes work, never corrupts
  it, but it also never finishes: raise the deadline (an unmeasured profile
  default) or investigate what grew — journal size growth is
  [FR-18](../product/roadmap.md)'s territory.
- Scheduler dead → restart it; the cursor's journalled checkpoints mean the
  next pass resumes, not restarts.
- Cursor fails verification → stop; that is a `repair-halted`-grade event:
  the persisted cursor disagrees with the journal. Capture evidence and
  treat under [repair halted](#repair-halted)'s digest-mismatch arm.

**Evidence.** Last success time, pass duration distribution, the deadline
value, cursor seq and running hash.

## Backup failed

**Meaning.** The store-level backup success signal failed
([observability — store-level signals](observability.md)). A missed backup
stretches no erasure window — but a **changed retention configuration or a
failed expiry does**, and a broken backup chain silently converts the
recovery story to "the composition since the last *good* backup"
([disaster recovery](disaster-recovery.md)).

**Diagnose.** The backup machinery's own signals — this is the one condition
whose diagnosis is mostly outside OML. Establish: the last verified-good base
backup, WAL-archive continuity since it, and whether retention *configuration*
changed (the hazard that touches erasure, not just recovery).

**Decide.**

- Transient job failure → fix and re-run; the RPO composition aged, nothing
  else.
- WAL archive gap → point-in-time recovery across the gap is gone; the next
  base backup is now urgent, and the gap belongs in the operator's
  declaration.
- Retention configuration changed → re-derive the erasure-SLO terms it
  composes ([deployment — backups](deployment.md)): a lengthened retention
  window lengthens how long erased derivatives survive in backups, and the
  evidence export's stated windows must follow. This is the correctness
  consequence hiding in an ops ticket.
- In `kms-file` profiles, check the keyring backup path with the same
  seriousness — it is a usable-key copy class
  ([audit and lineage](../architecture/audit-and-lineage.md#the-erasure-inventory)).

**Evidence.** Last good backup id and time, WAL continuity record, the
retention configuration diff, which copy classes the change touches.

## Sustained 429s

**Meaning.** `oml_quota_rejections_total` rising steadily for one tenant:
either the tenant outgrew its journalled quota grant, or the per-replica
token-bucket approximation is misbehaving under replica churn — each replica
enforces limit/N on the read path, so churn moves the effective limit
([FR-8](../product/roadmap.md), [commitments](../product/commitments.md)).

**Diagnose.** The rejection rate per tenant; which path (write-path limits
are exact — one lead, nothing to divide; read-path limits divide across
followers); follower replica count over the window; the effective quota via
`oml settings show --effective`.

**Decide.**

- Legitimate growth → raise the quota: a journalled settings act
  (`oml settings validate`, then `oml settings apply`) — never an in-place
  edit.
- Replica churn artifact → stabilize the follower count; the enforcement
  error is FR-8's open bound, documented rather than hidden.
- Abusive traffic → the admission-side valve is `oml bans add` with its
  reach preview; quota is for budgeting, bans are for refusal.

**Evidence.** Rejection rate per tenant, effective quota value, replica
count timeline, the path (read vs write) carrying the 429s.

---

The four sections below are **ceremonies**: planned multi-step operator acts,
not alert responses. Each exists on this page so that the first time it runs
is not the first time anyone thinks it through.

## Signing-key compromise

Compromise of the lead is compromise of the deployment signing key
([security](../architecture/security.md)): the holder can sign arbitrary
events as the deployment for as long as the compromise lasts, and nothing in
the chain distinguishes its signatures from honest ones. The ceremony bounds
the damage; it cannot undo it.

1. **Stop the write path.** Take the lead down (scale the lead Deployment to
   zero / stop the service / stop the supervisor). Writes refuse everywhere;
   reads keep flowing wherever followers exist. Every hour the key keeps
   signing is chain history that cannot be trusted.
2. **Fix custody.** Rebuild the lead's host or pod from a trusted image,
   rotate every credential the lead held (Postgres role, KMS grants,
   object-store credentials) — the lead also holds the KMS decrypt grant, so
   the compromise was decrypt-everything as well as sign-everything, and the
   security page says so without flinching.
3. **Rotate the signing key.** Mint a new deployment signing key and replace
   the old at its custody point (the Kubernetes Secret, or the key material
   the standalone/docker payload mounts). The rotation is journalled as
   `system.signing_key_rotated` ([event catalog](../data/event-catalog.md))
   — written at the new lead's startup, naming the retiring key and carrying
   the successor's public key, so the chain itself records where one signing
   era ends and the next begins. No CLI verb performs the material swap; it
   is a deployment-payload act, and the journalled event is the system's.
4. **Fix the trust boundary in time.** Events signed before the compromise
   window are trustworthy exactly as far as externally retained chain heads
   can corroborate them ([RS-9](../product/roadmap.md) — until it closes,
   retained evidence exports are what exists). Take `oml audit export` now,
   retain it outside the deployment: the first externally held head of the
   new era.
5. **State the honest residue.** Rotation does not heal the window: anything
   the attacker signed verifies forever under the old key. The journalled
   rotation event plus the last pre-compromise retained head bound the span
   that needs review; everything inside that span is suspect until examined.

**Evidence.** The compromise window's time bounds, the rotation event id,
the pre- and post-rotation exported heads, the credential-rotation record.

## KEK rotation

Two distinct acts share this ceremony: the **scheduled rotation** that mints
a new KEK, and the later **retired-KEK destruction** that makes old backups'
wrapped-DEK copies permanently useless. The pair is what bounds the erasure
SLO's wrapped-DEK term at backup retention + KEK rotation period
([audit and lineage](../architecture/audit-and-lineage.md#the-erasure-inventory)).

**Planned rotation.**

1. Mint the successor KEK: a KMS key rotation in the kubernetes profile; a
   new KEK minted into the keyring file in the `kms-file` profiles
   ([RS-12](../product/roadmap.md) owns the keyring mechanism).
2. Re-wrap **live DEKs only** under the new KEK — never the copies in
   backups; that is the design, not an omission: backup copies decay by KEK
   retirement, not by rewriting backups. The re-wrap is a journalled act,
   recorded as `system.kek_rotated` ([event catalog](../data/event-catalog.md));
   no CLI verb fronts it yet — roadmap-tracked.
3. The retired KEK **remains** until destruction day: restores of backups
   taken under it still need it ([restore](restore.md) preconditions).

**Retired-KEK destruction.**

1. Verify the horizon: no retained backup — base backups, WAL, volume
   snapshots, *and keyring-file backups in the `kms-file` profiles* —
   predates the KEK's retirement. Destroying early strands restorable
   backups; destroying late stretches the erasure bound. The backup table in
   [deployment](deployment.md) is the checklist.
2. Destroy: schedule key deletion in KMS, or — `kms-file` — delete the key
   file, by the host admin, journalled. Either way the act is recorded as
   `system.kek_destroyed` ([event catalog](../data/event-catalog.md)).
3. State the consequence in the operator's declaration: every wrapped-DEK
   copy in backups older than this KEK is now noise. That is the erasure SLO
   term closing — the point of the ceremony — and it also deliberately
   shortens the restore window to backups wrapped under surviving KEKs.

**Evidence.** The rotation and destruction event ids, the backup-horizon
check that justified the destruction, the re-wrap completion record.

## Tenant offboarding

Winding a tenant down is a sequence of journalled acts ending in a chain that
still proves what happened. Run in this order — each step depends on the
previous one being complete.

1. **Suspend writes.** No dedicated suspension verb exists yet —
   roadmap-tracked ([roadmap](../product/roadmap.md)). Today's composition:
   revoke the tenant's writer and dreamer keys —
   `oml keys revoke --matching <selector> --reason offboarding` (the default
   `--dry-run` reach preview, then `--apply`). Keep the admin key until
   step 4: later steps act under it.
2. **Final evidence export.**
   `oml audit export --out <dir> --include-simulations` —
   the complete decision trail, grants history,
   chain verification and separated simulation record. Retain it outside the
   deployment: after offboarding this bundle is also the tenant's last
   externally held chain head ([RS-9](../product/roadmap.md)).
3. **Erase content.** `oml erase <artifact>` per artifact, `--dry-run` first
   where the inventory should be inspected. No bulk-erase verb exists yet —
   roadmap-tracked — so offboarding a large tenant is an enumerated loop
   today, stated plainly rather than papered over. Each erase is journalled;
   the backup-resident copies decay on the stated windows
   ([audit and lineage](../architecture/audit-and-lineage.md#the-honest-boundary)).
4. **Revoke the remaining keys.** The admin key last
   (`oml keys revoke <key> --reason offboarding`); from here the tenant
   has no acting principals.
5. **Lake prefix disposition.** The tenant's lake and bodies prefixes now
   hold ciphertext whose DEKs are destroyed — noise. Removal is an
   object-store lifecycle or operator act on the stated vacuum/retention
   cadence; it is tidiness, not security — key destruction already did the
   security.
6. **What survives, on purpose.** The tenant's digest-only chain remains in
   the journal and still verifies end to end — the evidence outlives the
   content. Attribution metadata survives too; its erasure is
   [FR-1](../product/roadmap.md), and an offboarding agreement that needs it
   gone is blocked on that row, stated honestly.

**Evidence.** The export bundle, the erase event ids, the revocation events,
the disposition record for the prefixes.

## Erasure drill

A rehearsal that an erase does what the
[inventory](../architecture/audit-and-lineage.md#the-erasure-inventory) says,
run on a canary before a data subject or an auditor makes it urgent. The
drill rehearses [FR-17](../product/roadmap.md)'s end-to-end bound; it does
not produce FR-17's measurement — that requires the published, dated run the
row's exit criterion names.

1. **Plant the canary.** Submit a marked memory with an attachment through
   the ordinary admission path — real review, real promotion — so it reaches
   every copy class: journal digests, object-store ciphertext, lake rows,
   tsvector and pgvector entries, and at least one backup cycle if the drill
   is to test the long windows.
2. **Walk the inventory.** `oml erase <canary-artifact> --dry-run` prints
   the erasure inventory for this artifact — every copy class and its
   disposition, with the retention windows. Check it against the deployed
   reality: are volume snapshots in use but missing from the declared
   windows? Is the keyring backup path accounted for? A divergence found
   here is the drill paying for itself.
3. **Erase.** `oml erase <canary-artifact>`; record the event id.
4. **Verify each copy class at its window boundary.**
    - *Immediately:* index rows gone (removed in the erase transaction —
      recall no longer returns the canary); the `artifact_keys` row shows
      destruction; the chain still verifies
      (`oml audit verify-chain`) — the property the design stakes
      everything on.
    - *Vacuum cadence:* after the next lake maintenance run
      (`oml_lake_maintenance_duration_seconds`, op VACUUM), prior Delta
      versions holding the ciphertext are gone.
    - *Object-store lifecycle:* prior object versions age out under the
      stated rules; confirm the rules exist rather than assuming.
    - *Backup retention boundary:* when the pre-erase backups expire,
      confirm expiry actually deletes.
    - *KEK retirement boundary:* the composed bound's far edge — after the
      next [retired-KEK destruction](#kek-rotation), pre-erase backups'
      wrapped-DEK copies are noise even if the expiry above failed.
5. **Record the drill.** Timestamps per copy class against the declared
   windows. A class that outlived its window is a configuration defect to
   fix and re-drill — and the drill record is what an evidence export's
   stated windows are checked against when an auditor asks.

**Evidence.** The dry-run inventory as printed, the erase event id, the
per-class verification timestamps, divergences found and fixed.
