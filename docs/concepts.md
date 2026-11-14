---
status: Stable
verified: 2026-11-14
answers: "the five ideas OML is built on, in two or three sentences each, the page that owns each one, and where the architecture views and the lifecycle are drawn"
---

# Concepts

Five ideas carry OML: memory, held, workarea, key and journal. Each section gives the short version and links the page that owns it. Where this page and the owning page disagree, the owning page wins. The [architecture views](architecture/views.md) draw how they fit together, and the [workarea lifecycle](architecture/lifecycle.md) how work moves from development to production.

## Memory

A memory is one statement an agent wrote. OML does not vouch for it. It records who wrote it and who approved it.

A memory has versions, grouped by a `claim_key`. The version others read is the current one. A replacement is held and reviewed like any write, and it names the version it replaces. A retirement removes the memory from recall and keeps every version as history.

See [write and read memories](use/remember-and-recall.md) and [audit and lineage](architecture/audit-and-lineage.md).

## Held

Every write is scanned, held, reviewed by someone who did not write it, signed, and only then readable. While a memory is held, recall serves it to its author, and to readers inside an [in-progress workarea](use/workareas.md#in-progress-workareas) marked unreviewed; a tenant admin reads it through the journaled [admin read](architecture/roles-and-keys.md#the-admin-read). A reviewer's approval moves it to state `promoted`.

--8<-- "states.md"


The reviewer is never the author, and approval decides who can read a memory, not how it ranks. When no reviewer is available, nothing is approved and held memories wait. See [why a write is held](concepts/why-a-write-is-held.md) and [admission](architecture/admission.md).

## Workarea

A workarea is a node in a tree where memory lives and grants apply. One tree exists per tenant, rooted at `universe`. Each person also gets a root of their own, `universe.<user>`, when their first key is issued.

A workarea moves forward through three statuses: development, committed and validated. Each step is proposed by one identity and confirmed by another. A crew is a named record of the agents that work together. OML records crews and does not run them. See [workareas](use/workareas.md) and [architecture: workareas](architecture/workareas.md).

## Key

A key is an API key with one role. The server reads the tenant and role from the key, never from the request. The roles are reader, writer, dreamer, admin and operator.

A reviewer key has the role `dreamer`. An admin cannot approve content, and a writer cannot approve its own. [Producers and reviewers](concepts/producers-and-reviewers.md) lists which pairs count in which scope. A grant gives a key a role on a workarea and its subtree. A deny always wins over a grant. An agent can write and recall, and it can never approve, sign, demote or delete. See [roles and keys](architecture/roles-and-keys.md).

## Journal

The journal is the signed, hash-chained record of every state change. It holds digests and references, never memory text. It is append-only and lives in PostgreSQL.

Removal is a recorded act. Nothing disappears without an entry, and the server refuses to start on a forked chain. `oml audit verify-chain` checks the chain. See [audit and lineage](architecture/audit-and-lineage.md) and [what OML guarantees](product/commitments.md).

## Terms

[The glossary](reference/glossary.md) defines every other term used in these docs.
