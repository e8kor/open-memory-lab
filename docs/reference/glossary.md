---
status: Stable
verified: 2026-11-14
answers: "the terms the OML docs use and what each means"
---

# Glossary

| Term | Meaning |
|---|---|
| **memory** | One statement an agent or person wrote. OML records who approved it; it does not vouch for it. The code and the API also call a memory a claim, as in `claim_key`. |
| **claim key** | The name that groups the versions of one memory. The server derives one when none is sent. |
| **held** | A memory waiting for review. Recall serves it to its author, who asks for unreviewed memories, and to readers inside an in-progress workarea, marked unreviewed. A tenant admin reads it through the journaled admin read. |
| **approve / reject** | A reviewer's verdict on a held memory. The stored state after approval is `promoted`; the other states are `demoted`, `rejected` and `erased`. |
| **ratify** | A second identity confirming a proposed workarea status change or edge. Not used for memories. |
| **unreviewed** | The label recall puts on held memories inside an in-progress workarea and on the key's own held memories. |
| **workarea** | A node in the tree where memory lives and grants apply. The root is `universe`. |
| **key** | An API key with one role: `reader`, `writer`, `dreamer`, `admin` or `operator`. |
| **reviewer key** | A key with the role `dreamer`. It judges held memories and nothing else. |
| **dreamer** | The role name of a reviewer key in the CLI, the API and grants. The docs say reviewer; the code says `dreamer`. |
| **pipeline** | A search method the server offers, such as `bm25-postings`, the default. A recall can name one; see [retrieval](../architecture/retrieval.md). |
| **row security** | PostgreSQL's per-row access rule. OML applies it on every tenant table, so a key reads only its own tenant's rows. |
| **derives_from** | The memories a new memory is built on. A key cannot approve a memory whose sources it wrote. |
| **replacement** | A new version of a memory that names the one it replaces. Until it is approved, readers keep finding the old version. |
| **crew** | A named record of the agents that work together. OML records it; it does not run them. |
| **tenant** | One organization's isolated data on a server. The server derives it from the key. |
| **lead** | The server instance that accepts writes. This build runs one lead and no followers. |
| **journal** | The signed, hash-chained record of every write, verdict and key issuance. |
| **brief** | The memories a workarea's reviewers chose for an agent to read first when it enters. |
| **steering** | A memory of a kind that tells agents how to work. Recall returns it before hits. |
| **tip** | The current approved version of a memory. |
| **recall** | A ranked search over approved memories with `oml read --query` or `oml_read`. |
| **self-signed** | A memory approved by its own author on a development home. It counts once and is never called a review. |
