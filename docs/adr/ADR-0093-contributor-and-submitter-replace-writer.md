<!--
id: adr/ADR-0093-contributor-and-submitter-replace-writer
tier: engineering
status: proposed
answers: what the key that recalls and writes and the key that writes without recall are called from v0.8.4; whether the grant role writer is renamed and to what; which grants each key role admits and what issuance refuses; what an existing writer key becomes and what a key whose grants do not fit its role is called; how the database is migrated and how signed events that say writer are read; whether a retired writer is refused by name; which doctor line, upgrade note and client notice the cut carries; which release ships it
-->

# ADR-0093 — Contributor and submitter replace writer

**Status:** proposed · 2026-11-19, requested by the operator on 2026-11-19 ("Let's improve
naming to represent writer role correctly"), who chose a full rename over console labels: a
**contributor** recalls and writes, a **submitter** writes without recall, a **reader** is
unchanged; and decided the release, v0.8.4 with #498 (d8).

**Amends:**
- ADR-0010 (d1): the roles are `reader`, `contributor`, `submitter`, `reviewer`, `admin` and
  `operator`.
- ADR-0018 (d4): the roles row reads the same; `writer` as a role joins the never column (d7).
- ADR-0025 (d1, d14): the admissibility table of d2 below; the grant role that confers
  `remember`, `learn`, `attach` and `supersede` is `submitter`.
- ADR-0028 (d2): the CLI adds a reader grant beside each submitter grant of a contributor key;
  `--write-only` is retired in favour of `--role submitter` (d6).
- ADR-0030 (d8): a curator profile's grant roles are `reader`, `submitter` and `reviewer`.
- ADR-0044 (d6, d7): a user key that creates workspaces is a contributor key; a curator is a
  contributor key with a submitter node grant on `universe`.
- ADR-0084 (d11): the importer is a person's contributor key.
- ADR-0091 (d4, d11): the former-name map gains rule 7 (d4), and the vocabulary lint covers
  `writer` as a role (d7).

## Context

The console's Keys page offers `writer` or `reader`, never both, and the operator asked whether a
writer key also reads. The page's writer key does: `issue_own_key` sends a writer and a reader
grant on the person's root (`crates/oml-app/src/app/pages.rs:528`), as does the `<user>-web`
session key (`auth.rs:373`) and the CLI unless `--write-only` is given
(`with_reader_beside_writers`, [ADR-0028](ADR-0028-agent-key-issuance.md) d2). Nothing in the
word says so. Roles do not imply one another
([roles-and-keys](../architecture/roles-and-keys.md)): the writer key role admits reader and
writer grants (`admissible_grant_roles`, `crates/oml-domain/src/access.rs:108`), the writer grant
confers the four write verbs and no recall (`VERBS_CONFERRED`, `access.rs:274`), and recall comes
from a reader grant alone. So one word covers two different keys, one that recalls and writes and
one that writes and reads only its own rows (ADR-0025 d3).

`writer` is stored in `api_keys.role` and `grants.role` (checks of migration 0047), parsed in
`role_from_str` (`crates/adapters/oml-pg/src/lib.rs:2602`) and the HTTP layer, signed into
`key.issued` and `grant.issued`, and checked by key role in uploads (`require_role`), the
user-root workspace create (`app.rs:3864`), imports (`imports.rs:174`, `key.role !=
Role::Writer`) and the curator's ceiling (`ceiling_role`, `app.rs:712`). It is spelled in the
OpenAPI, the MCP tool texts, the SDKs, the CLI's `--role` and `<workspace>:writer` grants, the
console form, error details (`needed_role`), the docs and the plugins' key skills.
[ADR-0091](ADR-0091-workareas-become-workspaces-and-the-dreamer-role-becomes-reviewer-on-every-surface-in-one-cut-and-the-journals-former-names-are-read-through-one-map.md)
made the same kind of cut for `dreamer` → `reviewer`; this record follows its rules and states
only where a split word differs from a renamed one.

Decided, not built.

## Decision

- **d1 — The grant role `writer` becomes `submitter`; the key role `writer` splits into
  `contributor` and `submitter`.** Grant roles are `reader`, `submitter` and `reviewer`; the
  submitter grant confers exactly what the writer grant confers today. Key roles are `reader`,
  `contributor`, `submitter`, `reviewer`, `admin` and `operator`. `Role` stays one enum, as it
  already holds the key-only `admin` and `operator`; `contributor` is a key role only, and the
  grants check refuses it. A contributor's recall stays an explicit reader grant in the journal,
  never implied by a role, which keeps ADR-0025's rejected "writer implying read" rejected.

- **d2 — A contributor key carries at least one reader and one submitter grant; a submitter key
  carries submitter grants only.** The admissible grant roles are:

    | Key role | Grant roles |
    |---|---|
    | `reader` | `reader` |
    | `contributor` | `reader`, `submitter` (at least one of each) |
    | `submitter` | `submitter` |
    | `reviewer` | `reader`, `reviewer`; a curator profile also `submitter` (ADR-0030 d8) |

  A contributor issued without a reader or without a submitter grant is `request.validation`,
  `details.field: /grants`, `details.reason: contributor_grants`, nothing journalled; a reader
  grant on a submitter key is `access.issuer_ceiling` at step 3, as any inadmissible grant is.
  Grants are only ever added (`add_grants`), and step 3 applies there too, so a key keeps the
  shape it was issued with. The server refuses and never adds: it takes exactly the grants it is
  sent (ADR-0025 d2). A reader grant on the same workspace as each submitter grant stays the
  default of the CLI and the console (ADR-0028 d2), not a server rule, because `<user>-curate`
  writes on the `universe` node and reads elsewhere (ADR-0044 d7). A submitter still reads its
  own rows in every state (ADR-0025 d3); "no recall" means no recall of anything else.

- **d3 — An existing writer key becomes a contributor key; one without a reader grant is a
  "contributor without recall", named by the doctor and replaced by its holder.** Before the
  cut, a writer key could be issued with no reader grant (`--write-only`). d4's map is a pure
  function of one event, and `key.issued` carries no grants, so the map cannot tell the two kinds
  of writer key apart. The migration (d5) does the same as the map, `writer` → `contributor` for
  every key, so the twin's equality holds. Such a key works as before. `oml doctor` reports
  `key_contributor_without_recall` (warn) with each key's name, and the console marks its row
  "no recall". The remedy is a new submitter key and a cut-off of the old one. No act changes a
  key's role, and the upgrade writes no journal event.

- **d4 — Signed events keep `writer` and are read through ADR-0091 d4's map, rule 7.** The map
  gains rule 7. The `role` of `key.issued` that holds `writer` becomes `contributor`. The `role`
  of `grant.issued` and each `required_roles` value inside a `policy_snapshot`, and inside the
  `policy` of `lifecycle.promote` and `lifecycle.supersede`, that holds `writer` becomes
  `submitter`. The build lists every other catalog field typed as a role before the map is
  written. A `writer` met outside the list fails closed under the existing doctor line
  `journal_unmapped_former_name`. The map is the identity on every event the new server writes.
  No signed row is rewritten. ADR-0091 d5's argument carries over: the migration renames
  `api_keys.role` to `contributor` and `grants.role` to `submitter`, exactly as rule 7 does by
  event type, so `M ∘ fold = fold ∘ map`. The law
  `a_database_migrated_across_the_role_rename_equals_its_rebuild` joins ADR-0091 d5's, under
  the same wait for ADR-0090's compare. The lake has no role column; bronze holds events as
  signed.

- **d5 — One new migration rewrites the stored roles; no applied migration is edited.** Its
  number is reserved in `docs/NUMBERS.md` when the build starts. In one transaction it does the
  following:
  1. It drops `api_keys_role_check` and `grants_role_check`.
  2. It runs one `DO` block. That block sets `oml.startup_check` to `on` **before** the tenant
     loop, as migration 0047 does. Then, for each tenant, under its `oml.tenant_id` and the
     tables' forced row security, it rewrites:
     - `api_keys.role` from `writer` to `contributor`;
     - `grants.role` from `writer` to `submitter`;
     - any `writer` inside stored `required_roles`, to `submitter`. The build enumerates these
       columns from `pg_catalog` on a migrated throwaway cluster.

     It clears both settings after the loop.
  3. It adds the checks again: `api_keys.role IN ('reader', 'contributor', 'submitter',
     'reviewer', 'admin', 'operator')` and `grants.role IN ('reader', 'submitter', 'reviewer')`.
  4. It replaces, with `CREATE OR REPLACE`, every function whose body compares a role to
     `'writer'`. These are found by `pg_proc.prosrc`. The lake writer's own names are not a role
     and are left alone.

  The law `the_schema_holds_no_writer_role_after_the_role_migration` finds no `writer` in a role
  column or role check. Migrations 0001 to the one before keep their text and checksums. The
  locks are free because the server is stopped during the upgrade (d8).

- **d6 — The cut is pure on every surface, as ADR-0091 d2: `writer` is unknown input, not
  refused by name.** As a role value on HTTP, MCP, the SDKs or the CLI, `writer` is
  `request.validation` as any unknown enum value is, with no `details.renamed_to`. It has two
  successors, and naming one would mislead. The cut reaches the following surfaces:
  - **API and SDKs.** The OpenAPI enums and texts change, and error details read
    `needed_role: submitter` and `role: contributor`. The SDKs are regenerated (ADR-0091 d9).
  - **Rust.** `Role::Writer` becomes `Role::Contributor` and `Role::Submitter`, with
    identifiers such as `writer_reader_grants` following.
  - **CLI.** It takes `--role contributor` (the default of `oml keys issue-agent`) or
    `--role submitter`, and grants are written `<workspace>:submitter`. A contributor key gains
    a reader grant beside each submitter grant. `--write-only` is retired; clap refuses it as an
    unknown flag.
  - **Console.** The issue form offers `contributor` (the default) and `reader`. The listings
    print the role word, with d3's mark.
  - **MCP and docs.** The MCP tool texts, the docs, and the plugins' `issue-agent-key` skill and
    README change with the rest.

  Key-role checks follow the meaning of each site:
  - uploads admit contributor and submitter;
  - the user-root workspace create (ADR-0044 d6) and imports (ADR-0084 d11) admit contributor
    alone, because both act on a tree the key must read;
  - the curator's ceiling reads `submitter`.

  A failed request fails closed. A v0.8.3 CLI that sends `writer` is refused, and one that sent
  `--write-only` sends `writer` and is refused too. Neither issues a key.

- **d7 — The doctor, the vocabulary and the notice.** `oml doctor` gains
  `key_contributor_without_recall` (d3), and `journal_unmapped_former_name` covers `writer`
  (d4). The vocabulary lint and its allow-list (ADR-0091 d11) cover `writer` as a role:
  `Role::Writer`, a `"writer"` role literal, `:writer`, `--role writer` and the roles tables.
  The plain word keeps these senses:
  - the lake writer, its crate, its database role and Delta writers;
  - "the writing key" for an author;
  - applied migrations, released changelog sections and signed vectors;
  - rule 7 and its tests;
  - quoted text and earlier records' rejected alternatives.

  The upgrade note in `docs/operations/upgrades.md` and the client notice are one text:
  "OML v0.8.4 renames the writer role. A key that recalls and writes is a **contributor**. A key
  that writes without recall is a **submitter**. The grant that confers writing is `submitter`.
  Upgrade the CLI, plugins and SDKs before the server is upgraded. A v0.8.3 client that names
  `writer` is refused and issues nothing. Existing writer keys become contributor keys. One
  issued with `--write-only` keeps working, and `oml doctor` names it a contributor without
  recall. Issue a submitter key with `--role submitter` and cut the old one off. Memories, key
  values, grants and names are otherwise unchanged."

- **d8 — v0.8.4 ships the cut, with #498 (decided by the operator, 2026-11-19).** The operator
  wants the roles right in the release that closes this phase. The upgrade is not rolling and
  follows ADR-0091 d12's runbook. First, announce with the notice of d7. Then back up and verify
  the restore. Stop `oml-web`, the lead and the lake writer, and start v0.8.4 from one tag.
  Check `oml doctor`, `oml verify` and d5's law. Last, upgrade the operator's own clients and
  reissue any contributor without recall.

## Consequences

- The build reaches `oml-domain` (the enum, the admissibility table, the verbs table, the map's
  rule 7), `oml-app` (issuance and the `add_grants` check, the key-role sites of d6, the session
  and curate keys), `oml-pg` (role parsing, the migration), the HTTP layer, the CLI, `oml-web`,
  the OpenAPI, the generated SDKs and the references, `roles-and-keys.md`, the glossary, the
  event catalog's former-name table, the plugins' skills, and the doctor.
- The console-label lane (branch `web/key-role-labels`, uncommitted) is superseded: its question,
  whether a writer key reads, is answered by the word itself.
- Every installed v0.8.3 client must be upgraded before the server, as in ADR-0091.
- A contributor without recall keeps its name until its holder replaces it; the doctor line
  stays until none is left.
- The build is tracked by an issue in the v0.8.4 milestone, opened with the build.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Console labels only ("read and write", "read only", "write only")** | The first proposal. The operator chose a full rename: the API, the CLI and the docs would still say `writer` for two different keys. |
| **Keep the grant role `writer` under the new key roles** | A contributor key would list "writer on X", and the question the operator asked would come back on every listing. |
| **`contributor` as a grant role conferring recall and write** | It brings back "writer implying read", which ADR-0025 rejected. The read authority would not appear as its own grant in the journal, and a deny could not mask the two classes apart. |
| **Only `contributor`; a write-only key is a "write-only contributor"** | The operator named `submitter` as a role. A role word that must be qualified to be true is the problem this record removes. |
| **Separate `KeyRole` and `GrantRole` types** | The enum already holds key-only roles (`admin`, `operator`), and the check constraints and admissibility table already separate them. A type split would double this cut's diff without changing what is stored. |
| **The server requires a reader grant on the same workspace as each submitter grant** | `<user>-curate` writes on the `universe` node and reads elsewhere (ADR-0044 d7), so it would be refused. |
| **The server adds the missing reader grant** | It amends ADR-0025 d2, under which the server takes exactly the grants it is sent, and repeats ADR-0028's rejected "the HTTP issuance adds the reader grant". |
| **The migration makes a writer key without a reader grant a submitter** | `key.issued` carries no grants, so d4's map, a function of one event, would read the key as a contributor. The rebuilt database would then differ from the migrated one (ADR-0090 d12, ADR-0091 d5). |
| **An act that changes an existing key's role** | It would be a new verb, with its own ceiling and review, to serve a few keys that a new key and a cut-off already replace. |
| **Refuse `writer` by name with `details.renamed_to`** | `writer` has two successors, and naming either misleads. ADR-0091 d2 already chose a pure cut, where a retired name is unknown input. |
| **Accept `writer` as an alias of `contributor` for one release** | ADR-0091 d2 rejected aliases. A client that meant write-only would get a key that recalls. |
| **Edit the applied role checks of migrations 0001, 0004 and 0047** | It changes their sqlx checksums, and an applied migration is never edited. |
| **v0.9.0** | The operator wants the roles right in v0.8.4, the release that closes this phase, rather than in a later minor release after work parks. |
