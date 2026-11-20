---
status: Stable
verified: 2026-11-19
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
| **workspace** | A node in a tenant's tree, not an account or environment: memory is written into a workspace, grants bind to its node and reach its subtree, and workspaces nest under the root, `universe`. The tenant is the boundary. In Databricks and Microsoft Fabric a workspace is an environment boundary; in OML it is a node in the tree. Called a workarea before v0.8.3. |
| **status** | Where a workspace stands on its ladder: `development`, then `protected`, then `frozen`, each step proposed by one identity and ratified by another, never undone. A `protected` workspace needs two persons to reach and takes no shared drafts; a `frozen` one takes no new writes. Called `committed` and `validated` before v0.8.3. |
| **key** | An API key with one role: `reader`, `contributor`, `submitter`, `reviewer`, `admin` or `operator`. |
| **contributor key** | A key that recalls and writes: at least one `reader` grant and one `submitter` grant. Called a writer key before v0.8.4. One holding a submitter grant on a node no reader grant of it reaches, such as a writer key issued with `--write-only`, is a contributor without recall, named by `oml doctor`. |
| **submitter** | A key that writes without recall, holding `submitter` grants alone, and the grant role that confers writing (`remember`, `learn`, `attach`, `supersede`). It reads its own memories and nothing else. |
| **reviewer key** | A key with the role `reviewer`. It proposes edits and connections and reviews other identities' writes, never on a lineage it proposed in. The role was called `dreamer` before v0.8.3. |
| **take** | A reviewer takes one held memory at a time to judge it; another reviewer cannot take it meanwhile. |
| **used by** | Who a key is for, attested by the admin who issued it: `agent`, `person` or `service`, the field `used_by`. Called `held_by`, with `human` for a person, before v0.8.3. |
| **pipeline** | A search method the server offers, such as `bm25-postings`, the default. A recall can name one; see [retrieval](../architecture/retrieval.md). |
| **row security** | PostgreSQL's per-row access rule. OML applies it on every tenant table, so a key reads only its own tenant's rows. |
| **derives_from** | The memories a new memory is built on. A key cannot approve a memory whose sources it wrote. |
| **replacement** | A new version of a memory that names the one it replaces. Until it is approved, readers keep finding the old version. |
| **crew** | A named record of the agents that work together. OML records it; it does not run them. |
| **tenant** | One organization's isolated data on a server. The server derives it from the key. |
| **lead** | The server instance that accepts writes. This build runs one lead and no followers. |
| **journal** | The signed, hash-chained record of every write, verdict and key issuance. |
| **brief** | The memories a workspace's reviewers chose for an agent to read first when it enters. |
| **steering** | A memory of a kind that tells agents how to work. Recall returns it before hits, unless `kinds` leaves it out. |
| **catalog** | What a read without a query lists: every kind of claim the key reads in a workspace and below, newest first, each with its name or a preview, version and approval and no text unless asked, and on the first page a count per kind ([ADR-0092](../adr/ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md)). The key's own work is the same read with `own: true`. |
| **tip** | The current approved version of a memory. |
| **recall** | A ranked search over approved claims with `oml read --query` or `oml_read`: by default every kind but steering, test and the vocabulary kinds, or the kinds `kinds` names; skills only as offered, guardrails only as applicable, and documents by passage ([ADR-0092](../adr/ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md) d1). |
| **self-signed** | Designed, not built: a memory approved by its own author in a development workspace. It would count once and never be called a review. |
| **withdraw** | The author retracting its own held version before any verdict, for a mistaken write. The version is kept as `withdrawn` with its text, leaves every review queue and is never served; no reviewer is involved ([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d13). |
| **bundle** | One file that moves a workspace between servers: a tar with a signed manifest, encrypted to named recipients. Its versions land held and are reviewed again on the target ([bundle format](../data/bundle-format.md)). |
| **development rules / production rules** | The two rule classes a workspace's place sets: an open workspace is development, a `protected` or `frozen` one is production for every reader ([the place carries the rules](../architecture/lifecycle.md#lifecycle-development-to-release)). |
| **release** | Designed, not built: a fixed manifest of a workspace's promoted versions, cut by one person and published by another ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d1, d4). |
| **publish** | Designed, not built: the second person's act that makes a cut release readable to the keys bound to it, and binds its label ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d2, D3). |
| **staging** | Designed, not built: the stage between a release's cut and its publication, when a person's key and a CI key read the candidate and no agent key does ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) D3). |
| **binding** | Designed, not built: which version a key reads in a workspace: `tip`, `latest` or a pin. Today every key reads the tip ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d6). |
| **latest** | Designed, not built: the binding that reads a workspace's newest published release and never a held item ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d6). |
| **pin** | Designed, not built: the binding that reads one published release by its label, such as `payments.dev@1.3.0` ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d6). |
| **admin fix** | Designed, not built: an admin's marked act that demotes, edits, retires or rolls back production content at once, which another person confirms or reverts afterwards ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)). |
