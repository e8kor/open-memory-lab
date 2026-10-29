<!--
id: operations/disaster-recovery
tier: engineering
status: design draft for review · decided, not built
answers: what a disaster can cost per install profile — recovery point and recovery time as compositions of named mechanisms, never as numbers
-->

# Disaster recovery

**Status:** design draft for review · decided, not built — every bound on this
page is structural; no recovery time or loss window has been measured, and
none is stated as a number.

This page states, per install profile, what a disaster can cost: the recovery
point (what is lost) and the recovery time (what getting back costs), both
given as **compositions of named mechanisms rather than numbers**, because no
measurement of OML exists and the [roadmap](../product/roadmap.md) rules
forbid inventing one. The procedure that realizes recovery is
[restore](restore.md); the backup mechanisms and retention windows are the
table in [deployment](deployment.md); the guarantees that set the structure
are [storage](../architecture/storage.md)'s durability domain and
[commitments](../product/commitments.md) §2.

## Recovery point, stated structurally

What an unrecoverable failure of the primary store loses, per profile. The
statement is the composition, never a number: the number is whatever the
operator's backup cadence makes it.

| Profile | Journal and fold tables (Postgres) | Structural statement |
|---|---|---|
| kubernetes `scaled` | **Zero for acknowledged journal writes**, against any single-node loss | The journal domain commits quorum-synchronous (`remote_apply`, quorum standbys — [storage](../architecture/storage.md)); an acknowledged write exists on a quorum before the client hears about it. Losing the whole quorum at once degrades this profile to the backup composition below — which is why the backup chain exists even here. |
| kubernetes `local` | The last base backup + the WAL archived since it | One machine's disk is the durability domain, stated in [deployment](deployment.md); the backup composition is the only floor under it. |
| docker | The last base backup + the WAL archived since it | Same shape; the facility is the host's or container's own ([deployment — backups](deployment.md)). |
| standalone | The last base backup + archived WAL — or, for a dump-only regimen, **the last `pg_dump` and nothing after it** | The composition follows the facility the operator actually runs, and a deployment with no backups has a trivially absent recovery story; that fact belongs in the operator's declaration, not in silence. |

Two stores sit outside the table because their loss is not a recovery-point
event: the **object store** holds ciphertext whose loss surfaces as `missing`
reconciliation findings against a journal that still names every digest, and
the **lake** refolds from the journal entirely
([reconciliation](../architecture/reconciliation.md)). Losing either costs a
repair pass, not history.

## Recovery time, stated as its composition

RTO is the restore ceremony end to end, and it decomposes into exactly the
ceremony's steps ([restore](restore.md)):

> store restore (the backup facility's replay time)
> + keyring placement (`kms-file` profiles)
> + migrations where the binaries moved
> + the startup fork check
> + erase replay over the retained record
> + the full-genesis verification pass
> + `oml doctor` clean

Every term is **unmeasured, tracked as [FR-18](../product/roadmap.md) /
[FR-24](../product/roadmap.md)**: FR-18 owns the full-genesis fold time at
stated journal sizes and the quorum-commit latency; FR-24 owns the fork-check
and takeover windows. Until those rows close, this page commits to the shape
of the sum and refuses to guess its value. The dominant unknown is deliberate:
the full-genesis pass runs *before* traffic resumes, because serving reads
from stores the journal has not re-verified would trade integrity for a
faster-looking recovery — the same trade the fork check refuses on the write
path.

## Scenarios

| Scenario | Profiles exposed | What happens | Recovery |
|---|---|---|---|
| Single node lost | kubernetes `scaled` | The lease moves: lead takeover (write-refusal window, [FR-24](../product/roadmap.md)) or lake-writer failover (frontier lag spike). No acknowledged write is lost — the quorum has it. | Automatic; watch [lead churn](runbooks.md#lead-churn) and [frontier lag](runbooks.md#frontier-lag-growing). No ceremony. |
| Postgres primary lost, quorum intact | kubernetes `scaled` | Managed-service failover inside the quorum; the fork check at the next lead start verifies the head survived. | Automatic plus the fork check. A fork-check refusal here means the quorum was not what the configuration claimed — [fork check failing](runbooks.md#fork-check-failing). |
| Host disk lost | standalone, docker, kubernetes `local` | Everything on that disk: Postgres, data directory or volumes, and in the `kms-file` profiles possibly the keyring. | The full [restore](restore.md) ceremony from the backup set. RPO is the backup composition above. |
| Object store lost, Postgres intact | all | Ciphertext bodies and Delta tables gone; the journal still names every digest. | Restore ciphertext prefixes from their own backups where they exist; the lake refolds. `missing` findings drain through repair — no Postgres restore, no rewind. |
| Keyring file lost, no usable copy | docker, standalone (`kms-file`) | **Every body and attachment in the deployment becomes permanent noise**: the KEK is gone, no DEK can ever be unwrapped. The journal, chain and attribution survive and still verify. | None for content — this is crypto-shredding of the whole deployment, by the same mechanism that makes erasure work. The keyring's backup discipline is exactly as load-bearing as this row implies ([deployment — backups](deployment.md), [RS-12](../product/roadmap.md)). |
| KMS KEK destroyed | kubernetes | Same shape as the keyring row, scoped to whatever that KEK wrapped and not re-wrapped: wrapped DEKs decay to noise. | None for the affected content, by design. Scheduled destruction follows the [KEK rotation](runbooks.md#kek-rotation) ceremony precisely so this never happens unscheduled. |
| Region / total site loss | all | Everything co-located is gone at once. | The restore ceremony into new infrastructure, from backups retained **off-site** — a backup stored next to what it backs up is a scenario on this row, not a mitigation of it. The standalone and docker profiles are one trust domain on one host ([deployment](deployment.md)); for them, off-host backup retention is the entire total-loss story. |

## What is not recoverable, by design

- **Erased content.** A destroyed DEK is destroyed everywhere forever; the
  restore ceremony re-destroys what backups resurrect
  ([restore](restore.md)). A disaster does not un-erase, and neither does
  recovery — erasure is senior.
- **Acknowledged writes past the recovery point**, in every profile whose RPO
  is the backup composition. The chain rewinds consistently and verifies at
  the rewound head; only an externally retained head shows the gap
  ([RS-9](../product/roadmap.md)).
- **Content whose KEK is gone** — the keyring and KMS rows above. The
  deployment's chain still verifies end to end over the noise: the evidence
  survives the content, which is the design's central property
  ([audit and lineage](../architecture/audit-and-lineage.md)).
- **A unique process identity for the signing key.** Losing or rotating the
  signing key never loses history — signatures verify against the key that
  made them — but the rotation ceremony and its limits are
  [signing-key compromise](runbooks.md#signing-key-compromise)'s to state.

Attribution metadata survives every scenario on this page, including erasure;
closing that is [FR-1](../product/roadmap.md), not a recovery concern.
