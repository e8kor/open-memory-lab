---
status: Stable
verified: 2026-11-14
answers: "every guarantee OML makes, what enforces it, the bounds on it, and what OML does not claim"
---

# What OML guarantees

This page lists every guarantee OML makes, in four tiers: guaranteed by construction, guaranteed within a bound, provided but not measured, and not claimed. A claim missing from this page is not a guarantee. "By construction" means the code has no path that violates the guarantee, such as a verb that does not exist.

## Guaranteed by construction

| # | Guarantee | Enforced by |
|---|---|---|
| 1 | A write is durable if and only if its single database transaction commits. Journal, metadata and outbox commit together. See [storage](../architecture/storage.md). | One commit point. No write path exists outside the server's use-cases. |
| 2 | Within a tenant, the order of commits equals the order of journal sequence numbers. | A PostgreSQL advisory lock on the chain head, held until commit. |
| 3 | The journal is append-only. | The database revokes `UPDATE` and `DELETE` on it and a trigger guards it. The service role does not own the table. |
| 4 | Until a reviewer who did not write a memory approves it, only these read it. Its author does, and so does the user page of the person the author key belongs to. Reviewer keys that may judge it do: the one it is served to, and a person's reviewer key, which lists it with its text (`GET /v2/review/held`). Readers inside an [in-progress workspace](../architecture/workspaces.md#in-progress-workspaces) that holds it do, and see it marked unreviewed. Also, an admin of the tenant through the journaled [admin read](../architecture/roles-and-keys.md#the-admin-read) ([ADR-0045](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21). See [admission](../architecture/admission.md). | The held state, which every other read path filters out; a check that the reviewer is not in the memory's declared derivation; and a serve binding. |
| 5 | A workspace moves forward through its statuses only. See [workspaces](../architecture/workspaces.md). | No backward transition exists as a verb or as a domain rule. |
| 6 | No tenant reads or writes another tenant's rows. See [security](../architecture/security.md). | The server derives the tenant from the key. Row-level security applies on every tenant table. No service role bypasses it. |
| 7 | The lake writer cannot decrypt anything. | The binary holds no server keyring key and cannot unwrap a memory's data key; it seals what it writes with its own key-encryption key. |
| 8 | No held memory body reaches the lake. See [storage](../architecture/storage.md). | The outbox carries digests for held writes. Only approval enqueues the body. |
| 9 | No key exceeds the verbs its role allows. An agent can never approve, sign, demote or delete. See [roles and keys](../architecture/roles-and-keys.md). | A server-side check per role. Every transport is an adapter over the same application layer. |
| 10 | A same-person approval is marked as such to every reader. When a person approves work their own agents wrote, every reader sees that the approver was the author's own person. No approval is ever presented as another identity's review. See [audit and lineage](../architecture/audit-and-lineage.md) for the mark. | A mark set by the server from the identities of the author and the reviewer, on every served memory and in the signed record. |
| 11 | A verdict counts once, for exactly what the server showed the reviewer. | A single-use serve binding to the reviewer key, the version and its digests. |

Guarantee 6 depends on three deployment settings: `FORCE ROW LEVEL SECURITY` on every tenant table, separate owner and service roles, and a tenant context set inside each pooled transaction. The [deployment page](../operations/deployment.md) lists them as requirements.

## Guaranteed within a bound

- **Tamper evidence.** The chain detects rewriting by anyone who cannot sign. An attacker who holds the signing key and can write to the database can rebuild a consistent chain. Detection then depends on chain heads stored outside the deployment.
- **Durability.** Surviving a failover without losing acknowledged commits needs a standby that applies each commit before it is acknowledged. A standby that only receives commits does not survive its own operating-system crash. A single-node install is one machine's disk. The settings are on the [deployment page](../operations/deployment.md).
- **Exactly-once lake writes.** This rests on the Delta library's transaction-identifier behavior, which has not been tested under load.
- **Access changes.** A denied key or withdrawn grant stops acting at the key's next request. The server recomputes access from the record on every request.

## Provided, not measured

Each item works. Its quality has no figure yet.

- Review throughput per reviewing crew: it depends on the reviewers, and there is no figure.
- Recall quality and latency at scale beyond the [benchmarks](../benchmarks/index.md): there are no other figures.
- Scan latency at admission: only the per-rule timings on the [admission page](../architecture/admission.md).
- Lake-writer throughput and commit latency: not yet measured.
- The record store's size ceiling: not yet measured.
- Time to restore from backup: not yet timed.

## Not claimed

- **That production agents read only published releases.** Release binding is designed and not built: today every key reads the newest approved version of what it may read ([workspace lifecycle](../architecture/lifecycle.md)).
- **That a memory is true.** Review attests process: who looked, under what policy, at which digests.
- **That lineage is complete.** The author declares which memories a new one derives from. OML checks what was declared.
- **Protection from the database administrator.** A superuser can reach what construction cannot. The chain makes their tampering evident within the bound above, not impossible.
- **Review quality.** The gate guarantees that a different identity looked, not that they looked well.
- **Privacy from the tenant's administrators.** An admin key reads every memory of its tenant, held ones included. Each read is journaled, and the journal is the only check on it.
- **Collusion prevention.** An author and a reviewer who collude show in the trail. OML does not prevent it.
- **Custody of client copies.** Content the client held before submission stays the client's.
- **Containment of a compromised server.** The server holds the signing key and can decrypt every tenant's content. If it is compromised, an attacker can read and sign anything.
- **Availability under integrity stress.** A stalled reviewer, a missing standby or a lead change stops writes rather than weakening a check.
