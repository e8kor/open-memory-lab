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
| **held** | A memory waiting for review. Recall serves it to its author, who asks for unreviewed memories, and to readers inside an in-progress workspace, marked unreviewed. A tenant admin reads it through the journaled admin read. |
| **approve / reject** | A reviewer's verdict on a held memory. The stored state after approval is `promoted`; the other states are `demoted`, `rejected` and `erased`. |
| **ratify** | A second identity confirming a proposed workspace status change or edge. Not used for memories. |
| **unreviewed** | The label recall puts on held memories inside an in-progress workspace and on the key's own held memories. |
| **workspace** | A node in a tenant's tree, not an account or environment: memory is written into a workspace, grants bind to its node and reach its subtree, and workspaces nest under the root, `universe`. The tenant is the boundary. In Databricks and Microsoft Fabric a workspace is an environment boundary; in OML it is a node in the tree. Called a workarea before v0.8.0. |
| **status** | Where a workspace stands on its ladder: `development`, then `protected`, then `frozen`, each step proposed by one identity and ratified by another, never undone. A `protected` workspace needs two persons to reach and takes no shared drafts; a `frozen` one takes no new writes. Called `committed` and `validated` before v0.8.0. |
| **key** | An API key with one role: `reader`, `writer`, `reviewer`, `admin` or `operator`. |
| **reviewer key** | A key with the role `reviewer`. It proposes edits and connections and reviews other identities' writes, never on a lineage it proposed in. The role was called `dreamer` before v0.8.0. |
| **take** | A reviewer takes one held memory at a time to judge it; another reviewer cannot take it meanwhile. |
| **used by** | Who a key is for, attested by the admin who issued it: `agent`, `person` or `service`, the field `used_by`. Called `held_by`, with `human` for a person, before v0.8.0. |
| **pipeline** | A search method the server offers, such as `bm25-postings`, the default. A recall can name one; see [retrieval](../architecture/retrieval.md). |
| **row security** | PostgreSQL's per-row access rule. OML applies it on every tenant table, so a key reads only its own tenant's rows. |
| **derives_from** | The memories a new memory is built on. A key cannot approve a memory whose sources it wrote. |
| **replacement** | A new version of a memory that names the one it replaces. Until it is approved, readers keep finding the old version. |
| **crew** | A named record of the agents that work together. OML records it; it does not run them. |
| **tenant** | One organization's isolated data on a server. The server derives it from the key. |
| **lead** | The server instance that accepts writes. This build runs one lead and no followers. |
| **journal** | The signed, hash-chained record of every write, verdict and key issuance. |
| **brief** | The memories a workspace's reviewers chose for an agent to read first when it enters. |
| **steering** | A memory of a kind that tells agents how to work. Recall returns it before hits. |
| **tip** | The current approved version of a memory. |
| **recall** | A ranked search over approved memories with `oml read --query` or `oml_read`. |
| **self-signed** | Designed, not built: a memory approved by its own author in a development workspace. It would count once and never be called a review. |
