<!--
id: adr/ADR-0091-workareas-become-workspaces-and-the-dreamer-role-becomes-reviewer-on-every-surface-in-one-cut-and-the-journals-former-names-are-read-through-one-map
tier: engineering
status: accepted design · 2026-11-14
answers: what the statuses committed and validated and the key field held_by are called from v0.8.0, and whether human becomes person; which lifecycle display names change copy only; what the role that proposes and reviews is called from v0.8.0 and why not curator; what the governed node of a tenant's tree is called from v0.8.0 and how the docs define the word against its meaning elsewhere; which names change on the HTTP API, MCP, the CLI, the environment, the plugin settings, error codes, metrics, the request log, spans, the console and the lake; what a v0.7 client sees; how signed journal events that carry the former names are read by replay, the lake twin's fold_of, chain verification and oml events; why a rebuild still equals the database it was drained from; how the database is migrated without editing an applied migration; how a design record's file name may change and how links to the former name resolve; what never changes; how production is upgraded and what clients are told
-->

# ADR-0091 — Workareas become workspaces and the dreamer role becomes reviewer on every surface in one cut, and the journal's former names are read through one map

**Status:** accepted design · 2026-11-14, by the operator, as a pure cut: retired names are unknown
input, never refused by name (d2); the lake columns renamed by Delta column mapping after a spike,
the physical name kept behind a mapped one if it fails (d7); no redirects for renamed pages or
records, only the register's list of former file names (d10) · proposed the same day, requested by the operator on 2026-11-14 ("lets do renaming of
workareas to workspaces across whole project, sourcecode, documents, web, landing, everything"),
with four decisions the operator took the same day: a hard cut with no aliases (d2), former event
names read as the new ones (d4), every file renamed, design records included (d10), and the
release, v0.8.0 (d12). The operator added the role rename the same day ("Use 'reviewer'
instead"; asked whether it joins this record and release: "Yes, same ADR and v0.8.0"), under the
same rules (d13). The operator then folded three more stored names into the same cut ("Fold
into v0.8.0", so that clients take one breaking upgrade): the statuses `committed` and
`validated` become `protected` and `frozen` (d14), and the key field `held_by` becomes `used_by`
(d15); and accepted decision 3 of the lifecycle review, its display names (d16).

**Release:** planned for v0.8.0; not built

**Twin parts** (ADR-0090 d18): this record adds no state. It changes the names the twin's parts
carry from the release on and adds the former-name map (d4) to `fold_of`; d5 shows the twin's
equality survives the cut.

**Amends:**
- ADR-0018 (d4): the vocabulary row's word becomes `workspace`; `workarea` moves to the "never"
  column, outside the former names d4 lists. The roles row reads `reader`, `writer`, `reviewer`,
  `admin`, `operator`, and `dreamer` joins the never column (d13).
- ADR-0010 (d1, d5): the role `dreamer` is `reviewer`, with the same two capability classes (d13).
- ADR-0009 (d1, d4, d5): the statuses are `development`, `protected` and `frozen` (d14).
- ADR-0025 (d2) and ADR-0077 (d2): the key's and the approver's `held_by` is `used_by`, valued
  `agent`, `person` or `service` (d15).
- ADR-0026 (d1, d5): the kinds are workspaces, crews, keys and tenants; `name.taken`'s
  `details.kind` spells `workspace`. Names stay immutable keys: no instance is renamed (d11).
- ADR-0037 (d4): the key's guardrail scope value `workarea` becomes `workspace`.
- ADR-0090 (d12, d14): the fold law reads former names through d4's map, and the equality law
  gains the case of a database migrated across the cut (d5).

## Context

**The word.** Since [ADR-0008](ADR-0008-workarea-algebra.md) the governed node of a tenant's tree
is a *workarea*: grants bind to its node id, it moves up the status ladder, and memory is written
into it ([workareas](../architecture/workareas.md)). ADR-0018 d4 fixed the word and listed
"workspace" among the words never used for it. The operator reversed that on 2026-11-14 and asked
for the rename on every surface: source, documents, the console, the landing site and operations.

**The size.** On `main` at `bf2ec9b3`, `git grep -i workarea` matches 17,649 times in 706 files:
the database schema of 46 applied migrations, every crate, the HTTP API and its OpenAPI file, the
three MCP tools, the CLI's noun and flags, two environment variables, error codes, a metric, the
request log, the console's routes, three generated SDKs, the plugins, 72 design records (12 of
them in their file names) and two further repositories, the landing site and the operations
scripts. The inventory and the build order are a plan beside this record, not part of it.

**What cannot be renamed.** The journal is signed. Each row's `signed_bytes` holds its
`event_type` and its payload, and the signature and hash chain cover those bytes exactly
([chain format](../data/chain-format.md)): six event types spell `workarea.` today
([event catalog](../data/event-catalog.md)), and most payloads carry `workarea_id`. Rewriting a
signed row breaks verification, and R4 keeps the journal append-only
([ADR-0001](ADR-0001-the-constitution.md)). The lake twin holds the same rows and must rebuild a
database equal to the one it was drained from ([ADR-0090](ADR-0090-the-lake-is-a-replayable-twin-of-postgresql-that-holds-every-signed-event-with-its-sealed-fold-inputs-and-rebuilds-it-into-an-empty-database.md)
d12, d14). sqlx records a checksum of each applied migration, so an applied migration is never
edited. Instance names are keys and never change ([ADR-0026](ADR-0026-names-are-keys.md) d1, d10).

**The role.** [ADR-0010](ADR-0010-roles-and-keys.md) d1 names five roles; the one that
proposes edits and connections and reviews other identities' writes is `dreamer`, and d5 gives it
two capability classes, propose and review. The pages already call a key with that role "a
reviewer key", and a person's session key for it is `<user>-review`, so the role's name is the one
word on the surface that matches neither. On `main` at `3c21dc9f` `git grep -i dreamer` matches
2,204 times in 194 files: the role enum and its string, the role check constraints of
migrations 0001, 0004 and 0028, grants such as `dev:dreamer`, the CLI's `--role` and `--grant`,
error details, the OpenAPI descriptions, the SDKs and the docs. Signed `key.issued` and
`grant.issued` payloads carry `role: "dreamer"`, and a status transition's `policy_snapshot`
carries `required_roles`. "Curator" already has three meanings, none of them a role: the
in-progress curator key profile ([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)),
the ontology crew's curator grant profile, and a universe curator with its `<user>-curate` key
([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)).

**The word elsewhere.** In data platforms a workspace is usually the outer boundary, not a node
of a tree. Databricks: "The Databricks workspace is your central hub for accessing all Databricks
objects and features" ([Databricks documentation](https://docs.databricks.com/aws/en/workspace/),
fetched 2026-11-14). Microsoft Fabric: "Workspaces are places to collaborate with colleagues to
create collections of items such as lakehouses, warehouses, and reports", and a workspace holds
folders, not other workspaces ([Microsoft Learn](https://learn.microsoft.com/en-us/fabric/fundamentals/workspaces),
fetched 2026-11-14). An OML workspace nests, and the boundary is the tenant. The operator chose the
word knowing this; d1 states the cost and how the docs carry it.

Decided, not built.

## Decision

- **d1 — The word is `workspace`, on every surface, from v0.8.0.** A workspace is a governed node
  of a tenant's tree: workspaces nest under `universe`, grants bind to a workspace's node id and
  reach its subtree, and the tenant, not the workspace, is the isolation boundary. Every page,
  identifier, file name and message that says `workarea` says `workspace`, plural `workspaces`,
  type names `Workspace…`, constants `WORKSPACE…`. This amends ADR-0018 d4's row: the word is
  `workspace`; "namespace" and "folder" stay in the never column, and `workarea` joins them,
  except where d4 and d11 keep it. The substitution reaches the text of accepted records too: it
  changes what a record calls the thing and nothing it decides, which the register states once
  (d10) instead of a back-pointer on every record. The cost is a word that a reader from
  Databricks or Fabric reads as an account or environment boundary. The docs carry it in three
  places: the glossary row and the first paragraph of [concepts](../concepts.md) say "a workspace
  is a node in a tree, not an account or environment; the tenant is the boundary", and the
  workspaces page opens with the same sentence and the tree.

- **d2 — The cut is pure: no alias on any surface, and a retired name is unknown input
  (decided by the operator, 2026-11-14).** Every name below changes in v0.8.0 and the former one
  stops working in the same release. A retired name answers exactly as any unknown input answers
  today, with no rule of its own: a retired path is 404; a retired JSON field or enum value is
  `request.validation` as an unknown field or value, with no `details.renamed_to` entry
  ([errors](../api/errors.md)); a retired header (`X-OML-Workarea`), query parameter or
  environment variable (`OML_CHAT_WORKAREA`, `OML_WORKAREA_NAME`) is ignored, as an unknown one
  is today. The cost is stated openly: a v0.7 client that names its target only by the retired
  header or query parameter is not refused, so its write can land silently in its key's default
  workspace instead of the one it meant, held there under review like any write. The guard is
  the order of the upgrade: the v0.8.0 upgrade note (d12) tells every client to upgrade its CLI,
  plugins and SDKs before the server is upgraded, and a v0.8.0 client already speaks only the new
  names. The renames:

    | Surface | Former | From v0.8.0 |
    |---|---|---|
    | HTTP paths | `/v2/workareas`, `/v2/workareas/{workarea}` and its `tree`, `offers`, `crew`, `mode`, `federation`, `transitions` and `transitions/{transition_id}/ratify` | `/v2/workspaces`, `/v2/workspaces/{workspace}/…` |
    | HTTP header | `X-OML-Workarea` | `X-OML-Workspace` |
    | Query and JSON fields | every field whose name holds the token: `workarea`, `workarea_id`, `default_workarea_id`, `workarea_path`, `root_workarea_id`, `to_workarea_id`, `scope_workarea_id`, `workarea_ids`, … | the same name with `workspace` |
    | Error codes | the family `workarea` (`workarea.frozen`, `workarea.transition_refused`, `workarea.in_progress_refused`, `workarea.held_remaining`, `workarea.admin_key_under_root`, and the designed ones of ADR-0080 to ADR-0089); `request.workarea_unresolved` | the family `workspace`; `request.workspace_unresolved` |
    | Error details | `details.kind: "workarea"` of `name.taken`; `details.workarea_id` | `"workspace"`; `details.workspace_id` |
    | MCP | the tool `oml_workarea`, its argument `workarea`; the argument `workarea` of `oml_write` and `oml_read` | `oml_workspace`, `workspace` |
    | CLI | the noun `oml workareas` and its verbs; `--workarea`, `--workareas`, `--default-workarea`; the lake query `top-workareas` | `oml workspaces`; `--workspace`, `--workspaces`, `--default-workspace`; `top-workspaces` |
    | Environment | `OML_CHAT_WORKAREA`; `OML_WORKAREA_NAME` (the quickstart) | `OML_CHAT_WORKSPACE`; `OML_WORKSPACE_NAME` |
    | Plugin settings | `chat_workarea` in the `oml` plugin's `userConfig` | `chat_workspace` |
    | Key setting | guardrail scope `workarea` (ADR-0037 d4) | `workspace` |
    | Console | `/web/workareas`, `/web/admin/workareas`, `/web/workarea/new`, `/web/workarea/edit`, `/web/workarea/retire`, `/web/admin/workarea`, the query `?workarea=`, form fields and actions such as `workarea_transition` | `/web/workspaces`, `/web/admin/workspaces`, `/web/workspace/…`, `?workspace=`, `workspace_transition` |
    | Metrics | `oml_workarea_mode_changes_total{mode}` | `oml_workspace_mode_changes_total{mode}` |
    | Request log | the field `workarea_id` | `workspace_id` |
    | Spans (OTLP) | the store-port span names and `oml.store.method` values `workarea`, `workarea_ancestry`, `workarea_subtree`, `workarea_children`, … | `workspace`, `workspace_ancestry`, … |
    | Database | tables, columns, constraints, indexes, policies, functions and comments (d6) | the same with `workspace` |
    | Lake | the columns `workarea_id` of `bronze.events` and `silver.claims` (d7) | `workspace_id` |

    The cost is carried openly: every installed v0.7 CLI, plugin, SDK and console bookmark stops
    working against a v0.8.0 server until it is upgraded (d12), and every saved permission rule
    naming `mcp__plugin_oml_oml__oml_workarea` stops matching.

- **d3 — From the release on, the server writes only the new names.** Events are `workspace.*`
  (`workspace.create`, `workspace.mode_set`, `workspace.federation_set`,
  `workspace.status_proposed`, `workspace.status_transition`, `workspace.crew_updated`, and the
  designed ones), payload keys spell `workspace`, and payload values that named the kind spell
  `workspace`. No signed row is rewritten, re-signed or re-hashed: R4 and the chain hold as they
  are, and an auditor verifies a former row exactly as before.

- **d4 — Every reader reads a former name through one map, after verification.** One total, pure
  function in `oml-domain` maps an event as stored to the event as read:
  1. an `event_type` in the closed list of former names, the six the catalog lists today and
     every `workarea.*` type any released binary wrote, becomes the same suffix under
     `workspace.`;
  2. every object key at any depth of the payload whose name holds the token `workarea` or
     `workareas` has the token replaced by `workspace` or `workspaces`;
  3. a closed list of (event type, field) values that named the kind becomes `workspace`: the
     `kind` of `system.names_assigned` and the `guardrail_scope` of `key.issued` and of any event
     that records it;
  4. a closed list of (event type, field) values that hold a role has the value `dreamer`
     replaced by `reviewer`: `role` of `key.issued` and `grant.issued`, `required_roles` inside
     a `policy_snapshot`, and every other field the catalog types as a role (d13);
  5. a status value `committed` or `validated`, in `status` of `workarea.create` and `from` and
     `to` of `workarea.status_proposed` and `workarea.status_transition`, becomes `protected` or
     `frozen` (d14);
  6. the payload key `held_by` becomes `used_by`, and its value `human` becomes `person`, in
     `key.issued` and every event that records a key's holder (d15).

  The map is the identity on every event the new server writes, so applying it twice changes
  nothing and the readers need no cut-over date. Replay, the lake twin's `fold_of` (ADR-0090 d12),
  `oml verify` and `oml events`, the events tail and its visibility rules, and the lake writer's
  projection of `workspace_id` into `bronze.events` read through it and through nothing else.
  Verification checks the stored `signed_bytes` first, byte for byte as the chain format
  requires, and maps only for what it reports; it never re-serializes a mapped event. The event
  catalog lists each former name beside its successor in a table of its own, the only place the
  catalog spells `workarea`. A reader that meets a `workarea`-spelled name outside the map's
  lists fails closed naming the event and the name, and `oml doctor` gains the line
  `journal_unmapped_former_name`.

- **d5 — The twin's equality holds across the cut.** ADR-0090 d12's law is that each use case's
  fold equals `fold_of` of its event and its staged parts; d14 proves it as an equality of two
  databases. Write `fold₇` for the v0.7 fold, `M` for d6's migration and `fold₈` for the v0.8.0
  fold. A row written before the cut is `M(fold₇(e))` in the live database and
  `fold₈(map(e))` in the rebuilt one. `M` renames relations, columns and the closed list of
  stored values one to one, and `fold₈` is `fold₇` with the same names renamed, so
  `M ∘ fold₇ = fold₈ ∘ map` for every former event, and both sides hold equal rows. The journal
  table compares equal because both sides hold the stored rows unchanged: the rebuild copies
  `signed_bytes`, `event_type` and the payload as stored and maps only to fold. The unsigned
  copies of former names that d6 rewrites in the live database, such as an idempotency row's
  operation name, are produced by `fold_of` in the new spelling, so they compare equal too. The
  proof is a law in CI, `a_database_migrated_across_the_rename_equals_its_rebuild`: a throwaway
  cluster at migration 0046 receives journal rows signed with the former names by a test signer
  and the state their folds made, migrates to the cut, drains, rebuilds into a second cluster and
  compares under ADR-0090 d14. This amends ADR-0090 d12 (`fold_of` reads through d4's map) and d14
  (the law above joins the comparison laws).

- **d6 — One new migration renames the schema; no applied migration is edited.** Its number is
  reserved in `docs/NUMBERS.md` when the build starts. In one transaction it:
  - renames the tables `workareas`, `workarea_transitions` and `search_workarea_rows`, every
    column whose name holds the token (`workarea_id`, `default_workarea_id`,
    `workarea_node_id`, `source_workarea_id`, `scope_workarea_id`, `reference_workarea_ids`,
    `federated_workarea_ids`, …), every constraint, index, sequence and policy whose name holds
    it, and every comment that says it, enumerated from `pg_catalog` on a migrated throwaway
    cluster, never by reading old migrations alone;
  - replaces with `CREATE OR REPLACE` every function whose body names a renamed relation or
    column: PL/pgSQL bodies are stored as text and do not follow a rename, while policy
    expressions, views, indexes and constraints are stored parsed and do;
  - rewrites the unsigned values that spell the kind (`api_keys.guardrail_scope`, with its check
    constraint, and the operation names of live `idempotency` rows) and nothing signed or sealed;
  - rewrites the role value `dreamer` to `reviewer` wherever a column holds a role
    (`api_keys.role`, `grants.role`, and stored policy and transition snapshots), and replaces
    the role check constraints of migrations 0001 and 0004 and the curator-profile check of
    migration 0028 (`profile = 'curator' AND role = 'reviewer'`) (d13);
  - rewrites the statuses `committed` and `validated` to `protected` and `frozen` in the
    workspaces and their transitions, and replaces the status check constraints of migrations
    0001 and 0017 (d14);
  - renames the columns `held_by` and `approver_held_by` to `used_by` and `approver_used_by`,
    rewrites the value `human` to `person` in them, and replaces the check of migration 0001
    (d15).

  The law `the_schema_names_no_workarea_after_the_rename_migration` reads `pg_class`,
  `pg_attribute`, `pg_constraint`, `pg_policy`, `pg_proc.prosrc`, `pg_trigger` and
  `pg_description` after migrating and finds no `workarea`, and finds no `dreamer` in a role
  column or check constraint. The applied migrations 0001 to 0046
  keep their text and their sqlx checksums, and stay exempt from the final vocabulary check (d11).
  The renames take an `ACCESS EXCLUSIVE` lock for their transaction, which the upgrade's stopped
  server makes free (d12).

- **d7 — The lake renames its two columns by Delta column mapping after a spike, or keeps the
  physical name behind a mapped one (decided by the operator, 2026-11-14).** The
  lake is append-only with no retention cut (ADR-0090 d16), so its data files are never
  rewritten. `bronze.events.workarea_id` and `silver.claims.workarea_id` become `workspace_id` by
  Delta's column mapping in `name` mode, a metadata-only rename, if a spike shows the pinned
  `deltalake` (1.1.2) writes and reads such a table and `oml sync`'s Delta reader, which already
  accepts the `columnMapping` feature, reads it. If the spike fails, the physical name stays
  `workarea_id`, the lake's schema page names the column `workspace_id` with that physical name,
  and the readers map it. The spike runs first in the surfaces lane, and its result names which
  of the two the lane builds; both are decided here, so neither needs a new decision.

- **d8 — Operations read the new names; dashboards read both for one log window.** The metric,
  the request-log field and the span values of d2 change in the release. Prometheus series of the
  former metric end at the upgrade and the new ones start at zero. Dashboards and alerts in the
  operations repository move to the new names in the same release; queries over the request log
  read `workspace_id` or `workarea_id` until the log's retention window has passed since the
  upgrade, then the new name alone. That dual read is in the operator's own dashboards, not in
  the product.

- **d9 — The SDKs are regenerated, never edited.** The Python, TypeScript and Rust SDKs are
  regenerated from the renamed `openapi.yaml` by `scripts/generate_sdks.py`; module paths, type
  names and method names follow (`oml_sdk.generated.api.workspaces`, `Workspace`,
  `create_workspace`). The SDK packages take the version of the release, and their changelogs say
  that v0.7 code naming a workarea type does not compile or import against them.

- **d10 — A design record's number is its key; its slug may change, and the register lists the
  former file name; nothing redirects (decided by the operator, 2026-11-14).** A record is cited by
  `ADR-nnnn`, and its file name is `ADR-nnnn-<slug>.md`. A slug changes only by a record that
  decides it, and the register gains a list, "Renamed records", of former file name beside current
  file name, one row per rename, never pruned. It is a list for readers, not a redirect.
  The twelve records whose file names say `workarea` (ADR-0008, 0030, 0057, 0067, 0072, 0073,
  0082, 0084, 0085, 0086, 0087 and 0089) are renamed by this record, and so are the pages `architecture/workareas.md`
  and `use/workareas.md`, the design note `design/workarea-lifecycle.md` and every other tracked
  file whose path holds the word. How each link resolves:
  - Relative links inside the docs are rewritten in the same change. `hooks/adr_links.py` turns a
    relative link to a record into its GitHub blob URL; it gains a check that the target file
    exists, so a link to a former name fails `mkdocs build --strict` and names the listed row.
  - GitHub does not redirect a blob URL whose file was renamed; a link to a former name on
    `main` answers 404, and a permalink pinned to a commit keeps working. No stub file is left
    at a former name: `scripts/gen_design_records.py`, `check_numbers` and `oml sync` all take
    every `ADR-*.md` as a record. The register's list is where a reader holding an old link finds
    the new one, and `gen_design_records.py` prints it on the design records page.
  - The site gains no redirect plugin (`requirements.txt` pins only `mkdocs-material`, and stays
    so): a bookmarked URL of a renamed page, such as `use/workareas/`, answers 404 from the
    release. The landing site, which links `use/workareas/`, moves to the new paths in the same
    release window.
  - `oml sync` anchors a record's claims to its path and keeps each claim's key once written
    (ADR-0057 d18): a span whose text is unchanged while its file moved is `moved` and needs no
    review, while a span moved and edited in one commit reads `removed` and `new`. So the twelve
    files are renamed in a commit of their own, with their text unchanged, and synced into the
    design knowledge workspace before the text substitution lands; the substitution then
    supersedes each changed span under its kept key, one review per span.

  The register's lifecycle section gains one sentence: a vocabulary substitution that a record
  decides is applied to accepted records' text without changing what they decide, and quoted
  text keeps its words (d11).

- **d11 — What keeps the former word.** The final check of the build is
  `git grep -i workarea` with every remaining hit in an allow-list file the vocabulary lint reads,
  each line naming its reason. The reasons are closed:
  - signed bytes: the chain vectors in `docs/data/chain-format.md`, `scripts/check_chain_vectors.py`
    and `crates/bins/oml-cli/tests/vectors.rs`, which stay byte for byte, and a new vector in the
    new spelling beside them;
  - the former-name map of d4, its tests, and the catalog's former-name table;
  - the applied migrations 0001 to 0046, and d6's migration, which names what it renames;
    for the role, migrations 0001, 0002, 0004 and 0028 keep `dreamer`;
  - released history: the sections of `docs/changelog.md` for released versions, and upgrade
    notes for versions before v0.8.0;
  - measured results under `bench/*/results/` and `eval/*/results/`, and pages generated from
    them, which record what a run printed on its day;
  - the `oml sync` test fixtures that snapshot a repository at a named commit;
  - quoted text: an operator's or source's words keep their spelling inside quotation marks;
  - instance names and stored content: no workspace, key, crew or tenant name is renamed
    (ADR-0026 d1, d10), the prefix `wa` of the names ADR-0026 d11 derived stays, and no memory's
    text or attachment is touched;
  - this record and the register's list of renamed records, which name the former files;
  - for the statuses and the key field, the map's rules 5 and 6 and their tests, and the senses
    d14 and d15 leave alone (`committed_at`, a committed transaction, `commit`, `commit_first`,
    `held_by_key`, `held_since`, `human_required`, "AI+Human" in ADR-0021's title);
  - for the role, the map's rule 4 and its tests, and ADR-0010's rejected alternative, whose
    sentence about splitting the role into proposer and reviewer keeps its sense by naming the
    role `dreamer` as it was when the alternative was weighed.

  The allow-list and the vocabulary lint cover `workarea` and `dreamer` as words, and
  `committed`, `validated`, `held_by` and `human` as literals and identifiers, since the first
  two stay ordinary English.

- **d12 — v0.8.0 ships the cut, all nodes at once, and clients are told before.** The upgrade is
  not rolling: a v0.7 binary cannot read the renamed schema, and a v0.8.0 binary cannot serve a
  v0.7 client. The runbook, in the operations repository, is:
  1. announce the date to every key holder with the client notice below, which is also the
     upgrade note in `docs/operations/upgrades.md`, and ask them to upgrade their clients before
     the server;
  2. take a backup and verify it restores (the backup runbook);
  3. stop `oml-web` and the lead; the deployer installs v0.8.0 of both from one tag, its Caddy
     route read from `deploy/caddy/Caddyfile` at that tag, which names `/web/workspaces`;
  4. start the lead, which applies d6's migration; check `oml doctor` (no
     `journal_unmapped_former_name`), `oml verify` over every tenant, and the schema law of d6;
  5. drain the lake and run ADR-0090's compare when the twin is built;
  6. upgrade the operator's own CLI and plugins, set `chat_workspace` where `chat_workarea` was
     set, and supersede every reviewed steering, guardrail or skill whose text names
     `oml_workarea` or `X-OML-Workarea`, each a held version reviewed as usual.

  The client notice: "OML v0.8.0 renames workareas to workspaces. A v0.7 CLI, plugin, SDK or
  console bookmark stops working against a v0.8.0 server: paths, fields, the MCP tool
  `oml_workarea` (now `oml_workspace`), the CLI noun, `OML_CHAT_WORKAREA` and the plugin setting
  `chat_workarea` all change. Upgrade the CLI, plugins and SDKs before the server is upgraded:
  v0.8.0 ignores an old client's workarea header or query parameter, so a write it sends can
  land in the key's default workspace. Set `chat_workspace`, and update saved
  permission rules that name `oml_workarea`. Memories, keys, grants and workspace names are
  unchanged. The role `dreamer` is now `reviewer`: `--role reviewer` and grants such as
  `dev:reviewer`. The statuses `committed` and `validated` are now `protected` and `frozen`, and
  a key's `held_by` is now `used_by`, with `human` now `person`. Existing keys, grants and
  workspaces are converted by the upgrade."

- **d13 — The role `dreamer` becomes `reviewer`, under d2 to d12's rules.** The role keeps both
  capability classes of ADR-0010 d5, propose and review, its ceiling and its refusals; only its
  name changes, so a "reviewer key" on every page is a key whose role is `reviewer`, and the
  session key `<user>-review` already matches it. It is a pure cut with no alias (d2): `dreamer`
  is an unknown role value, so a request naming it is `request.validation` as any unknown enum
  value is, and the CLI rejects `--role dreamer` or a `:dreamer` grant as it rejects any unknown
  role; it is never read as another role. The
  server writes `reviewer` from the release on (d3); signed payloads keep `dreamer` and every
  reader maps it through d4 rule 4; the twin's equality holds by d5's argument with the value
  rewrite of d6 in `M`; d6's migration converts every stored role. The renames:

    | Surface | Former | From v0.8.0 |
    |---|---|---|
    | Role value (API, events, DB, SDK enums) | `dreamer` | `reviewer` |
    | Rust and SDK identifiers | `Role::Dreamer`, `NotDreamer`, `dreamer_grants`, … | `Role::Reviewer`, `NotReviewer`, `reviewer_grants`, … |
    | CLI | `--role dreamer`; `--grant <workspace>:dreamer` | `--role reviewer`; `--grant <workspace>:reviewer` |
    | Error details | `details.role`, `details.needed_role` and `required_roles` values `dreamer` | `reviewer` |
    | Policy objects | `required_roles: ["dreamer"]` | `required_roles: ["reviewer"]` |
    | OpenAPI, the console and the docs | "dreamer", "a dreamer key" | "reviewer", "a reviewer key" |

  The three curator meanings stay exactly as they are: the in-progress curator key profile
  (whose check now reads `role = 'reviewer'`), the ontology crew's curator grant profile, and a
  universe curator with its `<user>-curate` key. No metric or label names the role today, so none
  changes. The cost: "reviewer" names class (b) only, while the role also proposes (class (a));
  ADR-0010's text and the roles page say "a reviewer key proposes edits and connections and
  reviews other identities' writes, never on a lineage it proposed in", and the constitution's
  rule that the reviewer is never the author now reads as a sentence about a role as well as an
  act. Instance names that hold the word, such as a key named `the-dreamer`, are names and keep
  their spelling (ADR-0026 d10).

- **d14 — The statuses `committed` and `validated` become `protected` and `frozen`, under d2 to
  d12's rules (decided by the operator, 2026-11-14).** The ladder of ADR-0009 keeps its three
  rungs, its forward-only transitions, its two identities per step and its child constraint; only
  two names change. `protected`: two persons reach it, it is never undone, and it refuses shared
  drafts, the in-progress mode of ADR-0030, as `committed` does today. `frozen`: what
  `validated` means today, matching the error code that already refuses a write there, which
  becomes `workspace.frozen` (d2). The universe root is permanently `protected`. "Validated" no
  longer collides with ADR-0086's tests and gates, the lifecycle review's finding. The words keep
  their other senses: an idempotency row or a transaction is still committed (`committed_at`),
  and the in-progress API's `commit` field and `commit_first` reason are not this rename and stay.
  The renames reach the status enum and its strings, the check constraints and the constraint
  name `workareas_universe_committed`, the API and OpenAPI enums, the CLI's `status propose
  --to`, the console's badges, the SDKs and the docs; events follow d4 rule 5.

- **d15 — The key field `held_by` becomes `used_by`, valued `agent`, `person` or `service`
  (decided by the operator, 2026-11-14; `person` decided here).** The attestation of who holds a
  key keeps its meaning and its rules (ADR-0025 d2, ADR-0037 d5, ADR-0077 d2); the field and one
  value change. `human` becomes `person`, because the pages already say "a person's key",
  "a person's verdict" and "a different person" for this attestation, and a field spelling a
  different word from every sentence about it is the drift ADR-0018 d4 forbids. The rename reaches
  `held_by` on key issuance, whoami, key listings and served guardrail verdicts, `approver.held_by`
  (now `approver.used_by`), the reason `held_by_service` of `oauth.connect_refused` (now
  `used_by_service`), the `HeldBy` types (now `UsedBy`), the CLI's `--held-by` and its output,
  the SDKs and the docs; events follow d4 rule 6. It does not reach the review lease: a review
  item is still held by one key at a time, `review_queue.held_by_key` and `held_since` keep their
  names, because there "held" is the state a write is in and not who holds a key. It does not
  reach `human_required` in a review policy, which this record leaves as it is.

- **d16 — The lifecycle review's display names change copy only (decision 3 of the lifecycle
  review, 2026-11-14, accepted by the operator the same day).** These change console copy, CLI
  help and output wording and the docs, and no stored value, field, event or error code:
  - **approve, approved** for a non-author verdict that makes a version readable, with "1 of 2
    approvals" where a policy needs more than one, and "approved by a person" on instruction
    surfaces. The stored state `promoted` and the verdict values are unchanged.
  - **take** for the review lease: a reviewer takes one held item. The API's verb and fields for
    the lease are unchanged.
  - **the drafts-visibility toggle** in place of "put in progress" and "commit": "drafts visible
    to: the writing key · everyone working here". The API's `mode` values, its `commit` field and
    the reason `commit_first` are unchanged.
  - **development rules / production rules** for the two rule classes, in place of "scope" for
    them.

  The lifecycle review's decisions 1, 2, 4, 5 and 6 are not decided here. Its decision 3 also
  named reviewer, protected, frozen and used by as display names; this record makes those four
  stored renames instead (d13, d14, d15).

## Consequences

- The plan beside this record lists every hit by group, the lanes, the check that closes each
  and the open pull requests that conflict; it is the build's checklist, and this record is
  the decision the build cites.
- Pages that own the mechanics change in the build, not here: the event catalog (former-name
  table), the chain format (a new vector), errors, the HTTP API, the MCP catalog, the CLI
  reference, the settings reference, metrics, the lake tables page, the glossary, concepts and
  the workspaces pages.
- The v0.8.0 upgrade note tells clients to upgrade before the server, since d2 refuses nothing by
  name.
- The landing site (`e8kor/oml-web`) and the operations scripts (`e8kor/oml-devops`) change in
  the same release window; the landing site's lane also takes the legal pages' wording.
- The role rename (d13) changes the roles page, ADR-0010's text, the console's role labels and
  every `--role` and `--grant` example; no operations script names the role today.
- The connector directory submission names the tool `Workarea`; the directory listing is updated
  when v0.8.0 is live.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Rename only what a person reads (display-only)** | The console, docs and landing site would say "workspace" while the API, CLI, MCP tool, errors and schema say `workarea`, so every reader crosses from one word to the other at the first command or error. It costs less now, and it keeps two words for one thing on the engineering surfaces, which ADR-0018 d4 exists to prevent. The operator chose the full rename. |
| **Aliases until 1.0: accept both names, serve the new one** | No v0.7 client breaks, at the price of two names on every route, field, tool, flag and error until 1.0, each a branch to test, and responses that can never say which name a client should use. Pre-release, the house rule is to rewrite an obsolete contract rather than shim it. The operator chose a pure cut, with no transition aid on the server; the upgrade order of d12 is the aid. |
| **Name the role `curator`** | "Curator" already names three things that are not a role: the in-progress curator key profile, the ontology crew's curator grant profile, and a universe curator with its `<user>-curate` key. A role called curator would make every one of those sentences ambiguous. The operator chose `reviewer` ("Use 'reviewer' instead"), which matches "reviewer key" and `<user>-review` already on the pages. |
| **A separate record and a later release for the role** | Clients would break twice, once for each rename, and the former-name map and migration would be built twice. The operator put both in this record and v0.8.0 ("Yes, same ADR and v0.8.0"). |
| **Refuse retired names by name for one release** | A retired header, query parameter or environment variable would answer `request.validation` naming its successor, so no v0.7 write lands in a default workspace. It adds a rule per retired name to build, test and later remove. The operator chose the pure cut and the upgrade order instead (2026-11-14). |
| **Redirect renamed site pages with `mkdocs-redirects`** | Bookmarks of the two renamed pages would keep landing, at the price of a new pinned dependency for two pages. The operator chose no redirects (2026-11-14); old page URLs answer 404 and the landing site moves to the new paths. |
| **Rewrite the journal into the new names** | Every rewritten row would need a new signature and hash, breaking the chain an auditor verified and R4's append-only rule. The map of d4 costs one function and a closed list. |
| **Keep the former names in the journal forever, new events included** | Readers would need no map, but every new event would carry the retired word, and the catalog would document two vocabularies for one release onward. |
| **Edit the applied migrations to the new names** | sqlx refuses a migration whose checksum changed, and a database migrated with the old text would differ from one migrated with the new. |
| **Stub files at the former design-record names** | Keeps old GitHub links working, but every tool that reads `ADR-*.md` would take each stub as a record. The register's list and commit permalinks serve the same reader without that. |
