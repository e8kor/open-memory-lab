# Decision register

**Status:** design draft for review · decided, not built

Every decision that shapes Open Memory Lab is a numbered record in this directory. A record
states one decision set and lists its consequences, open feature requests among them. The
register below is the complete set; the [roadmap](../product/roadmap.md) carries the FR/RS
rows the records open.

| ADR | Title | Status | Decides |
|---|---|---|---|
| [ADR-0001](ADR-0001-the-constitution.md) | The constitution: four rules, one bounded development exception | accepted design | R1–R4 and the only place the development-workarea exception is scoped |
| [ADR-0002](ADR-0002-postgresql-system-of-record.md) | PostgreSQL is the system of record: one commit point, per-tenant signed chains, a synchronous journal domain | accepted design | one transactional commit point; per-tenant hash chains; digest-only journal payloads |
| [ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md) | Delta Lake is the derived body lakehouse: outbox-fed, single-writer, never the authority | accepted design | the lake never serves clients; exactly-once outbox drain; poison-row handling |
| [ADR-0004](ADR-0004-bodies-and-attachments.md) | Bodies and attachments: content-addressed, per-artifact encryption, digest-only journal | accepted design | resumable uploads, digest binding, what a reviewer is served per attachment type |
| [ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) | Erasure by crypto-shredding, with a named inventory and an honest boundary | accepted design | DEK destruction as erasure; the copy-class inventory; what shredding does not erase |
| [ADR-0006](ADR-0006-pure-rust.md) | Pure Rust, with Scala/Spark behind a named trigger outside the TCB | accepted design | the implementation stack and the one condition under which Spark enters |
| [ADR-0007](ADR-0007-tenancy-first-class.md) | Tenancy is first-class | accepted design | server-side tenant derivation, RLS everywhere, least-privilege service credentials |
| [ADR-0008](ADR-0008-workarea-algebra.md) | Workarea algebra: per-tenant universe tree, node-id grants, deny semantics, edges | accepted design | the tree, the universe root, grant and deny resolution, labelled edges |
| [ADR-0009](ADR-0009-status-ladder.md) | The status ladder: development, committed, validated; forward only; medallion words stay in the lake | accepted design | what each status permits and refuses; two vocabularies kept apart |
| [ADR-0010](ADR-0010-roles-and-keys.md) | Roles and keys: reader, writer, dreamer, admin, operator | accepted design | the role set, what each may do, and how the first key comes to exist |
| [ADR-0011](ADR-0011-simulations.md) | Simulations are a fenced provenance channel with taint propagation | accepted design | the fence, the audience, indelible provenance, taint and its end-of-simulation demotion |
| [ADR-0012](ADR-0012-admission-and-review.md) | Admission and review: external crews, serve binding, policy objects | accepted design | the pipeline, serve_id binding, review policy objects, fail-closed review |
| [ADR-0013](ADR-0013-reads-served-from-postgres.md) | Reads are served from Postgres, journalled: one serving tier, a logged read path, steering ahead of hits | accepted design | the serving tier, the read log, steering before hits, what v1 does not answer |
| [ADR-0014](ADR-0014-http-api-and-mcp-facade.md) | One versioned JSON HTTP API; MCP streamable HTTP as a stateless facade | accepted design | the /v1 contract, the agent-safe MCP tool subset, idempotency semantics |
| [ADR-0015](ADR-0015-cli-administration.md) | The CLI is the administration surface; a web console is declined | accepted design | no web UI; the ceremonies the `oml` CLI owns |
| [ADR-0016](ADR-0016-kubernetes-workloads.md) | Kubernetes workloads, two Helm profiles, deployment identity replaces volume identity | accepted design | the workload set, the local and production profiles, nothing naming a machine |
| [ADR-0017](ADR-0017-reconciliation.md) | Reconciliation is an incremental, journalled fold with a closed finding vocabulary | accepted design | the journal as fold source, the finding classes, journalled repair |
| [ADR-0018](ADR-0018-the-documentation-set.md) | The documentation set: tiers, honesty discipline, llms.txt, frontmatter | accepted design | the tier boundary, the writing rules, the fixed vocabulary |
| [ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) | Hexagonal architecture and plugin modularity | accepted design | the pure domain core, ports as traits, thin binary compositions, the plugin points and their law |
| [ADR-0020](ADR-0020-dataset-onboarding.md) | Dataset onboarding: the import manifest | accepted design — post-v1 | the manifest as the reviewed claim at dataset grain, seeded sampling review, one chain event per import, the per-manifest wrapping-key tier, the resurrection fence |
| [ADR-0021](ADR-0021-ai-human-ontologies.md) | AI+Human ontologies: vocabularies as governed claims | accepted design | terms as claims with permanent CURIEs, releases as journalled pins with a materialized closure, SSSOM mappings, the vocab scanner, policy-placed human verdicts over an issuance-time attestation |
| [ADR-0022](ADR-0022-two-ring-api.md) | The two-ring API: v1 memory provider, v2 memory lab | accepted design | one keyed listener, two stability rings; the v1 compatibility covenant (FR-21); the target workarea from the token with `X-OML-Workarea` as the plain override; the token/header/body attribute-placement rule |

## Format

Every record has the same shape, and a record that drops a section is a defect:

- an H1 of the form `ADR-NNNN — title`, then one **Status:** line (`accepted design` with its
  date — a fact about the decision, not the code; nothing in this register is built);
- **Context** — why the decision had to be taken, kept short;
- **Decision** — numbered `d1`…`dn`, one decision per number, so later records can amend by
  handle;
- **Consequences** — what the decision commits downstream pages and the build to, including
  the FR/RS rows it opens in the [roadmap](../product/roadmap.md);
- **Rejected alternatives** — the roads not taken, each with the reason it lost (present on
  every record that weighed alternatives; ADR-0001 and ADR-0018 carry none).
