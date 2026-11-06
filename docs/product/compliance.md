---
status: Stable
verified: 2026-11-06
answers: "what evidence OML produces, which data classes it holds and who controls their retention"
---

# Compliance evidence

OML produces evidence an auditor can check. It is not certified against any framework, and no assessor has examined it. Whether a deployment meets a requirement is a judgment for your auditor and counsel. The limits of each mechanism below are in [what OML guarantees](commitments.md).

## What OML records

| Mechanism | What it evidences | GDPR article |
|---|---|---|
| **The journal.** Every write, verdict, approval, removal and key act is a signed, hash-chained, attributed entry. See [audit and lineage](../architecture/audit-and-lineage.md). | Who did what, when and under whose authority. `oml audit verify-chain` shows the record was not edited. | Art. 30, Art. 32(1)(b) |
| **Review.** A memory is invisible to others until an identity that did not write it approves it. The verdict binds to what the server showed the reviewer. See [admission](../architecture/admission.md). | Separation of duties on content, with an attributable approval trail. Review attests process, never truth. | Art. 32 |
| **Tenant isolation.** The server takes the tenant from the key and the database enforces it on every table. See [security](../architecture/security.md). | A technical access restriction enforced in code and database. | Art. 32(1)(b) |
| **Access checks.** The server recomputes access from journaled grants and denials on every request. | When a key stopped being able to act. | Art. 32 |
| **Key custody.** Keys are shown once and stored as hashes. The wrapping key sits in a key-management service where the install profile provides one. | Separation of secrets, per install profile. | Art. 32 |

A row means "this mechanism produces evidence relevant to this article". It does not mean the article is met. The GDPR text is [Regulation (EU) 2016/679](https://eur-lex.europa.eu/eli/reg/2016/679/oj).

## What OML does not offer

- **Erasure.** OML has no erase verb. Removal is a journaled demotion or retirement that keeps the bytes. A right-to-erasure request needs a procedure you run outside OML.
- **Evidence export.** No export verb exists. An auditor can read the journal and run `oml audit verify-chain`.
- **Per-person lookup.** No index finds every memory about a person. You search your own content.
- **Legal hold or retention schedule.** Nothing expires memory content. Retire memories deliberately if you have a data-minimization duty.

## Data classes and retention

| Data class | Where it lives | Who controls retention |
|---|---|---|
| Memory bodies | PostgreSQL, the object store and the lake as ciphertext, and search-index entries derived from readable text. | You. Content stays until a journaled demotion or retirement. |
| Journal metadata | PostgreSQL. It records who wrote, reviewed and approved, with timestamps and placement. | OML. The record is append-only and permanent. |
| Telemetry | Your monitoring stack, outside OML's data stores. | You. Metrics and logs carry no memory content, no query text and no principal values. See [observability](../operations/observability.md). |

Agent memory is often free text about people. Treat bodies as personal data unless you know otherwise.

The telemetry rule keeps your monitoring pipeline free of copies that a removal would have to reach.

Recall telemetry keeps each recall for 30 days, with query text only if your organization opts in (`oml usage settings`).
