<!--
id: adr/index
tier: engineering
status: current
answers: which decision records exist, grouped by what they decide, with each record's status and the release that built it
-->

# Design records

Every decision that shapes Open Memory Lab is a numbered record in this directory. A record
states one decision set, its consequences and the alternatives it rejected. The tables below
group the records in eight categories. **Status** is `accepted`, `proposed`, `superseded` or
`declined`; **Built in** is the release that first shipped the decision, "(part)" where later
parts are not built, and "not built" where no release ships it. Open work that follows from a
record is a [GitHub issue](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap).
The [release notes](../changelog.md) say what each release changed.

## Trust and governance

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0001](ADR-0001-the-constitution.md) | The constitution: four rules, one bounded development exception | accepted | v0.1.0 (part) |
| [ADR-0007](ADR-0007-tenancy-first-class.md) | Tenancy is first-class | accepted | v0.1.0 (part) |
| [ADR-0009](ADR-0009-status-ladder.md) | The status ladder: development, committed, validated; forward only; medallion words stay in the lake | accepted | v0.5.2 |
| [ADR-0010](ADR-0010-roles-and-keys.md) | Roles and keys: reader, writer, dreamer, admin, operator | accepted | v0.1.0 (part) |
| [ADR-0012](ADR-0012-admission-and-review.md) | Admission and review: external crews, serve binding, policy objects | accepted | v0.1.0 (part) |
| [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md) | An in-progress workarea serves unreviewed memories inside it, a curator agent may only narrow them, and nothing leaves until a person commits it | accepted | v0.6.0 |
| [ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md) | A person reviews a listed set of held items in one act, each item judged and signed as one verdict | accepted | v0.5.7 |
| [ADR-0065](ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md) | The scan is a versioned, compiled-in rule set that vetoes leaked keys and holds what a reviewer should judge | accepted | v0.6.0 |

## Storage and data

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0002](ADR-0002-postgresql-system-of-record.md) | PostgreSQL is the system of record: one commit point, per-tenant signed chains, a synchronous journal domain | accepted | v0.1.0 (part) |
| [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) | Delta Lake is the derived body lakehouse: outbox-fed, single-writer, never the authority | accepted | v0.6.0 (part) |
| [ADR-0004](ADR-0004-bodies-and-attachments.md) | Bodies and attachments: content-addressed, per-artifact encryption, digest-only journal | accepted | not built |
| [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) | Erasure by crypto-shredding, with a named inventory and an honest boundary | accepted | not built |
| [ADR-0017](ADR-0017-reconciliation.md) | Reconciliation is an incremental, journalled fold with a closed finding vocabulary | accepted (not built) | not built |
| [ADR-0060](ADR-0060-the-lake-writer-drains-the-outbox-it-has-seals-what-it-materializes-and-answers-only-the-operator.md) | The lake writer drains the outbox it has, seals what it materializes, and answers only the operator | accepted | v0.6.0 |

## Search and recall

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0013](ADR-0013-reads-served-from-postgres.md) | Reads are served from Postgres, journalled: one serving tier, a logged read path, steering ahead of hits | accepted | v0.1.0 (part) |
| [ADR-0048](ADR-0048-search-improves-in-stages-any-word-first-meaning-last-each-gated-on-the-recall-eval.md) | Search improves in stages: any word first, meaning last, each gated on the recall eval | proposed | v0.5.5 (part) |
| [ADR-0049](ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) | Search is a pipeline of retrievers behind a port, chosen by name and gated by the recall eval | proposed | v0.5.5 |
| [ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) | Text search reads integer postings, which row security lets an index serve, and says when it pruned a word or scanned | accepted | v0.5.6 |
| [ADR-0053](ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md) | Reranking is a local cross-encoder stage after fusion, off by default until it fits the latency budget | accepted | v0.5.6 |
| [ADR-0054](ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) | Recall is measured by what readers do next, and only other readers' use moves a rank, within a bound | accepted in part | v0.5.7 |
| [ADR-0059](ADR-0059-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model.md) | A memory states when it is true, recall hides what has ended, and recall searches the dates a query names without a model | accepted | v0.6.0 |

## Writing and review

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0050](ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md) | Remembering checks what is already held, replaces instead of repeating, and says how the memory will be found | accepted | v0.6.0 |
| [ADR-0021](ADR-0021-ai-human-ontologies.md) | AI+Human ontologies: vocabularies as governed claims | accepted | v0.5.2 (part) |
| [ADR-0011](ADR-0011-simulations.md) | Simulations are a fenced provenance channel with taint propagation | accepted (not built) | not built |
| [ADR-0020](ADR-0020-dataset-onboarding.md) | Dataset onboarding: the import manifest | accepted (post-v1) | not built |

## Sources and sync

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md) | A memory carries the source anchor its author declares, and `oml sync` keeps a workarea in step with a repository's documents | accepted | v0.5.8 |
| [ADR-0063](ADR-0063-a-chat-turn-becomes-memory-only-when-the-agent-in-it-writes-the-fact-and-every-memory-says-what-kind-of-source-it-came-from.md) | A chat turn becomes memory only when the agent in it writes the fact, and every memory says what kind of source it came from | accepted | v0.6.0 |
| [ADR-0047](ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle.md) | Project knowledge and a code skeleton follow the development lifecycle | proposed | v0.5.8 |

## Integrations and agents

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0014](ADR-0014-http-api-and-mcp-facade.md) | One versioned JSON HTTP API; MCP streamable HTTP as a stateless facade | accepted | v0.1.0 (part) |
| [ADR-0022](ADR-0022-two-ring-api.md) | The two-ring API: v1 memory provider, v2 memory lab | accepted | v0.1.0 |
| [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) | One key writes and recalls through grants, and an agent reads back, budgets, supersedes and retires its own memory only through admission | accepted | v0.2.0 (part) |
| [ADR-0028](ADR-0028-agent-key-issuance.md) | The CLI issues an agent's key in one call with its reader grant by default, and an admin skill runs it without the key value entering a transcript | accepted | v0.2.1 |
| [ADR-0029](ADR-0029-session-start-hook.md) | The agent plugin starts every session from what OML holds for its key, through `oml hook session-start` | accepted | v0.2.2 |
| [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) | Claude's hosted apps reach the agent plugin over OAuth, through a connection that a one-time connect code binds to one agent key | accepted | v0.4.0 |
| [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md) | A guardrail is a claim of its own kind, ratified under its workarea's review policy, that the agent plugin loads at every session start within the scope an admin set on its key, and whose optional rule a local hook checks before each tool call | accepted | v0.5.1 |
| [ADR-0038](ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md) | The agent recalls for itself, and a failed command recalls only when the user opted in | accepted | v0.4.0 |
| [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) | The API is rewritten for v0.4.0 while OML is pre-release: three task-shaped MCP tools, a contract that declares only what is served, one URL and one key to start, and nothing lost | accepted | v0.4.0 |
| [ADR-0040](ADR-0040-plugin-setup-in-claude-code.md) | The plugin takes its server URL and its key through Claude Code's plugin configuration, keeps the key in the keychain, and falls back to `OML_URL` and `OML_API_KEY` only in its hooks | accepted | v0.4.0 |

## Operations and security

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0015](ADR-0015-cli-administration.md) | The CLI is the administration surface; a web console is declined | accepted in part | v0.1.0 (part) |
| [ADR-0016](ADR-0016-kubernetes-workloads.md) | Kubernetes workloads, two Helm profiles, deployment identity replaces volume identity | accepted | v0.3.0 (part) |
| [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) | Three install profiles, one conformance bar; cluster-mode lead/follower | accepted | v0.1.0 (part) |
| [ADR-0033](ADR-0033-interim-helm-chart.md) | The Helm chart starts as the current build's one lead: Recreate, a kept keyring claim, and the claim before any Ingress | proposed | v0.3.0 |
| [ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) | The lead migrates as an owner role and serves as a role that owns nothing; `_sqlx_migrations` is the one table without row-level security | accepted | v0.3.1 (part) |
| [ADR-0041](ADR-0041-a-quick-start-in-one-command-per-role.md) | A quick start in one command per role: an installer that checks what it downloads, `oml login`, `oml setup claude-code` and `oml quickstart` | accepted | v0.4.0 (part) |
| [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) | Every request logs one line under an id the caller may name, metrics stay off the public port, and trace export is opt-in | accepted | v0.4.1 |
| [ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) | What OML releases defaults to the maintainer's public server, so an agent needs only its key; only `oml setup claude-code` among the CLI's verbs takes that default | accepted | v0.4.1 |
| [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) | Every user works under a root of their own beneath `universe`, and `universe` serves only curated claims, without their source | accepted | v0.5.0 |
| [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) | People sign in with Google or GitHub to a user page and an admin page that the lead serves on its own origin, and every act there is a session key's | accepted | v0.5.0 |
| [ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) | The lifecycle, claim edges and the brief ship in v0.5.2 inside user roots, with no new tool | accepted | v0.5.2 |
| [ADR-0026](ADR-0026-names-are-keys.md) | Workareas, crews, keys and tenants are named by unique, immutable keys that every surface accepts in place of their ids | accepted | v0.3.0 (part) |
| [ADR-0058](ADR-0058-a-public-server-runs-to-stated-slos-with-an-error-budget.md) | A public server runs to stated SLOs with an error budget | accepted | v0.6.0 |

## Architecture and platform

| ADR | Decision | Status | Built in |
|---|---|---|---|
| [ADR-0006](ADR-0006-pure-rust.md) | Pure Rust, with Scala/Spark behind a named trigger outside the TCB | accepted | v0.1.0 (part) |
| [ADR-0008](ADR-0008-workarea-algebra.md) | Workarea algebra: per-tenant universe tree, node-id grants, deny semantics, edges | accepted | v0.1.0 (part) |
| [ADR-0018](ADR-0018-the-documentation-set.md) | The documentation set: tiers, honesty discipline, llms.txt, frontmatter | accepted; amended by ADR-0066 | in force since spec-v0.1 |
| [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) | Hexagonal architecture and plugin modularity | accepted | v0.1.0 (part) |
| [ADR-0024](ADR-0024-performance-foundation.md) | The performance foundation | accepted | not built |
| [ADR-0066](ADR-0066-open-work-is-tracked-as-github-issues-and-the-roadmap-fr-numbers-and-design-draft-status-lines-are-retired.md) | Open work is tracked as GitHub issues, and the roadmap, FR numbers and design-draft status lines are retired | accepted | v0.6.0 documentation |

## The D-register: which page owns which decision

[ADR-0018](ADR-0018-the-documentation-set.md) d5 gives every rule exactly one declaration
site and makes this register record which page owns which decision handle. The `Dn` handles
below are the decision citations used across the engineering tier: each is **declared** on
exactly one owning page and only **cited** everywhere else. A `Dn` cited anywhere in the set
that does not appear in this table is a defect in the citing page; a second page declaring
one is a defect in that page.

| Handle | Decision, in one line | Owning page | Decision record(s) |
|---|---|---|---|
| D1 | PostgreSQL is the sole transactional commit point: one transaction commits the signed journal event, the fold state and the outbox row, on a per-tenant signed hash chain | [architecture/storage.md](../architecture/storage.md) | [ADR-0002](ADR-0002-postgresql-system-of-record.md) |
| D2 | The Delta lake is derived, fed exactly-once by a single lake-writer draining the outbox; it serves no client read, and when journal and lake disagree the journal wins | [architecture/storage.md](../architecture/storage.md) | [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) |
| D3 | Bodies and attachments are content-addressed by plaintext digest, uploaded over resumable sessions, encrypted per artifact, and bound into the signed envelope by digest | [architecture/storage.md](../architecture/storage.md) | [ADR-0004](ADR-0004-bodies-and-attachments.md) |
| D4 | Erasure is a journalled crypto-shred — DEK destruction scoped by the named copy-class inventory; the journal trail and attribution remain | [architecture/audit-and-lineage.md](../architecture/audit-and-lineage.md) | [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D5 | The lake writer is pure-Rust delta-rs, and all lake maintenance runs inside the single lake-writer process — no second writer identity | [architecture/storage.md](../architecture/storage.md) (instantiated column-level in [data/delta-tables.md](../data/delta-tables.md)) | [ADR-0006](ADR-0006-pure-rust.md) |
| D6 | Tenancy is first-class: tenant derived server-side from the key, RLS on every table but `_sqlx_migrations`, which privilege fences, an owner that migrates and a serving role that owns nothing, least-privilege privileged services, quotas as journalled grant attributes | [architecture/security.md](../architecture/security.md) | [ADR-0007](ADR-0007-tenancy-first-class.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) |
| D7 | The workarea algebra: one universe tree per tenant, node-id grants attenuated by denies with deny-wins, labelled edges; universe refuses direct claim writes, and under ADR-0044 holds only curated claims beside one user root per user | [architecture/workareas.md](../architecture/workareas.md) | [ADR-0008](ADR-0008-workarea-algebra.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0026](ADR-0026-names-are-keys.md), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) |
| D8 | The status ladder — development → committed → validated — moves forward only; validated is frozen, and transitions need a proposer and a separate ratifier | [architecture/workareas.md](../architecture/workareas.md) | [ADR-0009](ADR-0009-status-ladder.md) |
| D9 | Roles are reader, writer, dreamer, admin, operator, each under a per-role verb ceiling; admin is never a reviewer, and issuance never amplifies authority | [architecture/roles-and-keys.md](../architecture/roles-and-keys.md) | [ADR-0010](ADR-0010-roles-and-keys.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0026](ADR-0026-names-are-keys.md), [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) |
| D10 | Simulations are a fenced provenance channel: indelible provenance, audience-scoped visibility, taint along the lineage closure, overlay tips never the default tip, end-of-simulation demotion | [architecture/simulations.md](https://github.com/e8kor/open-memory-lab/blob/main/design/simulations.md) | [ADR-0011](ADR-0011-simulations.md) |
| D11 | Admission is capture → deterministic scan (veto, never promote) → held → non-author review → ratification under a journalled policy snapshot; it fails closed by holding | [architecture/admission.md](../architecture/admission.md) | [ADR-0012](ADR-0012-admission-and-review.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D12 | Reads are served from Postgres only and journalled; steering precedes hits unranked, and reads fail closed as absence | [architecture/retrieval.md](../architecture/retrieval.md) | [ADR-0013](ADR-0013-reads-served-from-postgres.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D13 | One versioned, two-ring JSON HTTP surface with a closed fail-closed error taxonomy and idempotency by construction; MCP is a stateless facade over it | [api/http-api.md](../api/http-api.md) | [ADR-0014](ADR-0014-http-api-and-mcp-facade.md), [ADR-0022](ADR-0022-two-ring-api.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0026](ADR-0026-names-are-keys.md), [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md), [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md), [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) (proposed), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) |
| D14 | The CLI is the administration surface — its verb set, the show-once key ceremony, the human review exception path; a web console is declined, which ADR-0045 amends to a user page and an admin page, with a closed route set, for people signed in by Google or GitHub | [cli/reference.md](../cli/reference.md) | [ADR-0015](ADR-0015-cli-administration.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0026](ADR-0026-names-are-keys.md), [ADR-0028](ADR-0028-agent-key-issuance.md), [ADR-0029](ADR-0029-session-start-hook.md), [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md), [ADR-0038](ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md), [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md), [ADR-0040](ADR-0040-plugin-setup-in-claude-code.md), [ADR-0041](ADR-0041-a-quick-start-in-one-command-per-role.md), [ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) |
| D15 | What runs where: the Kubernetes workload set and replica counts, two Helm profiles, deployment identity in place of volume identity | [operations/deployment.md](../operations/deployment.md) | [ADR-0016](ADR-0016-kubernetes-workloads.md), [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md), [ADR-0033](ADR-0033-interim-helm-chart.md) (proposed) |
| D16 | Reconciliation is an incremental, journalled fold of the journal with a closed finding vocabulary; every repair travels through journalled channels | [architecture/reconciliation.md](https://github.com/e8kor/open-memory-lab/blob/main/design/reconciliation.md) | [ADR-0017](ADR-0017-reconciliation.md) |
| D17 | Measured, not assumed: no page states a number for an unmeasured quantity, and every unmeasured or unverified claim resolves to exactly one roadmap row with an exit criterion | [product/roadmap.md](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap) | [ADR-0018](ADR-0018-the-documentation-set.md) d3, [ADR-0024](ADR-0024-performance-foundation.md) |

## Format

Every record has the same shape, and a record that drops a section is a defect:

- an H1 of the form `ADR-NNNN — title`, then one **Status:** line (`accepted design` with its
  date — a fact about the decision, not the code), then one **Release:** line, the record's row
  on [releases](../changelog.md) in short, which is where the code is described;
- **Context** — why the decision had to be taken, kept short;
- **Decision** — numbered `d1`…`dn`, one decision per number, so later records can amend by
  handle;
- **Consequences** — what the decision commits downstream pages and the build to, including
  the issues it opens in the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap);
- **Rejected alternatives** — the roads not taken, each with the reason it lost (present on
  every record that weighed alternatives; ADR-0001 and ADR-0018 carry none).

### Status lifecycle

A record's Status line moves through a fixed lifecycle, each step dated:

> proposed → accepted design → `accepted design · amended by ADR-nnnn` / `superseded by ADR-nnnn`

An accepted record is never edited to say something different — a change of mind is a **new**
record. The new record carries, directly under its own Status line, an **Amends:** line
(`ADR-nnnn (dm, …)` — which record and which decision handles it changes) or a
**Supersedes:** line (the whole earlier record is replaced); the earlier record's Status line
gains the matching back-pointer and the earlier text stays in place as history. The `dn`
handles exist exactly so an amendment can name what it touches.

### Registering a new record

A new ADR registers at three points, and missing any of them is a defect: the record file
(`docs/adr/ADR-NNNN-short-slug.md`, next free number, numbers never reused), a row in this
index's register table, and a nav entry in `mkdocs.yml`. [TEMPLATE.md](TEMPLATE.md) is the
starting skeleton, matching the shape above.
