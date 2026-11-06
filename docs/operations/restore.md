---
status: Stable
verified: 2026-11-06
answers: "how the restore ceremony runs and why it is a ceremony \u2014 the erased-content-resurrection and chain-rewind hazards and how each step contains them"
---

# Restore

This page owns the restore ceremony [deployment](deployment.md) defers to it.
The reason a restore is a ceremony and not a routine act is the dual hazard
stated there: **restoring a Postgres backup resurrects erased content
derivatives and rewinds the chain head.** A backup taken before an erase holds
the plaintext-derived index rows and the wrapped DEK that erase destroyed;
a backup taken before the last acknowledged write holds a chain head older
than the one the deployment signed. Both hazards have designed answers — the
erase replay and the fork check below — and both answers run *before* the
deployment serves traffic again, never after. The structural statement of what
each profile can lose, and what recovery costs, is
[disaster recovery](restore.md#disaster-recovery)'s; this page is the procedure.

One scoping rule before any procedure: **a derived-store loss is not a restore
scenario.** The lake, the indexes and the fold tables all refold from the
journal ([reconciliation](https://github.com/e8kor/open-memory-lab/blob/main/design/reconciliation.md)); losing them
costs a repair pass, not a ceremony. A Postgres restore is the answer only
when the **system of record** itself is lost or corrupted, and nothing smaller
justifies the hazards this page manages.

## Preconditions

Before any store is touched, the operator holds:

- **A named backup set.** For base-backup profiles: the base backup plus the
  WAL archive continuous to the chosen recovery point. For a `pg_dump`-based
  standalone deployment: the dump file and the honesty that everything after
  it is gone. The backup mechanisms per profile are the table in
  [deployment — backups](deployment.md).
- **The keyring file copy current at the recovery point** (`kms-file`
  profiles: docker and standalone). A keyring whose retired KEKs were
  destroyed *after* the backup was taken cannot unwrap that backup's DEKs —
  that is the erasure bound working as designed
  ([audit and lineage](../architecture/audit-and-lineage.md)), not a defect,
  and it means the usable keyring copy is part of the backup set, not an
  afterthought.
- **The retained erase record since the recovery point.** Erase events
  committed after the backup are absent from the restored journal; replaying
  them requires a source that survived outside the database — the most recent
  evidence export (`oml audit export`), or the operator's own record of erase
  requests. Where no record survives, the resurrection is bounded only by the
  backup-retention + KEK-rotation decay
  ([the erasure inventory](../architecture/audit-and-lineage.md#the-erasure-inventory)),
  and the ceremony's evidence must say so.
- **The most recent retained chain head.** A restored deployment's chain
  verifies internally at the rewound head — rewinding is invisible from
  inside. Detecting what the restore discarded requires a head retained
  outside the deployment: the last evidence export's verified head
  (#152 tracks a standing mechanism; until it
  closes, exports are what exists).

## The ceremony

The same sequence in every profile; only step 3 differs per profile.

1. **Stop traffic.** Stop every OML binary before Postgres is touched:
   scale the `oml-server` Deployments, the lake-writer, reconciler and
   indexer to zero (kubernetes); `docker compose stop` the OML services
   (docker); stop the supervisor (standalone). Writes must not race the
   restore, and the lake-writer must not drain an outbox that is about to
   rewind.
2. **Record the pre-restore state.** Capture `oml doctor --json` output if
   the deployment can still answer, the current chain-head seqs per tenant,
   and the reason the restore was chosen. This is the evidence that
   distinguishes a restore from tampering later.
3. **Restore Postgres** — per profile, below.
4. **Restore the keyring file first among OML artifacts** (`kms-file`
   profiles). The keyring copy from the backup set is placed at its
   configured path with owner-only permissions **before any OML process
   starts** — `oml-server`'s admission and serving paths unwrap DEKs through
   it, and a lead started against a missing or wrong-generation keyring fails
   its first unwrap rather than its startup, which is the slow way to find
   out. A restored keyring is a usable key copy and re-enters the erasure
   accounting the moment it lands ([deployment — backups](deployment.md)).
   In the KMS profile this step does not exist: the KEK never left KMS and
   was never part of the backup.
5. **Decide the object-store and lake disposition.** The rules:
    - **Bodies and quarantine prefixes** hold ciphertext. If they survived,
      keep them; if they were lost, restore them from whatever backs them up
      — a missing ciphertext object otherwise surfaces as a `missing`
      reconciliation finding against a journal event that still names its
      digest.
    - **The lake is never restored as authority.** Leave the Delta tables as
      found (or empty). A lake *ahead* of the rewound journal holds rows no
      journal event accounts for — `orphan` findings; a lake *behind* it has
      `missing` rows. Both are the reconciler's to repair from the journal
      fold, through the outbox, and restoring lake files by hand would only
      manufacture a store-against-store comparison the design refuses
      ([reconciliation](https://github.com/e8kor/open-memory-lab/blob/main/design/reconciliation.md)).
6. **Run migrations if the binaries moved.** A backup from version N restored
   under version N+1 binaries needs the schema brought forward: performed by
   the bootstrap workload under the owner role, exactly as on any upgrade —
   forward-only, never down ([upgrades](upgrades.md)).
7. **Start the stack and let the fork check run.** Start Postgres-facing
   services, then the lead. The lead's readiness is the startup fork check
   ([deployment — probes](deployment.md)): the stored chain head must verify
   against the last signed head. A clean restore passes it — the rewound
   journal is a consistent, signed prefix. What the pass does **not** prove
   is that nothing was discarded; compare the restored head seq against the
   retained chain head from the preconditions, per tenant, and record the
   gap. The deployment refuses writes until readiness; there is no
   accept-and-fork override, by design.
8. **Replay erases before serving reads.** For every erase in the retained
   record newer than the recovery point: `oml erase <artifact>` — preview
   with `--dry-run` where the inventory should be re-inspected, then apply.
   Each replay is a new journalled erase event on the restored chain that
   re-destroys the resurrected wrapped-DEK row and removes the resurrected
   index rows. In the KMS profile the KMS-side state was never rewound, but
   the restored Postgres rows were — the replay is what reconverges them.
   No replay verb reads an export directly yet; the composition is by hand
   and tracked in the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap).
9. **Full-genesis reconcile before traffic.** Two acts, both named:
   `oml audit verify-chain --genesis` verifies every per-tenant chain from
   the first event; the **full-genesis verification job** — the reconciler
   invoked one-off with a cursor of zero, a named Job in kubernetes and a
   one-off invocation of the same binary in docker and standalone
   ([deployment — the reconciler](deployment.md)) — folds the whole journal
   and compares the stores. Its findings drain through the ordinary repair
   channels. Traffic resumes only when the pass completes and `oml doctor`
   reports no failing field; its runtime at realiztic journal sizes is.
10. **Resume traffic and file the evidence.** Scale the services back up,
    take a fresh `oml audit export`, and retain it outside the deployment —
    the restored era's first externally held head.

## Postgres restore, per profile

### standalone

Host Postgres, host tooling. Either the host's base-backup + WAL machinery
replayed to the recovery point (point-in-time recovery, the host facility's
documented procedure), or `pg_restore` of a logical dump where that is all
the deployment kept — the dump restores a single moment and archives nothing
after it, and the [disaster recovery](restore.md#disaster-recovery) RPO statement is
blunter for it. The object store is the local data directory: if host backups
covered `<data-dir>`, restore `quarantine` and `bodies` (ciphertext) and
leave `lake/` to the disposition rule above. The restored data directory must
still pass the local-POSIX self-test at startup
([deployment — the standalone profile](deployment.md)).

### docker

Stop the OML services, restore the Postgres volume — a volume snapshot, or
WAL replay into a fresh volume using the containerized Postgres's own
tooling — then the MinIO volume if it was lost (ciphertext; same disposition
rules). The keyring volume is step 4, before any OML container starts. The
bootstrap one-shot re-runs for migrations when the images moved; every other
service still gates on its completion
([deployment — the docker profile](deployment.md)).

### kubernetes

The chart does not own the restore; the store's procedure does, by reference:

- **`local`** — CloudNativePG's documented recovery: bootstrap a new cluster
  from the base backup and WAL archive to the recovery point, then point the
  deployment's Postgres URL at it.
- **`scaled`** — the managed service's point-in-time recovery into a new
  instance, then repoint. Before the lead starts, verify the recovered
  instance carries the quorum-synchronous settings the journal domain
  requires — `oml doctor`'s scaled-profile check reports it
  ([observability](observability.md)); a recovery instance provisioned
  without synchronous standbys silently reopens the durability reduction the
  profile exists to close.

## Disaster recovery

What a disaster costs is a composition of mechanisms. Recovery time is the ceremony above end to
end: store restore, keyring placement, migrations where the binaries moved, the startup fork
check, erase replay, the full-genesis verification pass and a clean `oml doctor`. The
full-genesis pass runs before traffic resumes, because serving reads from stores the journal has
not re-verified would trade integrity for a faster-looking recovery. Recovery point is whatever
your backup cadence makes it; the figures to measure are
[](https://github.com/e8kor/open-memory-lab/issues/99) and
[](https://github.com/e8kor/open-memory-lab/issues/117).

| Scenario | What is lost | Recovery |
|---|---|---|
| Single node lost (Kubernetes `scaled`) | no acknowledged write: the quorum has it | the lease moves; watch [lead churn](runbooks.md#lead-churn) and [frontier lag](runbooks.md#frontier-lag-growing) |
| Postgres primary lost, quorum intact (`scaled`) | nothing | managed failover; the fork check at the next lead start verifies the head ([fork check failing](runbooks.md#fork-check-failing)) |
| Host disk lost (standalone, docker, Kubernetes `local`) | everything since the last base backup and archived WAL, or since the last `pg_dump` for a dump-only regimen | the full ceremony above |
| Object store lost, Postgres intact | ciphertext bodies and Delta tables; the journal still names every digest | restore the ciphertext prefixes from their backups; the lake refolds; `missing` findings drain through repair |
| Keyring file lost, no usable copy (`kms-file`) | every body and attachment becomes permanent noise; the journal, chain and attribution still verify | none for content: this is crypto-shredding of the whole deployment, so back the keyring up as carefully as the database |
| KMS KEK destroyed (Kubernetes) | the DEKs it wrapped and nobody re-wrapped decay to noise | none for that content; follow the [KEK rotation](runbooks.md#kek-rotation) ceremony so it never happens unscheduled |
| Total site loss | everything co-located | the ceremony into new infrastructure from **off-site** backups; a backup stored next to what it backs up is no mitigation |

Not recoverable by design: erased content (a destroyed DEK is destroyed everywhere, and the
ceremony re-destroys what backups resurrect), acknowledged writes past the recovery point (the
chain rewinds consistently; only an externally retained head shows the gap,
[](https://github.com/e8kor/open-memory-lab/issues/152)), and content whose KEK is gone.
Losing or rotating the signing key never loses history: signatures verify against the key that
made them ([signing-key compromise](runbooks.md#signing-key-compromise)).

## What a restore cannot do

- **Un-erase.** A destroyed DEK stays destroyed; the replay step re-destroys
  what the backup resurrected. Erasure is senior to recovery, by design.
- **Recover acknowledged writes past the recovery point** in the profiles
  whose RPO is the backup composition ([disaster recovery](restore.md#disaster-recovery)).
  The journal is honest about the rewind only to a reader holding an external
  head — which is why step 10 exists.
- **Prove its own innocence.** A restore and a key-holding attacker's chain
  rewrite look identical from inside the deployment. The pre-restore evidence
  (step 2) and the retained exports are what distinguish them, and the
  standing-anchor question is.

The profile migration path
inherits this ceremony wholesale, and 's
rehearsal is the exit criterion that turns this page from design into
procedure.
