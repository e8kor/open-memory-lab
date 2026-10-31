<!--
id: adr/index
tier: engineering
status: design draft for review · decided, not built
answers: which decision records exist, what each decides, and which page owns which decision
-->

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
| [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) | Three install profiles, one conformance bar — and cluster mode | accepted design | standalone / docker / kubernetes with one conformance suite (FR-22); the software keyring's honest custody collapse; lead/follower cluster mode, lead-only by default, the lease never load-bearing |
| [ADR-0024](ADR-0024-performance-foundation.md) | The performance foundation | accepted design | the hot-path discipline (only hash+sign+one insert inside the chain lock), the DEK serving cache as a copy class, batch submit as the throughput lever, latency targets as FR-25 rows, the measured-not-assumed stack registry |
| [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) | One key writes and recalls through grants; the agent-only memory surface | accepted design | grant admissibility and the refusal order; issuance with grants and denials; own = the authoring key; include_own_held; the author-fenced held index; budget tokens (oml-tc/1) and max_tokens truncation; applicable steering and the total rank order; read-log content; the tip moves at ratification; a self-signed version never replaces a reviewed tip; the claim_key release and the colliding reopen; retirement by supersession; oml_get and oml_supersede (twelve tools); the own-work listing; the workarea self-view and permission epoch; the filtered events tail; strict bodies |
| [ADR-0028](ADR-0028-agent-key-issuance.md) | The CLI issues an agent's key in one call with its reader grant by default, and an admin skill runs it without the key value entering a transcript | accepted design | `oml keys issue-agent`, one `POST /v2/keys` printing the `OML_URL` and `OML_API_KEY` lines, and its `--out` key file; the CLI's reader grant beside a writer grant and `--write-only`; the `oml-admin` plugin, which runs `issue-agent --out` |
| [ADR-0029](ADR-0029-session-start-hook.md) | The agent plugin starts every session from what OML holds for its key, through `oml hook session-start` | accepted design | `oml hook session-start`, on by default and off with `OML_SESSION_CONTEXT=off`; its two provider-ring reads, the own-work listing and a budgeted recall with own held rows; the working directory's words as the query; `OML_URL` and `OML_API_KEY` alone, never the stored admin credentials; failing open; the injected context; the plugin's guarded SessionStart hook; the permission rule the user adds |
| [ADR-0034](ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) | The lead migrates as an owner role and serves as a role that owns nothing; `_sqlx_migrations` is the one table without row-level security | proposed | `FORCE` fences table reads and the owner/serving split fences the `pg_stats` view and DDL, not EXPLAIN's row estimate; the two roles, their privileges and the default privileges of later tables; migrating under `postgres.owner_url` and serving under `postgres.url`; one URL refused at startup as `postgres.role_split`; the `_sqlx_migrations` exemption (amends ADR-0007 d2); the single-role upgrade and the live host's entry, not run; the per-tenant FTS index, `memories` list-partitioned by `tenant_id` with partitions the owner creates at tenant creation, and what it leaves open (FR-45); foreign keys carrying `tenant_id`; the checks, the `rls-spike` change and the build order after iteration 3 |

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
| D6 | Tenancy is first-class: tenant derived server-side from the key, RLS on every table, least-privilege privileged services, quotas as journalled grant attributes | [architecture/security.md](../architecture/security.md) | [ADR-0007](ADR-0007-tenancy-first-class.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D7 | The workarea algebra: one universe tree per tenant, node-id grants attenuated by denies with deny-wins, labelled edges; universe refuses direct claim writes | [architecture/workareas.md](../architecture/workareas.md) | [ADR-0008](ADR-0008-workarea-algebra.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D8 | The status ladder — development → committed → validated — moves forward only; validated is frozen, and transitions need a proposer and a separate ratifier | [architecture/workareas.md](../architecture/workareas.md) | [ADR-0009](ADR-0009-status-ladder.md) |
| D9 | Roles are reader, writer, dreamer, admin, operator, each under a per-role verb ceiling; admin is never a reviewer, and issuance never amplifies authority | [architecture/roles-and-keys.md](../architecture/roles-and-keys.md) | [ADR-0010](ADR-0010-roles-and-keys.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D10 | Simulations are a fenced provenance channel: indelible provenance, audience-scoped visibility, taint along the lineage closure, overlay tips never the default tip, end-of-simulation demotion | [architecture/simulations.md](../architecture/simulations.md) | [ADR-0011](ADR-0011-simulations.md) |
| D11 | Admission is capture → deterministic scan (veto, never promote) → held → non-author review → ratification under a journalled policy snapshot; it fails closed by holding | [architecture/admission.md](../architecture/admission.md) | [ADR-0012](ADR-0012-admission-and-review.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D12 | Reads are served from Postgres only and journalled; steering precedes hits unranked, and reads fail closed as absence | [architecture/retrieval.md](../architecture/retrieval.md) | [ADR-0013](ADR-0013-reads-served-from-postgres.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D13 | One versioned, two-ring JSON HTTP surface with a closed fail-closed error taxonomy and idempotency by construction; MCP is a stateless facade over it | [api/http-api.md](../api/http-api.md) | [ADR-0014](ADR-0014-http-api-and-mcp-facade.md), [ADR-0022](ADR-0022-two-ring-api.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) |
| D14 | The CLI is the administration surface — its verb set, the show-once key ceremony, the human review exception path; a web console is declined | [cli/reference.md](../cli/reference.md) | [ADR-0015](ADR-0015-cli-administration.md), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0028](ADR-0028-agent-key-issuance.md), [ADR-0029](ADR-0029-session-start-hook.md) |
| D15 | What runs where: the Kubernetes workload set and replica counts, two Helm profiles, deployment identity in place of volume identity | [operations/deployment.md](../operations/deployment.md) | [ADR-0016](ADR-0016-kubernetes-workloads.md), [ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) |
| D16 | Reconciliation is an incremental, journalled fold of the journal with a closed finding vocabulary; every repair travels through journalled channels | [architecture/reconciliation.md](../architecture/reconciliation.md) | [ADR-0017](ADR-0017-reconciliation.md) |
| D17 | Measured, not assumed: no page states a number for an unmeasured quantity, and every unmeasured or unverified claim resolves to exactly one roadmap row with an exit criterion | [product/roadmap.md](../product/roadmap.md) | [ADR-0018](ADR-0018-the-documentation-set.md) d3, [ADR-0024](ADR-0024-performance-foundation.md) |

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
