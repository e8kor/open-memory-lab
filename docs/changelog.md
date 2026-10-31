<!--
id: changelog
tier: engineering
status: design draft for review · decided, not built
answers: how the design spec is versioned, what 0.1.0-design, 0.2.0-design and 0.3.0-design cover, and what each revision changed
-->

# Spec changelog

**Status:** design draft for review · decided, not built

This page is the revision record of the design specification itself. The current spec version
is **0.3.0-design**, matching `info.version` in the [OpenAPI document](api/openapi.yaml). Until
2026-10-30 no code existed; since the PoC entry below, an entry may also record an
implementation milestone where it changes what the repository holds.

## How this changelog is kept

- **One entry per design PR.** A merged design change adds one dated entry naming the ADRs and
  pages it touched; a change with no entry is a defect.
- **`info.version` bumps on any /v1-surface change.** A change to the provider ring's
  contract — paths, request or response shapes, the error taxonomy, idempotency or pagination
  semantics — bumps the version in `docs/api/openapi.yaml`, and this page records the bump
  alongside the change that forced it.
- **The repo is tagged `spec-vX.Y` at each published revision whose records are accepted**,
  starting with `spec-v0.1` at implementation start, so an implementation can state exactly which
  revision of the design it was built against; a revision carrying a `proposed` record is
  tagged when that record is accepted.

## 0.3.0-design

### 2026-10-31 — Names as keys (implementation)

An implementation milestone; no decision changes. The code now serves what the proposed
[ADR-0026](adr/ADR-0026-names-are-keys.md) decides on every verb this build serves, except what is
listed as unbuilt below, which also names the verbs ADR-0026 reaches that this build does not
serve. The record stays proposed: no Status line moves, `info.version` stays `0.3.0-design`, no
`spec-v0.3` tag is cut, and no package version moves until the release.

- **Classification at parse** (d2, d3). Every workarea, crew, key and tenant reference is an id in
  the RFC 9562 hex-and-dash form whose version digit is not `8`, or a name under the reference
  grammar, the derived form included; anything else is `request.validation` with `details.field`
  naming the field, so a path and an uppercase name are refused, never lowercased. A creation
  refuses the derived form, and a name that names no row is replaced by its version-8 unresolved
  value.
- **Storage** (d1, d5, d6, d10). Migration `0006_names.sql` adds the `crews` table and the `name`
  columns with their unique indexes, keeps the PoC's `tenants.name` and `workareas.name` as
  `prior_name`, ties `universe` to the root with a CHECK, and names nothing. The unique index is
  `name.taken`'s authority, so a creation that loses a race journals nothing, and a revoked key
  keeps its name, which still resolves; the one revocation this build journals is the server's
  own, of an issuance that failed after its `key.issued`.
- **Resolution and crews** (d4, d5, d8, d9, d15). Every use case resolves the workareas, crews and
  keys it is given after the verb ceiling and before any read, so an unknown or unreadable name
  answers `access.not_found` in the same step, with the same code and details, as its id would.
  Admin keys create, read and update crew records, journalled as `crew.created` and `crew.updated`;
  `workarea.create` and `workarea.crew_updated` journal `crew_id`, and `key.issued` journals the
  key's `name` and `crew_id`. The bootstrap claim and `POST /v2/keys` refuse a body with no name,
  or one the creation grammar refuses, as `request.validation` with `details.field` `/name`, the
  claim only after its enrollment token. An admin key reads workarea structure across its tenant by
  key role, the workarea read listing every child with `my_effective_verbs` empty, and still
  answers `access.verb_ceiling` on every claim read; a global denial on an admin key is refused.
- **The lead's start** (d11, d15). Before it serves, the lead names every row that predates names,
  in one `system.names_assigned` per chain: the root `universe`, a usable creation-time name kept,
  every other row in the derived form, and each distinct crew object one crew record. In the same
  commit it lifts each global denial on an admin key, one `key.denial_lifted` apiece with
  `reason_digest` over `adr-0026-d15`, folded into `key_denials.lifted_epoch`. A second start
  journals nothing; a lead that leaves a row unnamed reports `names_assigned` not ready on
  `/readyz` and answers every other route `system.unavailable`.
- **HTTP and MCP** (d4, d5, d8, d9, d15). Every header, path parameter, query parameter and body
  field the transport takes for a workarea, crew or key takes its id or its name, among them
  `X-OML-Workarea`, `{workarea_id}`, `{key_id}`, the listing's `workarea_id`, `parent_id`, a
  grant's `workarea_node_id`, a denial's `subtree_node_id`, `default_workarea_id` and `crew`.
  `name.taken` answers 409; `GET` and `POST /v2/crews`, `GET /v2/crews/{crew_id}`,
  `POST /v2/crews/{crew_id}/update`, `POST /v2/workareas/{workarea_id}/crew` and
  `GET /v2/workareas/{workarea_id}/tree` are served, the tree a page at a time under a cursor
  bound to the key, an admin key reading every node by its key role and a reader key every node
  within its read access; workarea responses carry `name` and `crew`, the crew record or null, and
  key responses `name`; and `GET /v2/keys/{key_id}` answers a key by its id or its name. A
  `workarea` on `POST /v2/review/next`, by id or by name, is refused as `request.validation` with
  `details.field` `/workarea`, never ignored, because the queue filter is not built and the queue
  is served whole. The served entries of [mcp-tools.json](api/mcp-tools.json) take a workarea's
  name and their `_meta.pending_design` notes for names are gone, and `oml_workarea_info` answers
  `crew` null for a workarea that names none.
- **CLI** (d2, d3, d4, d8, d12, d15). Every argument that names a workarea, crew, key or tenant
  takes its id or its name, sent as typed for the server to resolve, as one percent-encoded segment
  where the route takes it in its path; one a path carries that is empty, `.` or `..` is refused
  before anything is sent, exit `2`, since no path carries it as one segment. Among them are the
  `--grant <workarea>:<role>`, `--default-workarea`, `--workarea` and `--crew` of
  `oml keys issue <key-name>` and `oml keys issue-agent <key-name>`, `oml keys show <key>`,
  `oml deny set <key> <subtree>`, `oml submit --workarea`, `oml recall --workarea` and
  `oml audit verify-chain --tenant`. `oml keys issue-agent` keeps
  [ADR-0028](adr/ADR-0028-agent-key-issuance.md) d1's `--out`, d2's reader grant and `--write-only`;
  `oml crews create`, `update`, `list` and `show` administer crew records;
  `oml workareas create <parent> <name>` takes `--crew`, and `oml workareas crew set <workarea>
  --crew <crew>` points a workarea at another crew record; `oml workareas show` and `tree` run
  under an admin context, `tree` reading every page of the tree read; and the human layout of
  `oml keys show` prints a `revoked` line, the key's revocation time or `no (live)`.
- **Plugins** (d14). The `oml-admin` skill asks for the key's name and the workarea's name and
  passes them to `issue-agent`, with `--crew` when the operator names a crew, still only with
  `--out` and never with `--json`. The `oml` plugin's README and skill say that a workarea
  argument takes the workarea's name or its id, and the [agent plugin](api/mcp-plugin.md) page's
  `issue-agent` synopsis carries the key name. `scripts/check_plugin.py` now also fails if the
  skill's `issue-agent` run does not name its key right after the verb with a name the creation
  grammar admits, or if a command on a plugin page passes `--workarea` anything but a workarea
  name or `<workarea>`.
- **SDKs and the chart** (d1, d10, d12). Both generated layers of the [SDKs](engineering/sdks.md)
  are regenerated from this `openapi.yaml`, as their regeneration rule asks; both wrappers raise
  `NamingError` for the `name` family, named so beside Python's `NameError`; and both test harnesses
  name the keys they claim and issue. The Helm chart's install notes and the
  [deployment](operations/deployment.md) page's claim through a port-forward run
  `oml bootstrap claim <key-name>`.
- **Checks:** `crates/oml-domain/tests/names.rs`, the port contracts of
  `testing/oml-port-contracts`, which both stores run, `crates/adapters/oml-pg/tests/pg_store.rs`,
  `crates/oml-app/tests/use_cases.rs`, `crates/bins/oml-server-lead/tests/http_api.rs` and
  `mcp_facade.rs`, and `crates/bins/oml-cli/tests/cli_e2e.rs` run in `cargo test --workspace`.
- **Still unbuilt.** The verbs ADR-0026 reaches that this build does not serve, so no name reaches
  them either: key revocation (`POST /v2/keys/{key_id}/revoke`, `oml keys revoke`) and rotation, and
  so `new_name` on `key.rotated` and `oml keys rotate <key> <new-key-name>`; the key listing
  (`GET /v2/keys`) and `oml keys list`, so d10's revoked key is read as history only through
  `GET /v2/keys/{key_id}` and `oml keys show`; of the other verbs d9 addresses by the key's name,
  adding or revoking a grant after issuance, lifting a denial and the effective-access read; d15's
  lookup by path (`GET /v2/workareas?path=`) and its `edges` and `status` reads under
  `GET /v2/workareas/{workarea_id}`; the re-parent's `new_parent_id`, a workarea edge's
  `to_workarea_id` and the reassign's `workarea_id`; the queue filter, `workarea` on
  `POST /v2/review/next`, refused by name above; and a simulation audience's `key_id`. The
  operator tier is unbuilt, and so are tenant names beyond the one tenant the bootstrap claim
  creates, operator keys and the provider's key namespace, `tenant_id` on issuance and
  `POST /v2/tenants`. So are the independence axis's crew and manager comparisons at promotion,
  whose domain functions exist and which no promotion calls yet. Two pieces differ from the design
  because the operator tier is unbuilt: the PoC has no provider chain, so each tenant's name is
  journalled in its own chain's `system.names_assigned`; and the bootstrap claim mints the tenant's
  first admin key, so its `name` is checked in that tenant's namespace, not the provider's. Two
  listings are ordered otherwise than the [pagination](api/http-api.md#pagination) convention's
  `created_at` descending: the tree is served in path order, a node before its children and
  siblings by name in byte order, its cursor carrying the last path served, where newest first
  would list a node's children before it; and the crew listing is served in name order.
- **Pages touched:** this page and the [agent plugin](api/mcp-plugin.md) (the `issue-agent`
  synopsis); [mcp-tools.json](api/mcp-tools.json) and the [CLI reference](cli/reference.md)
  change with the code that serves them; the [runbooks](operations/runbooks.md) spell every key
  argument `<key>`, as the CLI reference does; outside `docs/`, `README.md` (the honest status),
  `plugins/oml/README.md`, `plugins/oml/skills/oml-memory/SKILL.md`,
  `plugins/oml-admin/skills/issue-agent-key/SKILL.md` and `scripts/check_plugin.py`; and, with
  the SDKs and the chart, [SDKs](engineering/sdks.md), [deployment](operations/deployment.md),
  `sdks/` and `deploy/helm/oml/templates/NOTES.txt`.

### 2026-10-30 — names as keys (ADR-0026)

[ADR-0026](adr/ADR-0026-names-are-keys.md), accepted by the maintainer on 2026-10-31 with release v0.3.0, gives workareas, crews, keys and tenants a
name that is a unique, immutable key beside the id. It proposes amending ADR-0008 (d3, d7), ADR-0010
(d6, d7), ADR-0012 (d4), ADR-0018 (d4), ADR-0022 (d4), ADR-0025 (d1, d2, d14) and ADR-0028 (d1, d3);
each of those records' Status lines gains the back-pointer when ADR-0026 is accepted.

- **Decided:** one flat namespace per kind — the tenant for workareas, crews and keys (operator keys
  share the provider's), the deployment for tenants — with names never changed and never released,
  and the reserved provider tenant carrying none; a lowercase RFC 1035 label as the grammar,
  refusing every spelling that could read as an id, with no case mapping, and the derived form of
  rows that predate names refused at creation and accepted in a reference; the ids of the four kinds
  fixed as RFC 9562 hex-and-dash UUID text, minted as version 4 and never version 8, so a value in
  that form is always an id; names accepted wherever an id is — `/v1` and `/v2` path parameters,
  query parameters and body fields, `X-OML-Workarea`, MCP tool arguments and CLI arguments — and
  resolved on the server by substituting the id after the verb ceiling, in a namespace fixed by the
  caller and the kind, so an unknown or unreadable name answers exactly what its id would, with no
  refusal naming an id a name resolved to beyond what the naming rule already discloses, and an
  unknown name takes a deterministic version-8 unresolved value of its own that shares no
  idempotency scope; the request digest over paths and bodies as sent, with `X-OML-Workarea` outside
  it and resolved into the provider ring's record scope; `name.taken` (409) for a taken name at
  creation, which only admin and operator keys, and the holder of a valid enrollment token at the
  bootstrap claim, can receive; an operator key's operator keys issued without `tenant_id` and named
  in the provider's namespace, and its first admin key for a tenant refused once that tenant has
  held any key; the root named `universe` in every tenant; paths derived from names, served and
  looked up but never accepted as a reference; crews as named records that workareas and keys
  reference, created and updated by admins only, with the independence axis reading a key's crew and
  failing closed where none is named; a key's name distinct from its principal and label, and a
  rotation's successor named afresh; the naming of rows that predate names by a deterministic rule
  journalled as `system.names_assigned`, which lists kept names too, read before the creation events
  it names by a re-fold, with the PoC's name columns kept as `prior_name`, every pre-names denial on
  an admin key lifted in the same commit, and followers not ready until the event is folded; the
  key's name as the positional argument of `oml keys issue-agent` and its crew as `--crew`, and
  every creating CLI verb's name positional, `oml bootstrap claim` included (d12), while the verb,
  its `--out` key file, reader grant default and `oml-admin` plugin moved to
  [ADR-0028](adr/ADR-0028-agent-key-issuance.md) so they ship before names (d13 and d14 point
  there); and admin keys reading the structure of every workarea in their tenant without grants —
  the workarea read, the lookup by path, the tree, edges and status — and never claim content, with
  no global denial on an admin key, declared in the roles-and-keys permission matrix (d15).
- **Surface:** the `name` error family with `name.taken` (errors.md, the OpenAPI `ErrorCode` enum
  and http-api.md's tables together); `GET /v2/crews`, `POST /v2/crews`, `GET /v2/crews/{crew_id}`
  and `POST /v2/crews/{crew_id}/update`; the `crew.created`, `crew.updated` and
  `system.names_assigned` events; `crew_id` in place of the crew object on `workarea.create` and
  `workarea.crew_updated`; `name` and `crew_id` on `key.issued`, `new_name` on `key.rotated`,
  `reason_digest` on `key.denial_lifted`; the `oml_name` domain, the `tenants` and `crews` tables
  and the `name` columns with their unique indexes, and the PoC's name columns kept as `prior_name`;
  the OpenAPI `Id` refusing version 8; the OpenAPI `NameRef` schema for references beside `Name` for
  creations; `name` and `crew_id` on the lake's `gold.areas` in place of its three crew columns.
  Example ids of the four kinds are rewritten to UUID text on the OpenAPI document, the agent
  plugin, the reviewer-crews pages and the CLI sessions; the other example ids are untouched.
- **`info.version` 0.2.0-design → 0.3.0-design**, because `/v1` changes on the request side:
  `X-OML-Workarea` and the listing's `workarea_id` accept names, and a value that is neither an id
  nor a name, which 0.2.0-design answered `access.not_found`, now answers `request.validation`. That
  narrowing is permitted only because `/v1` is not frozen (FR-21). The 0.2.0-design revision was
  tagged `spec-v0.2` when ADR-0025 was accepted, so this record cannot share its bump; `spec-v0.3`
  is cut when ADR-0026 is accepted.
- **Pages touched:** [ADR-0026](adr/ADR-0026-names-are-keys.md), the
  [decision register](adr/index.md) (row and the D7, D9, D13 and D14 cells),
  [workareas](architecture/workareas.md), [roles and keys](architecture/roles-and-keys.md),
  [concepts](concepts.md), [HTTP API](api/http-api.md), [OpenAPI](api/openapi.yaml),
  [errors](api/errors.md), [mcp-tools.json](api/mcp-tools.json) (unserved tools' descriptions and
  `_meta.pending_design`; the tool count is unchanged), [agent plugin](api/mcp-plugin.md),
  [reviewer crews](api/reviewer-crews.md) (example ids), [PostgreSQL schema](data/postgres-schema.md),
  [event catalog](data/event-catalog.md), [Delta tables](data/delta-tables.md) (`gold.areas`),
  [CLI reference](cli/reference.md),
  [CLI sessions](cli/sessions.md), [deployment](operations/deployment.md) and
  [runbooks](operations/runbooks.md) (the CLI invocations whose arguments changed),
  [observability](operations/observability.md) (the `names_assigned` doctor line),
  [llms.txt](llms.txt), this changelog, `mkdocs.yml` and `CONTRIBUTING.md` (the vocabulary row);
  the spec version also on the [home page](index.md),
  [CI and toolchain](engineering/ci-and-toolchain.md) and [upgrades](operations/upgrades.md),
  which also states that the upgrade adding names stops every earlier binary before it migrates.
- **Roadmap:** no row opens or closes. Name resolution's request-path cost is unmeasured and is
  tracked against FR-25.

## 0.2.0-design

### 2026-10-31 — M0 spikes: the Rust tests' copies of the vectors are compared with the page, S3.2 is two findings, and EXPLAIN's estimate is a recorded cross-tenant disclosure

A second review fix to the M0 spike findings and harnesses. No decision changes, and no record is
amended or proposed.

- **Chain vectors:** the M0 spikes page said the repository held no second copy of the vectors.
  It holds three: the Rust tests in `crates/oml-domain` and `crates/bins/oml-cli` keep the
  vectors as literals, and each Rust test checks the code against its own copy, so until now the
  copies agreed with the page only because they had been copied by hand.
  `scripts/check_chain_vectors.py` now finds every Rust file that copies the vectors and compares
  each copy with the page. It reads `crates/` and changes nothing there. Run from a scratch tree,
  the check as first committed passed with a drifted copy. As it stands, it fails on six drifted
  copies and passes on the repository's three files (6 `signed_bytes` literals, 34 hex values).
  Having the Rust tests read the page is a follow-up for `feat/iteration-3`
  ([finding](engineering/spikes/chain-vectors.md#the-rust-tests-copies)).
- **S3.2:** S3.2 is now two findings. The first is PostgreSQL's mechanism: a non-owner role
  without `BYPASSRLS` stays inside the policy, and migrations succeed under the owner. It can
  pass in CI. Its controls are the refused DDL taking effect for the owner, rolled back, and the
  service role's rows under tenant B. The `BYPASSRLS` control role is gone. The second is the
  claim about OML's own roles. It is **inconclusive, not testable** on every host, because the
  harness's roles are its own and no page specifies a deployment's roles and grants, so a CI run
  never prints `pass` and `none` for it. The harness's role attributes are now preconditions,
  not checks ([findings](engineering/spikes/spike-3-rls.md#s32)).
- **S3.1 and S3.4:** the page claimed that the role split confines the statistics leak to the
  owner. Its own S3.4 observation contradicted that. EXPLAIN's row estimate under tenant A follows
  tenant B's words, and by PostgreSQL's documentation and the `REL_17_STABLE` source it does so for
  every role the policy binds. The estimate is now the check `S3.4/<role>/estimate`. The owner's
  failure is recorded. The service role's is not, so CI fails if it leaks. Both findings now also
  cite ADR-0007 d4 and security.md's STRIDE information-disclosure mitigation ("no cross-tenant
  index to leak through"), which the shared index `memories_claim_fts` breaks. Their action is
  ADR amendment needed, or an implementation fix that makes the FTS index per tenant, awaiting
  the maintainer. The designed `memories_fts` and `held_fts` are not partitioned per tenant
  either.
- **Harness fix found on the way:** the owner's `UPDATE journal` observation ran with no tenant
  set, matched no row, and could not show the guard trigger. It now runs under tenant A.
- **Pages touched:** [M0 spikes](engineering/spikes/index.md),
  [chain vectors](engineering/spikes/chain-vectors.md), [spike 3](engineering/spikes/spike-3-rls.md),
  [critical path](product/critical-path.md) (M0 progress),
  [CI and toolchain](engineering/ci-and-toolchain.md) (the gate row), `docs/llms.txt` and this
  page. Outside `docs/`, the two harnesses.
- **Not verified here:** any pg17 or service-role result, S3.2's mechanism among them. CI jobs
  `chain-vectors` and `rls-spike` have not run. The S3.2 code path was run only with the owner
  standing in for the service role, which the preconditions reported as not as minted.

### 2026-10-31 — M0 spike 3: a failed scenario says ADR amendment needed, the lead run is recorded, and the harness follows new migrations

A review fix to the spike 3 findings and harness of the entry below. No decision changes, and no
record is amended or proposed.

- **Findings:** S3.1 and S3.4 now say **ADR amendment needed** against ADR-0007 d2 and
  security.md's `FORCE` commitment, awaiting the maintainer's decision. The earlier wording said
  no amendment was needed and that the maintainer had confirmed this, and the maintainer had not.
  `_sqlx_migrations`, which has no RLS, no `FORCE` and no policy, is recorded as ADR amendment
  needed (ADR-0007 d2) or an implementation fix. The run of `oml-server-lead` 0.2.0 behind both
  claims is now on the page: the command, the configuration shape and the catalog output, which
  show the serving role owning all 12 tables
  ([findings](engineering/spikes/spike-3-rls.md)).
- **Harness:** `testing/spikes/rls/spike_rls.py` prints the same action for a failure. It records
  which tables its seeders write. A table that a later migration adds and no seeder writes is
  reported as unprobed for row visibility, and it does not fail the run. The no-tenant write and
  the probe function no longer depend on the 0001–0005 column shapes. Run on PostgreSQL 18.6 with
  `feat/iteration-3`'s `0006_names.sql` added from scratch, the harness as first committed failed
  S3.1 (`crews` empty) and S3.3 (`23514`). As fixed, it gives the 0001–0005 verdicts and reports
  `crews` as unprobed. It still gives the 0001–0005 verdicts on this branch's migrations.
- **Pages touched:** [M0 spikes](engineering/spikes/index.md) (how the harness follows the
  migrations, and two more rejected alternatives), [spike 3](engineering/spikes/spike-3-rls.md),
  [critical path](product/critical-path.md) (M0 progress) and this page. Outside `docs/`, the
  harness.
- **Not verified here:** any pg17 or service-role result. CI job `rls-spike` has not run.

### 2026-10-31 — M0: the chain-format vectors reproduce outside Rust, and spike 3 runs on PostgreSQL 18.6

An M0 change: two of the critical path's de-risking experiments are built and run, and their
findings are recorded. No decision changes, no record is amended or proposed, and the harness
decisions live on an engineering page, [M0 spikes](engineering/spikes/index.md).

- **Built:** `scripts/check_chain_vectors.py`, which rebuilds the three
  [chain format](data/chain-format.md) vectors from their inputs with two RFC 8785
  implementations (`rfc8785`, `jcs`), two Ed25519 implementations (`cryptography`, PyNaCl) and
  two SHA-256 implementations (`hashlib`, libsodium). It runs the verification procedure and its
  mutation claims, and fails on any difference. Also built: `testing/spikes/rls/spike_rls.py`,
  which applies `migrations/*.sql` to a throwaway database and asserts S3.1–S3.4. Both pin their
  libraries through uv, inline and in a committed lock file. `.github/workflows/m0-spikes.yml`
  adds the jobs `chain-vectors` and `rls-spike`, the latter on `pgvector/pgvector:pg17`.
- **Found:** the vectors reproduce byte for byte
  ([finding](engineering/spikes/chain-vectors.md)). On PostgreSQL 18.6, as the table owner, every
  read shape of the fold tables stayed inside its tenant. A read with no tenant set returns zero
  rows, which is now the recorded behaviour. `pg_stats` shows the owner other tenants' lexemes
  through the expression index `memories_claim_fts`, so S3.1 and S3.4 fail for the owner, and the
  harness records both as expected failures. Under RLS that GIN index serves no FTS plan, since
  `@@` is not leakproof. S3.2 did not run, for lack of a role that can create roles
  ([findings](engineering/spikes/spike-3-rls.md)).
- **Page edit:** chain format now says that omitting the last event fails step 9, against the
  stated head, and not step 1 or 5. The check's first run failed on the old sentence.
- **Not verified here:** any result on the pinned pg17, any result for a non-owner role, and
  S3.2. The two CI jobs have not run. Spikes 1, 2 and 4 are not run.
- **No surface change:** no HTTP operation, error code, event type or claim state changes, and
  `info.version` stays `0.2.0-design`.
- **Pages touched:** [M0 spikes](engineering/spikes/index.md) and its two finding pages (new),
  [chain format](data/chain-format.md), [critical path](product/critical-path.md) (M0
  progress), [CI and toolchain](engineering/ci-and-toolchain.md) (the M0 spikes gate),
  `mkdocs.yml`, [llms.txt](llms.txt) and this page. Outside `docs/`, the two scripts, their lock
  files and the workflow.

### 2026-10-31 — A Helm chart for the current build's one lead (ADR-0033)

[ADR-0033](adr/ADR-0033-interim-helm-chart.md), proposed on 2026-10-31, decides what a Helm
chart holds while the build has one server binary, and this change builds it. Numbers 0030 to
0032 are taken by records on other branches. It amends ADR-0016 (d4) and ADR-0023 (d16) for this
chart only; their Status lines gain the back-pointer when the record is accepted.

- **Decided and built:** `deploy/helm/oml`, the first revision of the one chart ADR-0016 d2
  names, holding `oml-server-lead` alone (d1): one replica under `Recreate` in place of the lead
  lease, with the chain lock as the fence (d2); the keyring on a ReadWriteOnce claim mounted at
  `/var/lib/oml/keyring`, kept by `helm uninstall` and reattachable through
  `keyring.existingClaim` (d3); uid and gid 999, the image's `oml` user, with `fsGroup: 999` under
  `fsGroupChangePolicy: OnRootMismatch`, because the default policy turns the 0600 keyring into
  0660 and the lead refuses it (d4); `runAsNonRoot`, a read-only root filesystem, every capability
  dropped, seccomp `RuntimeDefault`, no service account token, resources as comments, and an
  optional NetworkPolicy (d5); `OML_POSTGRES__URL` from an existing Secret's key, never a value
  (d6); startup and liveness probes on `/healthz` and readiness on `/readyz` (d7); the Ingress
  off by default, and install notes that give the claim of the enrollment token through
  `kubectl port-forward` before any Ingress, and warn when an install enables one (d8);
  `version` = `appVersion` = the workspace version, the image
  `ghcr.io/e8kor/open-memory-lab:<appVersion>`, pinned by `image.digest` when set (d9); and the
  release and CI jobs (d10).
- **Checks:** `scripts/check_chart.py` runs `helm lint --strict` and `helm template` on the
  default values, `ci/kind-values.yaml` and the optional features, and fails on a version
  mismatch, a duplicated mapping key, or the loss of any property d2–d8 decide, NOTES.txt's
  included; with `--image-user` it also requires the pod's ids to be the built image's.
  `scripts/check_release.py` now also fails if `release.yml` stops checking the chart's version
  against the tag, packaging it, signing it with `cosign sign-blob` or uploading the archive and
  its bundle to the release. `ci.yml` gains job `chart`: kind, `pgvector/pgvector:pg17` in the
  cluster, the image built from the checkout and loaded, `helm install`, the claim through
  port-forward, `scripts/quickstart.sh`, then a restart that must keep the public key, answer
  `/readyz` 200 and print no token, and an uninstall that must leave the keyring claim and a
  reinstall that must load the same key. `release.yml` gains jobs `chart` and `chart-release`; its
  other jobs are unchanged.
- **Not run here:** the kind job and the release jobs. This change ran `helm lint`,
  `helm template`, a strict Kubernetes schema validation of the rendered objects, both checkers
  and `actionlint` locally, and the quickstart against the installed lead the way the kind job
  drives it; the pull request's CI run is the kind job's first, and the first tag after it is the
  release jobs' first. d4's fsGroup behaviour is read from the kubelet's source and the
  Kubernetes documentation, not observed on a cluster.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`. The chart is at the workspace version, 0.2.2, and moves with it.
- **Pages touched:** [ADR-0033](adr/ADR-0033-interim-helm-chart.md), the
  [decision register](adr/index.md) (row and the D15 cell), `mkdocs.yml`, [llms.txt](llms.txt),
  [deployment](operations/deployment.md) (the interim chart's section and the keyring backup
  row), [CI and toolchain](engineering/ci-and-toolchain.md) (the chart gate and the release
  artifact) and this page; outside `docs/`, `deploy/helm/oml/`, `deploy/k8s/README.md`,
  `scripts/check_chart.py`, `scripts/check_release.py`, `.github/workflows/ci.yml` and
  `.github/workflows/release.yml`.

### 2026-10-31 — Python and TypeScript clients generated from openapi.yaml (implementation, FR-12)

[Client SDKs](engineering/sdks.md), a new engineering page, decides how the client half of
[FR-12](product/roadmap.md) is made and kept current, and this change builds it. FR-12's batch
half, `POST /v1/memories:batch`, changes the server and is not part of it. No ADR records the
decision: FR-12 already decides that the clients are generated, and the page decides the
generators, the package names, the wrapper and the regeneration rule, with the rejected
alternatives. It amends no record.

- **Decided and built:** `sdks/python`, the distribution `oml-sdk` imported as `oml_sdk`,
  generated by openapi-python-client 0.29.1, and `sdks/typescript`, the npm package `oml-sdk`,
  typed by openapi-typescript 7.13.0 over openapi-fetch 0.17.0, with every generator pinned and
  locked in `uv.lock` and `package-lock.json`; neither is released to a registry. Over each, a
  hand-written wrapper that takes the key from the caller or `OML_API_KEY`, sends one
  `Idempotency-Key` per write and reuses it on every retry of that call, follows `next_cursor`
  through the listing, raises one exception class per [errors.md](api/errors.md) family carrying
  `code`, `rule`, `message` and `retryable`, and refuses uploads with `NotBuiltError`, because the
  server serves no upload sessions yet.
- **The regeneration rule:** `scripts/generate_sdks.py` regenerates both generated layers from
  openapi.yaml, and with `--check` fails on stale generated code or on a change to the Python
  generator's recorded warnings, so a change to openapi.yaml regenerates both clients in the
  same change.
- **Checks:** `sdks/python/tests` (pytest) and `sdks/typescript/test` (`node --test`) run against
  a real `oml-server-lead` on a throwaway database: remember, held, the author's own recall, a
  dreamer key's approve verdict, recall; a write whose response was lost, retried and replayed
  under the same key; a key reused with another body, refused and not retried; the listing
  iterated across pages; the error mapping. `.github/workflows/ci.yml` gains three jobs,
  `sdk-python`, `sdk-typescript` and `sdk-generated`, and changes none.
- **Not verified here:** the three CI jobs, which have not run yet. Locally both suites passed
  against the installed `oml-server-lead` 0.2.0 and `oml` 0.2.2, the Python suite on Python 3.11
  and 3.13 and the TypeScript suite on Node 24.
- **Not built:** the batch write path and the upload wrapper, which keep FR-12 open; and the
  Python generated layer types no `Workarea`, `WorkareaPage` or `WorkareaView`, the generator gap
  the SDK page records.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`.
- **Pages touched:** [client SDKs](engineering/sdks.md), new; [HTTP API](api/http-api.md) and
  [errors](api/errors.md), which link it; the FR-12 status on the [roadmap](product/roadmap.md),
  the M2 progress on the [critical path](product/critical-path.md),
  [CI and toolchain](engineering/ci-and-toolchain.md), `mkdocs.yml`, [llms.txt](llms.txt) and this
  page; outside `docs/`, `README.md`, `sdks/`, `scripts/generate_sdks.py` and
  `.github/workflows/ci.yml`.

### 2026-10-31 — Every agent session starts from what OML holds for its key: `oml hook session-start` (ADR-0029)

[ADR-0029](adr/ADR-0029-session-start-hook.md), accepted by the maintainer on 2026-10-31 with release v0.2.2, records what the maintainer decided on
2026-10-31, and this change builds it: the `oml` plugin ships a SessionStart hook that runs a Rust
subcommand of the `oml` CLI, on by default and bounded by a token budget. Numbers 0026 and 0027
are taken by records on other branches. It amends no record.

- **Decided and built:** `oml hook session-start`, which sends the own-work listing
  (`own=true`, ten newest) and a recall with `include_own_held` under a budget of 800 budget
  tokens (`OML_SESSION_CONTEXT_MAX_TOKENS`, 1 to 2000), queried with the words of the working
  directory's name (d1–d3); `OML_URL` and `OML_API_KEY` alone, never the stored admin credentials
  or `OML_CLI_KEY`, a key value that is not all printable ASCII refused before anything is sent,
  and the key value redacted from everything it prints (d4); no output and exit
  0 on every failure, `OML_SESSION_CONTEXT=off` included, with a 3-second timeout per request, its
  connect included, an 8-second deadline on the whole verb, and a context printed whole or not at
  all (d5); one SessionStart hook document whose context puts the request to record, and after a
  compaction the request to record the summary's durable facts, right after the header and above
  the line that frames the entries below as stored data, and prints claim keys and the path in at
  most 120 characters, the key redacted before the cut (d6);
  `plugins/oml/hooks/hooks.json`, which runs the verb only when `oml` is on `PATH` (d7); and the
  two `permissions.allow` rules in the plugin README, the five read tools and every tool, with the
  cost of the second, writes the user never sees, stated beside it (d8).
- **Checks:** `crates/bins/oml-cli/tests/cli_e2e.rs` runs the verb against the real lead behind a
  request recorder; `scripts/check_plugin.py` now also fails if `plugins/oml/hooks/hooks.json` is
  missing or does not parse, or if its SessionStart hooks are not exactly one command, without a
  matcher, with a 10-second timeout and the status message `Loading OML memory`, that runs
  `oml hook session-start`, exits 0 with no output when `oml` is not on `PATH`, and exits 0 when
  that `oml` fails.
- **Not verified here:** that Claude Code runs the hook and adds its context to a session; the
  tests run the binary, not Claude Code. The [roadmap](product/roadmap.md) opens FR-33 for it,
  which closes on a recorded session on a named Claude Code version, a compaction run included.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`, and no package version moves until the release.
- **Pages touched:** [ADR-0029](adr/ADR-0029-session-start-hook.md), the
  [decision register](adr/index.md) (row and the D14 cell), `mkdocs.yml`, [llms.txt](llms.txt),
  [CLI reference](cli/reference.md), [agent plugin](api/mcp-plugin.md), the
  [roadmap](product/roadmap.md) (FR-33) and this page; outside
  `docs/`, `plugins/oml/README.md`, `plugins/oml/hooks/hooks.json` and
  `scripts/check_plugin.py`.

### 2026-10-31 — The agent-only memory surface (implementation)

An implementation milestone; no decision changes. The code now serves what
[ADR-0025](adr/ADR-0025-the-agent-only-memory-surface.md), accepted on 2026-10-31 with this build,
decides for FR-10 and FR-11:

- **Eight of the twelve tools.** The [agent plugin](api/mcp-plugin.md)'s catalog holds twelve
  tools, and `tools/list` serves eight: `oml_remember`, `oml_learn`, `oml_recall`, `oml_get`,
  `oml_supersede`, `oml_my_work`, `oml_workarea_info` and `oml_events_tail`. `oml_attach`,
  `oml_relate_propose`, `oml_graph` and `oml_vocab` are named and answer not built (d12).
- **One key.** Grant admissibility per key role, the four-step refusal order and effective access
  computed on every verb, so a writer-role key with `wa:writer` and `wa:reader` grants remembers
  and recalls (d1); issuance that carries grants and denies and discloses the value only after
  the last `grant.issued` commits, and `POST /v2/keys/{key_id}/denials` (d2); "own" as the
  authoring key (d3).
- **Reading back, budgets and order.** `include_own_held` on recall (d4); the budget token under
  counter `oml-tc/1` and the `max_tokens` cut with its `budget` object (d6, d7); applicable
  steering ahead of the hits and a total hit order (d8); the ids served and the whole-request
  digest that the read log will record, computed on every recall (d9).
- **Versions.** Each claim version stores its ordinal, kind, NFC `body_bytes` and the version it
  replaces, and a never-promoted rejection releases its `claim_key` (d6, d18); the tip moves only
  at ratification, as one `lifecycle.supersede`, the author's retire-proposal is a supersession
  carrying `retirement: true`, a supersede replays from its stored receipt, and a dreamer's edit
  is a proposal (d10, d11). Migration `0005_claim_versions.sql` removes the PoC's `superseded`
  status.
- **Listings.** The own-work listing on `GET /v1/memories` (d13), the workarea read on
  `GET /v2/workareas/{workarea_id}` (d14), and the events tail's per-family visibility with a
  bounded scan answering `next_after_seq` (d15).
- **The one-key plugin.** `plugins/oml/.mcp.json` declares the `oml` server alone, and
  `scripts/check_plugin.py` fails if a second server appears; `OML_RECALL_KEY` is gone.
  `scripts/quickstart.sh` issues one agent key and a dreamer key; it remembers, lists its own work
  and recalls its own held row on the agent key, records an approving verdict with the dreamer
  key, and recalls the promoted row on the agent key.
- **The scenario.** `crates/bins/oml-server-lead/tests/agent_only_memory.rs` runs in
  `cargo test --workspace`. No CI run has carried it yet, so FR-10 and FR-11 stay open on the
  [roadmap](product/roadmap.md) until one passes; their rows then cite that run (ADR-0018 d3).
- **The multi-arch image** is recorded in the release-image entry below.
- **Bounds the ring already enforced, now declared.** Within 0.2.0-design,
  `RecallRequest.max_tokens` and the budget object's echo of it declare a maximum of 2⁵³ − 1
  ([retrieval](architecture/retrieval.md#token-budgets), d7, d9), and in
  [mcp-tools.json](api/mcp-tools.json) `oml_recall`'s `max_tokens` and `oml_my_work`'s `limit`
  declare the ring's maxima, 2⁵³ − 1 and 200. The served `oml_recall` entry now describes
  full-text search, and `_meta.pending_design` names the hybrid search mode and the read log that
  it does not serve yet (d12).
- **Still unbuilt**, each stated in the README or a code comment: the designed `held_fts` (own
  held recall reads through the author-predicated `recall_own_held` port method instead, d5); the
  read log (d9); held-item expiry (`held_expires_at` is null); workarea edges (`edges` is `[]`);
  grant add and revoke, and denial lift (d2); issuance idempotency; key rotation (`rotated_from`
  is null) and expiry enforcement (an expiry on issuance is refused with `request.validation`,
  d16); `api.cursor_ttl`, held as the code constant `CURSOR_TTL_SECS`, 3600 s, until settings
  are built; the operator tier; the `in_progress` idempotency claim
  ([http-api.md](api/http-api.md#idempotency)): the record is written in the commit's own
  transaction, so a concurrent identical request answers the first attempt's receipt as a replay,
  or, for a submit that read the `claim_key` after that attempt committed,
  `submission.claim_key_conflict`, and never `idempotency.in_progress` (`IdempotencyPut` in
  `crates/oml-app/src/ports.rs`); the chain export (`POST /v2/exports` with `kind: "chain"`,
  [http-api.md](api/http-api.md#evidence-export)), and with it live `oml audit verify-chain`,
  which refuses a live run by name since d15 made the events tail a filtered feed, not a
  verification surface — the evidence export of M4 on the
  [critical path](product/critical-path.md) owns it; and set-wise hydration of served rows: each
  served row's readable `derives_from` ancestors, its `replaces` target and its workarea path are
  read one row at a time, against ADR-0024 d4, at a cost that is unmeasured (FR-7, FR-25).

### 2026-10-30 — `/readyz` names the build that answered (implementation)

A page edit; no decision changes. [Health](api/http-api.md#health) now declares a `build` object
on every `/readyz` response, ready or not: version, commit, commit time, build time, compiler,
target and profile, each `unknown` when nothing could supply it. The `Ready` schema in
[openapi.yaml](api/openapi.yaml) gains the `Build` schema. The image build has no git metadata,
so the release workflow passes the commit and its time as build arguments, and
`scripts/check_release.py` refuses a release workflow that does not. `SOURCE_DATE_EPOCH`, when
set, fixes the reported build time so a pinned build stays reproducible. The stated cost: an
unauthenticated probe now discloses the exact build.

### 2026-10-30 — The key tool: `oml keys issue-agent`, the default reader grant, the `oml-admin` plugin (ADR-0028)

[ADR-0028](adr/ADR-0028-agent-key-issuance.md), accepted by the maintainer on 2026-10-31 with
release v0.2.1, decides, and this change builds, the
key tool the maintainer chose to ship in v0.2.1, before names. Its three decisions were drafted as
d12–d14 of the proposed ADR-0026 (names are keys), which with the proposed ADR-0027 sits on its
own branch; this record takes the next number after both. It amends ADR-0015 (d1), whose Status line
carries the back-pointer.

- **Decided and built:** `oml keys issue-agent --principal <principal> --workarea <workarea-id>`,
  one `POST /v2/keys` for a writer key with writer and reader grants on the workarea and that
  workarea as its default, printing exactly the `OML_URL` and `OML_API_KEY` export lines; with
  `--out <path>`, the value written once to a new 0600 file created before anything is sent, an
  existing path, a path with a quote or a newline, and `--out` with `--json` refused; an issuance
  that gets no usable answer once it may have reached the server, which the CLI reports as a key
  that may have been issued, never as a refusal, and without echoing the body; the reader
  grant `oml keys issue` and `issue-agent` add beside every writer grant unless `--write-only`,
  carrying the writer grant's deny masks (neither verb takes a mask yet, so none is sent),
  while HTTP issuance still takes exactly the grants it is sent (ADR-0025 d2); the `oml-admin`
  Claude Code plugin, one skill that runs `issue-agent` only with `--out` and never with `--json`,
  and no MCP server.
- **Checks:** `crates/bins/oml-cli/tests/cli_e2e.rs` drives the verbs against the real lead behind
  a request recorder, and against stubs that cut the `201` short or close the connection
  unanswered; `scripts/check_plugin.py` now also fails if the marketplace stops listing
  `oml-admin` with a manifest, if `oml-admin` gains an `.mcp.json`, or if its skill runs
  `issue-agent` without `--out` or with `--json`.
- **Not built:** the positional key name (ADR-0026 d1), key revocation, which a write failure
  after issuance names as the remedy, and `oml context show` and `oml keys show`, which the skill
  would otherwise check with.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`, and no package version moves until the release.
- **Pages touched:** [ADR-0028](adr/ADR-0028-agent-key-issuance.md), the
  [decision register](adr/index.md) (row and the D14 cell), `mkdocs.yml`, [llms.txt](llms.txt),
  [CLI reference](cli/reference.md), [roles and keys](architecture/roles-and-keys.md) (the key
  format rule names the `--out` file) and this page; outside `docs/`,
  `.claude-plugin/marketplace.json`, `plugins/oml-admin/`, `plugins/oml/README.md` and
  `scripts/check_plugin.py`.

### 2026-10-30 — The release image for linux/amd64 and linux/arm64 (implementation)

An implementation milestone; no decision changes. `.github/workflows/release.yml` now builds the
image as [CI and toolchain](engineering/ci-and-toolchain.md) decides: one leg per platform on a
native runner (`ubuntu-24.04`, `ubuntu-24.04-arm`), each pushed by digest with its own SBOM and
provenance, then a merge job that assembles one index with `docker buildx imagetools create`,
asserts both platforms are in it, and signs the index digest with `cosign sign --recursive`. The
binary tarballs stay `x86_64-linux`, and the release notes say so. `ci.yml` gains an `image` job
that builds the image for both platforms on every pull request and runs its `oml --version` on
each, and `scripts/check_release.py` fails the pull request if `release.yml` stops building both
platforms, assembling the index or signing its digest. No tag has run the new pipeline yet; the
`v0.1.0` image is `linux/amd64` alone.

### 2026-10-30 — the agent-only memory surface (ADR-0025)

[ADR-0025](adr/ADR-0025-the-agent-only-memory-surface.md) decides the surface FR-10 and FR-11
need and the access model it rests on. It amends ADR-0001 (d5), ADR-0008 (d4, d6), ADR-0010
(d1, d3), ADR-0012 (d7), ADR-0013 (d1, d2, d5), ADR-0014 (d2), ADR-0018 (d4) and ADR-0022
(d4, d6), and each of those records' Status lines carries the back-pointer. Proposed on
2026-10-30; accepted by the maintainer on 2026-10-31, with the build that implements it, and
tagged `spec-v0.2`.

- **Decided:** grant admissibility per key role and one four-step refusal order, so one
  writer-role key with writer and reader grants remembers and recalls; issuance that carries
  grants and denies and discloses the value only after the last grant commits; "own" as the
  authoring key; `include_own_held` on recall, with held text matched only through an
  author-fenced index; the budget token (counter `oml-tc/1`) and the `max_tokens` cut with a
  `budget` object; applicable steering and a total hit order; a recall logs the ids served and a
  digest of the whole request; supersession moves the tip only at ratification, by a
  compare-and-set bound to the successor's workarea and `claim_key`, as one `lifecycle.supersede`,
  with the authors of rival held successors kept off each other's review; the author's
  retire-proposal as a supersession carrying `retirement: true`, open to the same keys as any
  supersession; distinct version ordinals, so competing held successors coexist and the tip
  compare-and-set decides between them; own read on the target version; a self-signed version
  never replacing a reviewed tip (d17); the `claim_key` release after a never-promoted rejection
  and the refusal of a reopen that would collide (d18); `simulation.write` journalling
  `body_bytes`, `replaces` and `retirement` like every submission event; own simulation rows read
  only inside the simulation's audience; `oml_get` and `oml_supersede` and one claim-item shape;
  the own-work listing across every workarea the key wrote to; the workarea read with the caller's
  children, effective verbs and permission epoch; per-family visibility on the events tail with a
  bounded scan and `next_after_seq`, where a `lifecycle.*` event reaches readers only while the
  version's latest move between held and promoted is a promotion, reaches its author in any case,
  and names no reviewer, admin or claim workarea the key holds nothing on; a request digest over
  method, route, path parameters and body, with supersede's idempotency record scoped to the key
  alone, looked up before the target's readability so a committed supersession always replays, and
  claimed only once the target is known readable, every target-dependent refusal ordered after
  that; a `held_by: service` key refused every review-class verb; `derives_from` and `replaces`
  served only as far as the caller can read; a successor's kind changing only within its class,
  and nothing but its kind inherited from the target; a row-count check on every `held_fts`
  removal; effective access computed on every request with no cache, so a revocation applies to
  the key's next request served by the lead, and on follower-served reads after replication lag
  (proposing to amend ADR-0008 d4 and d6); request bodies that refuse unknown fields.
- **`info.version` 0.1.0-design → 0.2.0-design**, because `/v1` request and response shapes
  change. No operation, error code, event type or claim state is added: the taxonomy and the
  operation set are unchanged. The MCP catalog goes from ten tools to twelve.
- **Roadmap:** FR-30 (the budget-token count against model tokenizers, unmeasured), FR-31 (the
  `claim_key` collision, the version ordinal, the tenant-wide journal positions and the lineage
  read's redacted ids as residual existence and count signals), FR-32 (a service key's grant
  confers every verb of its role, not only the one act its job needs) and RS-17 (revising one's
  own claim before its review) are opened. FR-10's exit criterion is reworded to the
  retire-proposal ADR-0025 d11 decides, a supersession of the claim rather than a verb of its
  own. FR-10 and FR-11 stay open until the agent-only-memory scenario passes in CI.
- **The release image** is decided as one index for `linux/amd64` and `linux/arm64`, signed on the
  index digest; a page edit, no decision change.
- **Pages touched:** the [decision register](adr/index.md) (register row; D4, D6, D7, D9, D11,
  D12, D13 and D14 cells), `mkdocs.yml`, [llms.txt](llms.txt), [openapi.yaml](api/openapi.yaml),
  [HTTP API](api/http-api.md), [errors](api/errors.md), [the agent plugin](api/mcp-plugin.md),
  [mcp-tools.json](api/mcp-tools.json) (the two new tools, `oml_my_work`, `oml_workarea_info` and
  `_meta`, with `_meta.pending_design` naming what the served `oml_recall` and `oml_events_tail`
  entries still lack; those entries gain only what the build already serves — the `k` maximum,
  the `after_seq` default, and no `scope` on the tail),
  [retrieval](architecture/retrieval.md), [roles and keys](architecture/roles-and-keys.md),
  [workareas](architecture/workareas.md), [admission](architecture/admission.md),
  [audit and lineage](architecture/audit-and-lineage.md),
  [event catalog](data/event-catalog.md), [Postgres schema](data/postgres-schema.md),
  [roadmap](product/roadmap.md), [CLI reference](cli/reference.md),
  [CI and toolchain](engineering/ci-and-toolchain.md), [upgrades](operations/upgrades.md),
  [security](architecture/security.md), [configuration](operations/configuration.md)
  (`auth.epoch_cache_bound` removed with the effective-access cache it bounded, which ADR-0025
  rejects),
  [commitments](product/commitments.md), [compliance](product/compliance.md),
  [concepts](concepts.md), [the overview](index.md) and this page; outside `docs/`, `CONTRIBUTING.md`, `README.md` and
  `plugins/oml/README.md` (version and tool counts).
- **Not built.** Nothing above is implemented by this entry; the PoC's divergences are named in
  ADR-0025's Context, and the code lands in later changes, each with its own entry. A
  `spec-v0.2` tag follows the maintainer's acceptance.

## 0.1.0-design

Reconstructed from the repository history; all dates 2026-10-29. The set was written and
revised in one sustained effort, so the entries below group commits by what they decided
rather than pretending a longer calendar.

### 2026-10-30 — The agent plugin's first tools, CI and the release pipeline (implementation)

An implementation milestone; no decision changes. The [agent plugin](api/mcp-plugin.md) is served
at `/mcp` for four of its ten tools — `oml_remember`, `oml_learn`, `oml_recall`,
`oml_events_tail` — as the stateless facade [ADR-0014](adr/ADR-0014-http-api-and-mcp-facade.md)
decides, each tool running its ring endpoint's own handler. The pinned MCP revision is
**2025-06-18**, checked against modelcontextprotocol.io (basic/transports, server/tools) on
2026-10-30. The remaining six tools answer as not built. A PoC key carries one role, so the Claude
Code plugin under `plugins/oml` connects twice, with a writer key and a reader key; the
single-key shape waits on grants of several roles per key. The merge gates and release artifacts
of [CI and toolchain](engineering/ci-and-toolchain.md) now run, except the MSRV build, the sqlx
offline check, the container tests against MinIO, the scheduled `cargo audit` and the bench job.
The docker profile's environment names were corrected to the ones
[configuration](operations/configuration.md) declares.

### 2026-10-30 — Scan-veto journalling decided; a transition precondition added

Two design points the Live Atlas review rounds forced into the open, now decided:

- **A scan veto journals nothing.** `submission.scan_veto` is a refusal like any other refusal:
  deterministic, reproducible from the journalled rule-set pin, and never a write into the
  signed chain (hostile input must not grow it). The wording is now uniform across
  [admission](architecture/admission.md), the
  [event catalog](data/event-catalog.md#what-is-deliberately-not-an-event),
  [errors](api/errors.md), [concepts](concepts.md), the [HTTP API](api/http-api.md),
  ADR-0012 (amended) and ADR-0020; whether refused submissions deserve a durable trace outside
  the chain is RS-16 on the [roadmap](product/roadmap.md).
- **`committed` → `validated` is refused while a simulation is fenced on the subtree** — the
  simulation must be ended (journalled) first, since taint can never enter a `validated` area
  ([workareas](architecture/workareas.md), transition matrix).

### 2026-10-30 — Live Atlas visualization

- The published site gains the [Live Atlas](atlas/index.html): an interactive 3D visualization
  of the crew-harness story — one crew per workarea, recall pulses pulled from production
  areas, remember pulses into the crew's own area, supersession with pulled evidence, ladder
  transitions, and the integration lattice between production areas — with drill-in modes for
  the admission reactor and the chain ledger. All data is mocked client-side in the shapes of
  the [event catalog](data/event-catalog.md); three.js is vendored (MIT), nothing loads from a
  CDN at runtime. Replaying a real PoC `/v2/events` tail is a planned data-source swap.

### 2026-10-30 — The PoC lands (M1 slice)

- The critical path's M1 slice is implemented in this repository (commit `54dadfd`): the Cargo
  workspace of [workspace-plan](engineering/workspace-plan.md), the signed chain reproducing
  [chain-format](data/chain-format.md)'s test vectors byte-for-byte, the one-transaction
  commit point, the standalone-profile server with startup fork check, and an independent CLI
  chain verifier — demonstrated end to end by `scripts/poc-demo.sh`. The spec remains the
  authority; where the PoC diverges it says so in code comments and the README.

### 2026-10-29 — Documentation finalization

A coordinated finalization pass over the whole set, encoding decisions taken for
implementation readiness:

- The [decision register](adr/index.md) gains the D1–D17 ownership table (which page declares
  which decision handle), the ADR status lifecycle, and an [ADR template](adr/TEMPLATE.md);
  this changelog and its going-forward rules are introduced.
- The closed error taxonomy grows 45 → 46 with `request.unsupported_media` (HTTP 415, family
  `request`, not retryable), carried on all three synced surfaces
  ([errors](api/errors.md), [openapi.yaml](api/openapi.yaml),
  [HTTP API](api/http-api.md)).
- The chain signing format is pinned: `signed_bytes` is RFC 8785 (JCS) canonical JSON of the
  enumerated envelope fields, `hash` is SHA-256 over them, `sig` is pure Ed25519 (RFC 8032)
  over `signed_bytes`; genesis is `prev_hash` = 32 zero bytes with `tenant.genesis` at seq 1
  ([chain format](data/chain-format.md)).
- The API key wire format is fixed (`oml_` prefix + 43 base62 characters, 256-bit random,
  stored as SHA-256 of the full key), and key-lifecycle journal events for signing-key and
  KEK rotation are added to the [event catalog](data/event-catalog.md).
- Operational commitments are pinned: forward-only `sqlx` migrations run by the bootstrap
  workload, a 24-hour committed floor on the idempotency replay window, positional pagination
  cursors with a named TTL setting, `simple` as the v1 FTS configuration with NFC
  normalization at admission, and one [runbooks page](operations/runbooks.md) with one
  anchored section per condition.
- Glossary entries for steering, the overlay tip and the fold join
  [Concepts](concepts.md).
- FR-26 (retention + legal hold), FR-27 (tenant offboarding ceremony verbs), FR-28 (operator
  verbs: dead-letter re-enqueue, erase replay, standalone migrate), FR-29 (attachment/bundle
  Range support) and RS-15 (per-tenant FTS configuration scope) added to the
  [roadmap](product/roadmap.md).

### 2026-10-29 — Install profiles, cluster mode, performance foundation

[ADR-0023](adr/ADR-0023-install-profiles-and-cluster-mode.md) (three install profiles, one
conformance bar, lead/follower cluster mode) and
[ADR-0024](adr/ADR-0024-performance-foundation.md) (hot-path discipline, the DEK serving
cache as a copy class, batch submit, latency targets as FR-25 rows).

### 2026-10-29 — The two-ring API

[ADR-0022](adr/ADR-0022-two-ring-api.md): one keyed listener, the /v1 provider ring under a
compatibility covenant and the /v2 lab ring, the token/header/body attribute-placement rule;
the [HTTP API](api/http-api.md) restructured around the rings.

### 2026-10-29 — AI+Human ontologies

[ADR-0021](adr/ADR-0021-ai-human-ontologies.md): vocabularies as governed claims — terms with
permanent CURIEs, releases as journalled pins, SSSOM mappings, the vocab scanner
([ontologies](architecture/ontologies.md)).

### 2026-10-29 — Critical review and dataset onboarding

The commitments register ([commitments](product/commitments.md)), the provider floor, dataset
onboarding ([ADR-0020](adr/ADR-0020-dataset-onboarding.md)), and the critical path
([critical path](product/critical-path.md)). Separately, the FR-9 market-claim
re-verification was refreshed with a 2026-10-29 fetch date.

### 2026-10-29 — Review-blocker resolution

All review blockers from the first read-through resolved; the [Concepts](concepts.md) page
and [ADR-0019](adr/ADR-0019-hexagonal-architecture-and-plugins.md) (hexagonal architecture
and plugins) added, the hexagonal-architecture presentation corrected, Mermaid sequence
diagrams fixed (semicolons escaped as `#59;`), and accidentally committed session telemetry
removed from the working tree and ignored (commit 19b4ee6); it remains in the repository's
earliest history pending a maintainer decision on a rewrite.

### 2026-10-29 — Initial design set

The founding commit: ADR-0001 through ADR-0018, the architecture, data, API, CLI and
operations pages, the [OpenAPI document](api/openapi.yaml), the [roadmap](product/roadmap.md)
as honesty ledger, and the GitHub Pages site built with `mkdocs --strict`.
