---
status: Stable
verified: 2026-11-14
answers: "what evidence OML produces, which data classes it holds and who controls their retention"
---

# Compliance evidence

OML produces evidence an auditor can check. It is not certified against any framework, and no assessor has examined it. Whether a deployment meets a requirement is a judgment for the auditor and counsel. The limits of each mechanism below are in [what OML guarantees](commitments.md).

## What OML records

| Mechanism | What it evidences | GDPR article |
|---|---|---|
| **The journal.** Every write, verdict, approval, removal and key act is a signed, hash-chained, attributed entry. See [audit and lineage](../architecture/audit-and-lineage.md). | Who did what, when and under whose authority. The procedure in [chain format](../data/chain-format.md) checks that the record was not edited, run against the journal table by someone with read access to the database; the built-in `oml audit verify-chain` refuses until the server serves a chain export, which is not built. | Art. 30, Art. 32(1)(b) |
| **Review.** Recall serves a memory to other keys only after an identity that did not write it approves it, except inside an in-progress workarea, where it is marked unreviewed. A tenant admin reads held memories through the [admin read](../architecture/roles-and-keys.md#the-admin-read), and each such read is a journal entry, `memory.admin_read`. The verdict binds to what the server showed the reviewer. See [admission](../architecture/admission.md). | Separation of duties on content, with an attributable approval trail. Review attests process, never truth. | Art. 32 |
| **Tenant isolation.** The server takes the tenant from the key and the database enforces it on every table. See [security](../architecture/security.md). | A technical access restriction enforced in code and database. | Art. 32(1)(b) |
| **Access checks.** The server recomputes access from journaled grants and denials on every request. | When a key stopped being able to act. | Art. 32 |
| **Key custody.** Keys are shown once and stored as hashes. The wrapping key sits in a key-management service where the install profile provides one. | Separation of secrets, per install profile. | Art. 32 |

A row means "this mechanism produces evidence relevant to this article". It does not mean the article is met. The GDPR text is [Regulation (EU) 2016/679](https://eur-lex.europa.eu/eli/reg/2016/679/oj).

## What OML does not offer

- **Erasure.** No verb erases a memory in this build. Erasure by crypto-shredding is designed ([ADR-0005](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0005-erasure-by-crypto-shredding.md)) and not built; the API's `410 lifecycle.erased`, the [lake](../operations/lake.md#what-happens-on-erase) page's erase section and the [restore](../operations/restore.md) page's erasure hazard describe that design. Removal today is a journaled retirement that keeps the bytes; demotion is not built either. A person's deletion request, once an admin closes it, disables the person and leaves the bytes. A right-to-erasure request needs a procedure run outside OML.
- **Evidence export.** `oml audit export` and its signed evidence bundle are designed ([ADR-0015](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0015-cli-administration.md) d4) and not built, and the server serves no chain export. An auditor verifies the journal by the [chain format](../data/chain-format.md) procedure. A person's own export is built: the web console downloads every memory in the person's user root, with the person's keys, as one JSON file (`GET /v2/web/export`). It is a copy for the person, not an evidence bundle.
- **Per-person lookup.** No index finds every memory about a person. Searching means reading the content itself.
- **Legal hold or retention schedule.** Nothing expires memory content. Retire memories deliberately where a data-minimization duty applies.

## Data classes and retention

| Data class | Where it lives | Who controls retention |
|---|---|---|
| Memory bodies | PostgreSQL, the object store and the lake as ciphertext, and search-index entries derived from readable text. | The operator. Content stays: a journaled retirement takes it out of recall and keeps the bytes, and no verb erases it in this build. |
| Journal metadata | PostgreSQL. It records who wrote, reviewed and approved, with timestamps and placement. | OML. The record is append-only and permanent. |
| Telemetry | The monitoring stack, outside OML's data stores. | The operator. Metrics and logs carry no memory content, no query text and no principal values. See [observability](../operations/observability.md). |

Agent memory is often free text about people. Treat bodies as personal data unless known otherwise.

The telemetry rule keeps the monitoring pipeline free of copies that a removal would have to reach.

Recall telemetry keeps each recall for 30 days, with query text only if the organization opts in (`oml usage settings`).
