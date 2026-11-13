---
status: Stable
verified: 2026-11-06
answers: "how to back up an OML server and restore it: what to copy, the restore steps in order, why a restore rewinds the chain and can bring back erased content, and what each kind of loss costs"
---

# Back up and restore

Two things are backed up together: the PostgreSQL database and the keyring file. A restore brings both back to
one moment. It cannot bring back what happened after that moment.

## Back up

A PostgreSQL superuser and a place off the server's host to keep copies are required.

1. **Back up the database.** Run a daily `pg_dump` as a superuser, because forced row security stops
   the owner role's dump. Or use base backups with WAL archiving, which allow a restore to a point in time.
2. **Copy the keyring file** (`kms.keyring_path`) in the same run, under its own protection. A database
   restored without the keyring cannot be read.
3. **Keep the copies off the host.** A backup stored next to what it backs up does not survive the
   loss of the disk.
4. **Alert on a failed backup** ([runbooks](runbooks.md)) and rehearse one restore into a scratch
   PostgreSQL before relying on it.

A pre-upgrade dump on the data volume protects an upgrade and nothing else, because it shares the
volume it would restore ([Upgrade OML](upgrades.md)).

## Why a restore is a ceremony

A restore has two hazards:

- **It rewinds the chain.** A backup older than the last acknowledged write holds an older chain head
  than the one the server signed. The restored journal is a consistent, signed prefix, so it verifies
  from inside. Only a head kept outside the server shows what the restore discarded.
- **It can bring back erased content.** A backup taken before an erase holds the content and the wrapped
  key that the erase destroyed.

A lost lake, or other derived data, is not a restore scenario. The lake is derived from the journal.

## Restore

Three things must be at hand: the backup set, the keyring copy from the same moment, and a head seq
recorded earlier. `oml events` prints the head seq.

1. **Stop the server and the lake writer.** Writes must not race the restore.
2. **Record the current state.** If the server can still answer, save `oml doctor --json` and the
   current head seq. This is the evidence that tells a restore from tampering later.
3. **Restore PostgreSQL** to the recovery point. Use the host's base-backup and WAL tooling, `pg_restore`
   of a dump, or the managed service's point-in-time recovery into a new instance. Then point
   `OML_POSTGRES__URL` and `OML_POSTGRES__OWNER_URL` at it. A dump restores one moment and nothing after it.
4. **Place the keyring copy before the server starts.** Put the copy from the same moment at
   `kms.keyring_path`, owner-only (mode 0600). A server started against a missing or wrong keyring fails at
   its first decryption, not at startup.
5. **Install the server version that matches the backup, or newer.** A newer server migrates the
   schema forward at boot, as in any upgrade ([Upgrade OML](upgrades.md)). Migrations never run backward.
6. **Start the server and let the fork check run.** The stored chain head must verify against the last
   signed head. A clean restore passes, because the rewound journal is a consistent prefix. A pass does not
   prove that nothing was discarded. Compare the restored head seq with the one recorded, and note the
   gap. The server refuses writes until it is ready, and there is no override.
7. **Check.** `oml doctor` must report no failing field before traffic resumes.
8. **Resume traffic, then restart the lake writer** if one runs. How the writer resumes against a rewound
   journal is not verified. Run `oml-lakewriter doctor` and check the lake against the journal before relying on it
   ([Run the lake writer](lake.md)).

The restored server keeps the same signing key and the same history up to the recovery point. Keep the
new head seq somewhere off the server.

## What a restore cannot do

- **Recover writes after the recovery point.** The chain rewinds consistently. A head seq saved before
  the loss is the only evidence of the gap.
- **Un-erase.** A destroyed key stays destroyed in the live system. A backup from before the erase still
  holds the content. The backup retention window is the upper bound on how long erased content survives
  ([compliance](../product/compliance.md)).
- **Prove its own innocence.** From inside the server, a restore and a rewrite by an attacker who holds
  the signing key look the same. The state recorded in step 2 and the head seqs kept outside tell
  them apart.

## What a loss costs

| Loss | What is lost | What to do |
|---|---|---|
| The host disk, no backup | Every memory, journal event and key | Nothing. Back up before this happens. |
| The host disk, with a backup | Everything since the last base backup and archived WAL, or since the last dump | The restore steps above. |
| PostgreSQL only, keyring intact | Everything since the last backup | Restore PostgreSQL, then start. |
| The keyring only, no copy | Every body becomes unreadable. The journal, the chain and the attribution still verify. | None for content. Back the keyring up as carefully as the database. |
| The whole site | Everything co-located | Restore into new infrastructure from off-site copies. |

Losing or replacing the signing key never loses history, because signatures verify against the key that
made them ([runbooks](runbooks.md)).

Recovery time is the steps above, from the restore to a clean `oml doctor`. It has not been timed.
