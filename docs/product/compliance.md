<!--
id: product/compliance
tier: client
status: design draft for review · decided, not built
answers: what evidence OML produces for GDPR, SOC 2 and ISO 27001 asks; what classes of data OML holds, where, and who controls their retention; whether memory content expires automatically
-->

# Compliance evidence

**Status:** design draft for review · decided, not built

This page maps Open Memory Lab's mechanisms to the questions compliance frameworks ask. The
claim is the sanctioned one and no stronger: the system **produces the evidence an auditor asks
for**. OML is not certified against any framework, no assessment of it has been performed,
nothing is built, and no sentence here or anywhere in this documentation calls the design
compliant, audit-ready or certified — the verdict belongs to the deploying organization's
auditor, assessor and counsel. What this page adds to the guarantee register in
[commitments](commitments.md) is the cross-walk: which mechanism produces evidence relevant to
which framework's ask, and where each mechanism's honest boundary sits.

## Evidence, not certification

Three rules govern every row of the mapping below:

- A row means "this mechanism produces evidence relevant to this requirement" — never "this
  requirement is met". Meeting a requirement involves the organization's own processes, scope
  definitions and an assessor's judgment, none of which a software design can supply.
- Every mechanism's limits carry over from [commitments](commitments.md) unchanged. In
  particular the erasure boundary: destruction is immediate on live stores and bounded — not
  instant — for copies in backups, attribution metadata (who wrote what, when, where) survives
  erasure until [FR-1](roadmap.md) closes, and copies a client held before submission are
  outside OML's reach entirely.
- Frameworks are named with their article or criterion identifiers so the reader can check the
  source: GDPR (Regulation (EU) 2016/679) Articles 15, 17, 30 and 32; the AICPA SOC 2 Trust
  Services Criteria; ISO/IEC 27001:2022 Annex A controls. SOC 2 criteria and ISO control
  references are given at the family level — an actual examination maps controls at a finer
  grain than a design page honestly can.

### The mapping

| Mechanism | What it evidences | GDPR | SOC 2 (Trust Services Criteria) | ISO/IEC 27001:2022 Annex A |
|---|---|---|---|---|
| **Signed journal** — every write, verdict, approval, removal and key act is a signed, hash-chained, attributed record ([audit and lineage](../architecture/audit-and-lineage.md)) | Who did what, when, under what authority — and that the record was not edited after the fact, within the stated tamper-evidence bound of [commitments](commitments.md) | Art. 30 (records of processing activities); Art. 32(1)(b) (integrity of processing) | CC7 (system operations); PI1 (processing integrity) | 5.33 (protection of records); 8.15 (logging); 5.28 (collection of evidence) |
| **Evidence export** — the signed bundle: per-memory decision trails, chain verification, grants history, the retention-scoped read log, reciprocal-approval statistics | An on-demand answer to an auditor's ask, verifiable against the deployment that produced it. Limit: the export is per-memory and per-tenant; locating every memory *about a person* is the tenant's search over its own content — no per-person index exists | Art. 15 (right of access — the raw material for answering one); Art. 30 | CC4 (monitoring activities — evidence for evaluations); P5 (privacy: access) | 5.28 (collection of evidence); 5.34 (privacy and protection of PII) |
| **Erasure inventory + crypto-shredding** — a journalled erase event destroys the content's key; the inventory names every copy class and its disposition ([audit and lineage](../architecture/audit-and-lineage.md)) | Demonstrable destruction with a proof trail that survives the destruction. The boundary, stated as [commitments](commitments.md) states it: scoped to body and attachment bytes; backup copies decay within the stated backup-retention plus key-rotation window (its size unmeasured — [FR-17](roadmap.md)); attribution metadata is not erased ([FR-1](roadmap.md)) | Art. 17 (right to erasure) | P4 (privacy: use, retention and disposal); C1 (confidentiality) | 8.10 (information deletion); 8.24 (use of cryptography) |
| **Read log** — every read is recorded: who read what, when, within a tenant-set retention window, integrity-anchored into the journal ([retrieval](../architecture/retrieval.md)) | Access history for stored content. Limit: it answers nothing outside the retention window and says so; the not-yet-anchored tail is a stated bound in [commitments](commitments.md) | Art. 30; Art. 32 (security of processing) | CC6 (logical access); CC7 (system operations) | 8.15 (logging); 8.16 (monitoring activities) |
| **Tenant isolation (row-level security)** — tenant identity is derived server-side from the key and enforced by the database on every table ([security](../architecture/security.md)) | A technical access-restriction measure enforced by construction, not convention | Art. 32(1)(b) | CC6 (logical access) | 5.15 (access control); 8.3 (information access restriction) |
| **Key custody** — one signing process; per-service credentials with stated "never" entries; keys shown once and stored only as hashes; the wrapping key held in a key-management service where the profile provides one ([security](../architecture/security.md)) | Custody and separation of secrets, per install profile — each profile's weakening is named in [commitments](commitments.md), never discovered | Art. 32 | CC6 (logical access) | 8.24 (use of cryptography); 5.17 (authentication information) |
| **Access checked on every request** — every grant change and revocation is a journalled record, and access is recomputed from those records on every request, so "when did this key stop being able to act" is answerable from the record ([workareas](../architecture/workareas.md)) | Timely, provable access removal: a revocation applies to the key's next request, and on a cluster's followers once their copy of the record catches up — the window [commitments](commitments.md) states | Art. 32 | CC6 (access provisioning and removal) | 5.18 (access rights) |
| **Review and lineage** — nothing is visible to others until an identity that did not write it ratified it; the reviewer check runs against the declared derivation graph; the verdict is bound to exactly what was served ([architecture: admission](../architecture/admission.md)) | Enforced segregation of duties on content, with an attributable approval trail. Limit: review attests process, never truth; collusion between distinct identities is made visible, not prevented ([commitments](commitments.md), never-claimed tier) | Art. 30; Art. 32 | PI1 (processing integrity); CC5 (control activities — segregation of duties) | 5.3 (segregation of duties); 5.28 (collection of evidence) |

## Data classification and retention

What OML holds, where, how sensitive it is, and who decides how long it stays. "Retention
authority" names the party whose act or setting bounds each class's lifetime — the table is the
map an operator's data-protection review starts from.

| Data class | Where it lives | Sensitivity | Retention authority |
|---|---|---|---|
| Memory bodies | The record store (held bodies and metadata), the object store and the analytics mirror (encrypted copies), plus search-index entries derived from the readable text | Tenant content. Agent memory is free text about people as the normal case, not an edge case — treat the class as personal data unless the tenant knows otherwise | **The tenant.** No automatic expiry exists in v1: content stays until a journalled demotion or erasure act, and erasure completes per the [erasure inventory](../architecture/audit-and-lineage.md)'s stated bounds |
| Attachments (files, images, PDFs) | The object store, encrypted; referenced — never copied readable — everywhere else | Same class as bodies, plus whatever the file format itself embeds (metadata, location tags) | **The tenant**, same acts and same bounds as bodies |
| Journal metadata | The record store, mirrored to the analytics store | Attribution: who wrote, reviewed, approved, removed — identity, timestamps, placement. Survives erasure; its own erasure or pseudonymization is open, tracked as [FR-1](roadmap.md) | **The design**: the record is append-only and permanent by construction — that permanence is the product's evidence property, and its tension with erasure is resolved as [audit and lineage](../architecture/audit-and-lineage.md) states, not denied |
| Read log | The record store, in time-partitioned storage | Who read what, when; queries are recorded as digests, never as text | **Tenant retention policy** — the window is a tenant-set policy and expired partitions are dropped; the one automatic retention bound that exists in v1 |
| Telemetry (metrics and service logs) | The deployment's monitoring stack, outside OML's data stores | None by rule: no memory content, no query text, and no principal values appear in metrics or logs — services identify actors by tenant and key identifiers only ([observability](../operations/observability.md) owns the rule) | **Deployment-site choice** — permitted precisely because, under the rule, this class holds nothing erasable |

Three statements the table implies, made explicit:

**Memory content has no automatic expiry in v1.** Nothing ages out. Every removal is a
journalled act — demotion keeps the bytes, erasure destroys the ability to read them — because
silent disappearance is the exact failure this product exists to refuse. The consequence runs
the other way too, and is stated rather than discovered: a tenant under a data-minimization or
retention obligation must operate removal deliberately. There is no set-and-forget retention
schedule, and no legal-hold act fences content against an erasure request. Both are a tracked
deferral, [FR-26](roadmap.md): a tenant retention policy whose age-out applies as journalled
acts, and a journalled legal hold — including a recorded decision on which wins when a hold and
an erasure request collide, because those two obligations genuinely conflict and a design that
pretends otherwise would be overclaiming.

**The read log's window is the only automatic retention in v1.** It is tenant policy, it bounds
what an evidence export can answer about reads, and the export says so instead of producing an
unverifiable tail.

**The telemetry rule is a correctness rule, not hygiene.** Metrics and logs carry no memory
content, no query text, and no principal values — actors appear as tenant and key identifiers
only. The rule exists so that monitoring pipelines, which are retained and indexed by machinery
outside OML's control, never hold a copy that erasure would have to reach
([observability](../operations/observability.md)).

## Where to go next

[Commitments](commitments.md) is the guarantee register every row above defers to;
[audit and lineage](../architecture/audit-and-lineage.md) owns the evidence export, the erasure
inventory and the named GDPR Article 17 tension; the [roadmap](roadmap.md) tracks every open
edge named here — [FR-1](roadmap.md) (attribution metadata), [FR-17](roadmap.md) (measured
erasure bounds), and [FR-26](roadmap.md) (retention policy and legal hold).
