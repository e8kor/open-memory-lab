<!--
id: adr/ADR-0026-names-are-keys
tier: engineering
status: proposed
answers: which records carry a unique name, what a name may spell, how a name is accepted wherever an id is and resolved on the server without leaking existence, what the universe root is called, how paths relate to names, what a crew is, how a key's name differs from its principal and label, whether a name is ever released, how rows that predate names are named, how the CLI's agent-key verb takes a key's name, and what workarea structure an admin key reads
-->

# ADR-0026 — Workareas, crews, keys and tenants are named by unique, immutable keys that every surface accepts in place of their ids

**Status:** proposed · 2026-10-30

**Amends:** ADR-0008 (d7), ADR-0010 (d6, d7), ADR-0012 (d4), ADR-0018 (d4), ADR-0022 (d4), ADR-0025 (d1, d2, d14)

## Context

**Ids are the only reference.** Every surface addresses a workarea, a key or a tenant by its id. A
workarea's name exists only as a path segment, unique nowhere: two workareas anywhere in a tenant
may share one, so the derived path, not the name, is what tells them apart
([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)). A key has a
principal and a label and nothing that addresses it but its id
([roles and keys](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)). An
operator issuing a plugin key has to copy a UUID out of one command's output into the next, and
an agent configured with `X-OML-Workarea` has to carry one.

**The convenience the pages already promise is undecided.** The [CLI](../cli/reference.md#conventions)
says "workareas may be named by path for convenience"; the `oml_remember` argument says "a path
given here is resolved to its node first" ([mcp-tools.json](../api/mcp-tools.json)). Neither says
what happens when two paths collide, when a path goes stale after a re-parent, or what a key is
told about a path it cannot read.

**A crew is a string copied into each workarea.** The crew record is metadata on a workarea
(ADR-0008 d7). The independence axis compares crews and external managers at promotion time
([ADR-0012](ADR-0012-admission-and-review.md) d4) through "journalled key→crew→manager metadata"
([ADR-0010](ADR-0010-roles-and-keys.md) d7), but no key carries a crew: `key.issued` has no such
field ([event catalog](../data/event-catalog.md)), and the issuance body has none
([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d2). Two workareas cannot say they belong
to one crew, so the axis has nothing to compare.

**The maintainer's decisions.** The maintainer fixed these, and this record designs around them and
does not reopen them: workareas, crews, keys and tenants each get a name that is a unique key like the
id, unique within the tenant in one flat namespace per kind, tenant names unique per deployment,
immutable once created; names are accepted everywhere an id is and resolved on the server, while
grants, denials and journal events keep binding to ids and creation events record the name; crews
become named records; the CLI gains `oml keys issue-agent`, with `--out` writing the key value to
a file so that an `oml-admin` skill can run it without the value entering a transcript; the CLI's
issuing verbs add a reader grant beside every writer grant unless `--write-only` is passed; and
admin keys read workarea structure in their tenant without grants, never content. The maintainer
later moved the key tool ahead of names: `issue-agent`, its `--out` key file, the reader grant
default and the `oml-admin` plugin are decided in ADR-0028, and d12–d14 below say what names add.

**What this record holds itself to.** Nothing it decides may leak existence: a name that does not
exist and a name outside the caller's access must answer exactly as an id does
([errors](../api/errors.md), `access.not_found`). It builds on the proposed ADR-0025 and reverses
none of its rules: the refusal order (d1), the issuance order (d2), own (d3) and the events-tail
visibility (d15) apply to a named request exactly as to the same request by id. It adds fields to
d2's issuance body and changes what d14's `crew` holds, which is why the Amends line names them. The existing PoC
data, and the live v0.1.0 deployment on oml.doublegate.io (one tenant named `oml`, a root workarea
the PoC created with the name `root` at crates/oml-app/src/app.rs:573-579, and keys with
principals), must survive with its journal untouched.

Line citations refer to the tree at commit `d1d44f4`, the base this record was written against.
External formats are cited from their sources, fetched 2026-10-30: the label grammar of
[RFC 1035](https://www.rfc-editor.org/rfc/rfc1035.html) §2.3.1, the UUID text form of
[RFC 9562](https://www.rfc-editor.org/rfc/rfc9562.html) §4, and the `ltree` label rule of the
[PostgreSQL 17 documentation](https://www.postgresql.org/docs/17/ltree.html). Everything this
record decides is design; nothing below is built.

## Decision

- **d1 — Workareas, crews, keys and tenants each carry a name: a unique, immutable key beside the
  id.** Every row of the four kinds has exactly one name, given at creation and never changed;
  no rename verb exists on any surface. The namespaces are flat, one per kind:

    | Kind | Unique within | Created by | The creation event records it in |
    |---|---|---|---|
    | workarea | its tenant, the whole tree | admin (`POST /v2/workareas`); the root by tenant creation (d6) | `workarea.create.name` |
    | crew | its tenant | admin (`POST /v2/crews`, d8) | `crew.created.name` |
    | key | its tenant; operator keys share the provider's namespace | admin, or operator for operator keys and a tenant's first admin key (`POST /v2/keys`); rotation (d9); the bootstrap claim | `key.issued.name`, `key.rotated.new_name` |
    | tenant | the deployment | operator (`POST /v2/tenants`) | `tenant.created.name`, `tenant.genesis.name` |

    The id stays the binding everywhere state is kept: grants, denials, every journal payload
    field that names a row, the idempotency record's scope, the read log, cursors and the lake.
    A name is how a caller addresses a row, never what the system stores in its place. The rules
    are declared once, in the [HTTP API's Names section](../api/http-api.md#names-and-ids) (D13),
    and every other page links there.

    This amends ADR-0018 d4 with one vocabulary row:

    | Term | Means | Never |
    |---|---|---|
    | name | the unique, immutable key of a workarea, crew, key or tenant, accepted wherever its id is | "slug", "handle", "alias"; "label" (a key's label is free text, d9) |

    One name per row, unique in its kind, lets a human or an agent say `metals-crew` where it said
    a UUID, and immutability is what makes that safe to cache: a name means the same row for as
    long as the deployment exists (d10). The cost is that a name chosen badly stays: a row that
    needs a different name is a new row (a new key, a new crew), never an edit.

- **d2 — A name is a lowercase RFC 1035 label that can never read as an id.** The grammar, in
  ABNF over ASCII:

    ```
    name   = letter [ *61( letter / digit / "-" ) ( letter / digit ) ]
    letter = %x61-7A        ; a-z only
    digit  = %x30-39
    ```

    That is RFC 1035's `<label>` (§2.3.1: a letter first, letters, digits and hyphens inside, a
    letter or digit last, "63 characters or less") restricted to lowercase. Three spellings the
    grammar admits are refused all the same, so that no name ever looks like an id to any tool:
    the RFC 9562 hex-and-dash form (d3); 32 hexadecimal digits, the form many UUID parsers also
    accept; and the derived form `tenant-`, `wa-`, `crew-` or `key-` followed by 32 hexadecimal
    digits, which d11 reserves for rows that predate names.

    There is no normalization and no case mapping. Every allowed character is ASCII, so Unicode
    NFC is the identity on a name, and names compare byte for byte. A value with an uppercase
    letter, a non-ASCII character, a `.`, a `/`, an `_` or a space is refused, never lowercased into a
    name it resembles ("refused, not coerced", [errors](../api/errors.md)).

    The grammar is the intersection of every place a name must travel unescaped: a URL path
    segment, an HTTP header value, a shell argument, a JSON string, a Kubernetes object name and a
    PostgreSQL `ltree` label, which PostgreSQL 17 allows to hold "alphanumeric characters,
    underscores, and hyphens" up to 1000 characters ([ltree](https://www.postgresql.org/docs/17/ltree.html)).
    Every existing example name fits it: `universe`, `procurement`, `metals-crew`, `q1-2027`,
    `research-crew`, `lit-review`, `run-grid`, `oml`. Principals keep their own free-text form
    (`crew-procure/agent-7`, d9). The cost: `Research Crew` and `2027-q1` are not names, and the
    caller spells them `research-crew` and `q1-2027`.

- **d3 — The ids of the four kinds are UUIDs in RFC 9562 hex-and-dash text, and a value in that
  form is always an id.** An id is 36 characters, 8-4-4-4-12 hexadecimal digits separated by
  hyphens, the form RFC 9562 §4 gives; either case is accepted on input, as §4 allows ("all
  uppercase, all lowercase, or mixed case"), and servers emit lowercase. Every value a request
  carries for one of the four kinds is classified at parse, with the rest of the request's
  syntax and before any check that consults state:

    1. in the id form → an id;
    2. otherwise, a name under d2 → a name;
    3. otherwise → `request.validation`, `details.field` naming the field.

    Because d2 refuses every name that is in the id form, the two classes never overlap, and a
    value's class never depends on what exists. The [HTTP conventions](../api/http-api.md#conventions)
    still tell clients never to parse an id; the form is fixed so the server can tell an id from a
    name. Ids of the other kinds — artifacts, serves, edges, grants, simulations — have no names
    and are untouched by this record.

- **d4 — Names are accepted everywhere an id is, and the server resolves a name by substituting
  its id before any check that consults state.** Every request field, path parameter, query
  parameter, header and MCP tool argument that names a workarea, crew, key or tenant accepts that
  row's id or its name: `X-OML-Workarea`, the `{workarea_id}`, `{key_id}` and `{crew_id}` path
  parameters, `workarea_id` filters, `parent_id`, `new_parent_id`, `to_workarea_id`, a grant's
  `workarea_node_id`, a deny's `subtree_node_id`, `default_workarea_id`, `crew`, `tenant_id`, a
  simulation audience's `key_id`, the `workarea`, `scope`, `from`, `to` and `workarea_id`
  arguments of [mcp-tools.json](../api/mcp-tools.json), and every CLI argument that names one of
  the four kinds. Field names do not change: a request field called `workarea_id` accepts the id
  or the name, and a response field called `workarea_id` always carries the id. The
  [Names section](../api/http-api.md#names-and-ids) lists every field.

    Resolution happens on the server, never in the MCP facade or the CLI, in the caller's
    namespace: the key's tenant, or the deployment for a tenant name, or the provider's namespace
    for an operator key's name. It runs after the verb ceiling, which needs no row — step 1 of the
    [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order), or
    the key-role check of an admin or operator verb — and before every check that reads one. A
    resolved name is replaced by its row's id, and an unknown name by an id that matches no row;
    the request then runs exactly as
    the same request by id runs, through the same refusal order, issuance order, supersede rule
    and events-tail visibility. Names therefore add no refusal path: an unknown name and a name
    outside the key's access both answer `access.not_found`, in the same step, with the same body,
    as the id would; and no refusal names the id a name resolved to unless it would name that id
    for a request that sent it.

    Responses carry ids in id fields and paths where they carry paths today, and each object of
    the four kinds — a workarea, a crew, a key, a tenant — gains a `name` field. Claim items,
    hits and events tail entries gain no name field: a claim's workarea is named by its id and its
    derived path, whose last segment is the name (d7).

    The idempotency request digest covers the request as sent
    ([HTTP API](../api/http-api.md#idempotency)), so a request naming a row and the same request
    by id are two requests: a retry resends the same bytes, and reusing one `Idempotency-Key` for
    the other spelling is `idempotency.digest_mismatch`, never a replay. On the provider ring the
    record's workarea component is still the resolved node id (ADR-0022 d4), so `X-OML-Workarea`
    by name and by id share one scope. This amends ADR-0022 d4: the override is "a node id or
    name", and `/v2` keeps explicit workarea references — ids or names — in paths and bodies.

    Substitution is the smallest rule that keeps names inside every guarantee the ids already
    have: there is one access computation (ADR-0008), and a name cannot reach a branch the id
    would not. Resolving after step 1 keeps a reader key's submit an `access.verb_ceiling`
    whether its header is an id or a name, as ADR-0025 d1 requires. Digesting the request as sent
    keeps the record lookup ahead of any state read, at the cost that a client which changes its
    spelling between retries is refused rather than replayed.

- **d5 — A malformed name is `request.validation`, an unknown one `access.not_found`, and a taken
  one `name.taken`, a new code only an admin or operator key can receive.** `name.taken` (409)
  is the one code this record adds, in a family of its own, `name` ([errors](../api/errors.md));
  `details` is `{kind, name}`. It answers a creation — workarea create, crew create, key
  issuance, key rotation, tenant create, the bootstrap claim — whose name its kind already holds
  in the namespace, a revoked or expired key's included (d10), and `universe` for any workarea
  but the root (d6). It is checked after every other check of the creating request, for key
  issuance after step 6 of the [issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order),
  and the unique index is its authority: a creation that loses the race to a concurrent one
  answers `name.taken` and journals nothing, which for a key means before `key.issued` commits.

    Only admin and operator keys hold a creating verb
    ([permission matrix](../architecture/roles-and-keys.md#permission-matrix)), and an admin sees
    every row of its tenant, an operator every tenant, so only a caller entitled to see the whole
    namespace ever learns that a name is taken. No agent key can create a named row, so no agent
    key can probe for names: every lookup it makes resolves through d4 to `access.not_found` or to
    a row it may read. A value that is neither an id nor a name is `request.validation` on every
    surface, at parse, before any check that consults state, so its answer reveals nothing about
    what exists.

    A taken name differs from a malformed one: it depends on state, and retrying it can never
    succeed, which is what a 409 says and what `vocab.prefix_conflict` and
    `submission.claim_key_conflict` already say for their permanent identifiers. It is not a
    `request.*` code, because that family holds what is wrong with a request whatever the state,
    and not a `workarea.*` code, because it also answers keys, crews and tenants.

- **d6 — Every tenant's root workarea is named `universe`.** Tenant creation creates the root with
  the name `universe` and journals it in the root's `workarea.create`; the root's name is the
  only workarea name fixed by the design. Since every tenant has its root from creation onwards,
  a workarea create naming `universe` answers `name.taken`, with no special case. Every derived
  path therefore starts with `universe`, as every path in the doc set already does, and
  `universe` stays the word ADR-0018 d4 gives the root. The PoC's root named `root` is named
  `universe` under d11.

- **d7 — Paths stay derived; they are displayed and looked up, never accepted as a reference.**
  A path is the names from `universe` to the node joined with `.` — the derivation ADR-0025 d14
  fixes — and with unique names the last segment alone identifies the node, which is why the
  workarea read's `path_leaf` is the child's name. Every segment is a name, so every path is an
  `ltree` value. Names never change and re-parenting moves no name, so a re-parent changes a
  node's path and nothing else; grants, which bind ids, are untouched, as before. A path is
  never accepted in a field that takes an id or a name: it contains `.`, so d3 refuses it with
  `request.validation`. `GET /v2/workareas?path=` stays the one lookup by path. The CLI
  convention that workareas "may be named by path" and the `oml_remember` sentence that "a path
  given here is resolved to its node first" go: the name says the same thing in one segment, and
  never goes stale.

- **d8 — Crews are named records of their own, created and updated by admins, referenced by
  workareas and by keys.** A crew record is `{crew_id, name, charter_uri, external_manager}`:
  recorded, not operated, as before ([workareas](../architecture/workareas.md#crews-recorded-not-operated)
  owns the record).

    - **Verbs.** `POST /v2/crews` creates one (journalled `crew.created`); `POST
      /v2/crews/{crew_id}/update` changes its charter URI or external manager, never its name
      (journalled `crew.updated`); `GET /v2/crews` and `GET /v2/crews/{crew_id}` read them. All
      four need the admin key role: a crew is tenant governance metadata, and no agent role
      reaches an admin verb (R1). No MCP tool creates, updates or lists crews, and the
      [agent-safe tool list](ADR-0014-http-api-and-mcp-facade.md) is unchanged. The CLI verbs are
      `oml crews create`, `update`, `list` and `show` ([CLI](../cli/reference.md#oml-crews-crew-records)).
    - **Workareas.** A workarea names at most one crew, the one whose memory space it is; a crew
      may serve many workareas. `crew` on workarea create is a crew's id or name, and `POST
      /v2/workareas/{workarea_id}/crew {crew}` points the workarea at another crew record. That
      act stays the journalled crew-metadata update `workarea.crew_updated`, now carrying
      `crew_id`, and stays admitted on `validated` workareas. A vocabulary workarea still requires
      a crew ([ADR-0021](ADR-0021-ai-human-ontologies.md)). The workarea read serves the crew
      record, or null, and is the only crew read an agent key has.
    - **Keys.** Key issuance takes an optional `crew`, a crew's id or name, journalled in
      `key.issued` as `crew_id`. A key's crew is fixed for its life, and rotation carries it to
      the successor; a key holder who changes crews holds a new key. This is the key→crew link
      ADR-0010 d7 names and no payload carried.
    - **The independence axis.** At promotion, a crew-inequality requirement compares the
      authoring key's crew with the reviewing key's crew, and a manager-inequality requirement
      compares those crews' `external_manager` values as the journal holds them at that
      promotion ([roles and keys](../architecture/roles-and-keys.md#review-independence-and-the-evidence-of-collusion)).
      A key with no crew satisfies neither requirement, and a crew with no external manager
      satisfies no manager-inequality requirement: such a verdict does not count toward the
      requirement it cannot establish, and the row waits for one that does — failing closed by
      holding, as D11 does everywhere.
    - **What the existing crew metadata becomes.** The `crew` object on a workarea, at
      `workarea.create` and `workarea.crew_updated`, is replaced by a `crew_id`. Existing values
      become crew records under d11, and nothing in the journal is rewritten.

    This amends ADR-0008 d7 (a workarea names a crew record instead of carrying one), ADR-0010 d7
    (a key's crew is a journalled reference set at issuance) and ADR-0012 d4 (an unestablished
    crew or manager satisfies no inequality requirement), and ADR-0025 d2 and d14 (issuance takes
    `crew`; `crew` at `workarea.create` is a reference, and the workarea read serves the record
    with its `crew_id`).

    The independence axis compares crews, and a string copied into each workarea's record can say
    neither that two workareas share a crew nor which crew a key belongs to. A record of its own
    gives both relations one row to point at, gives a renamed charter one place to change, and
    keeps crew administration where every other governance act lives: admin key role, journalled,
    off the agent surface.

- **d9 — A key's name addresses it; its principal and label keep their meanings.** The three are
  independent and none is derived from another on the server:

    - **name** — the key's unique, immutable key in its namespace (d1), required at issuance,
      by which admin verbs address it: show, revoke, rotate, grants, denials, effective access, a
      simulation audience;
    - **principal** — who acts: the attribution identity in every envelope, shared by design
      across the keys one identity holds, compared by R2 and matched by bans; free text, unchanged;
    - **label** — an optional free-text note, unique nowhere, unchanged.

    Rotation mints a new key, so the successor takes a new name: `POST /v2/keys/{key_id}/rotate`
    takes `{grace, name}`, and `key.rotated` journals `new_name`. The bootstrap claim names the
    root operator key: `POST /v2/bootstrap/claim` takes `{enrollment_token, name}`. A principal
    cannot be the name, because two keys of one principal — its admin key and its writer key, the
    two-key rule's pair — would collide, and a principal's `/` is not in the grammar. A label
    cannot be, because it is prose. A successor cannot inherit its predecessor's name, because both
    keys are live through the grace window. Declared on
    [roles and keys](../architecture/roles-and-keys.md#key-names-principals-and-labels).

- **d10 — A name is never released.** A revoked or expired key keeps its name, and the name still
  resolves for an admin, so `oml keys show` answers for it as history. Workareas, crews and
  tenants have no removal verb, so their names are never freed either, and erasure shreds claim
  content and touches no name ([audit and lineage](../architecture/audit-and-lineage.md)). A
  reissued agent key therefore takes a new name (`claude-laptop-2`). A name that once named one
  key and later another would make every journal reading, export and runbook that quotes the
  name ambiguous across time; ids never recur, and names, which stand in for them, must not
  either.

- **d11 — Rows that predate names are named once, by a deterministic rule the journal records;
  nothing is deleted or rewritten.** At its first start on the schema version that adds names,
  and before it serves a request, the lead names every unnamed row and journals the assignments:

    1. **The root.** Each tenant's root workarea is named `universe` (d6), whatever its
       `workarea.create` says.
    2. **Rows with a usable name.** For each kind and namespace, in journal order of creation,
       a row whose creation event carries a name that d2 admits, that is not `universe` for a
       non-root workarea, and that no earlier row of its kind holds, keeps it.
    3. **Every other row** — no name (every key before this record), a name d2 refuses, or a
       name an earlier row holds — is named `<prefix>-<32 lowercase hex digits of its id>`, the
       prefix `tenant`, `wa`, `crew` or `key`. Ids are unique, and d2 refuses the form to every
       creation, so a derived name collides with nothing.
    4. **Crew metadata.** For each workarea, in journal order, its current crew object (the
       latest of its `workarea.create` and `workarea.crew_updated`) is taken; each distinct
       `{name, charter_uri, external_manager}` becomes one crew record with a fresh id, named by
       rules 2 and 3 in order of first appearance, and every workarea with that object points at
       it. Keys that predate names have no crew.

    One `system.names_assigned` event per chain that has anything to assign — each tenant's chain,
    and the provider chain for tenant and operator-key names — lists every assignment as `{kind,
    id, name, prior_name}` and every crew record it creates
    ([event catalog](../data/event-catalog.md)); the folds read it like any event. Every earlier
    event stays as it was signed, its original name or crew object included, so the journal shows
    both what a row was called and what it is named now. The rule is declared with the names, in
    [the HTTP API](../api/http-api.md#rows-created-before-names).

    On the live deployment this names the tenant `oml` (it fits d2), names the root `universe`
    with `prior_name: root`, and names every key `key-<hex>`. An admin who wants a chosen name on
    a live key issues a new key under that name and revokes the old one; the derived name stays on
    the old key as history. Keys from before this record have no crew, so a crew-independence
    policy (d8) counts none of their verdicts.

    Journalling the assignment is what keeps it from being silent: every state change is a
    journal event, and an auditor reading an export sees each name's origin without knowing the
    rule. A derived name is ugly by construction, and that is its cost: it is unambiguous,
    collision-free and reproducible, and a row whose name matters is cheap to recreate.

- **d12 — Moved to ADR-0028 d1.** `oml keys issue-agent` and its `--out` key file are decided in
  ADR-0028 d1, and this record's d1 adds the key's name to the verb as its positional `<key-name>`
  argument, which also becomes the key's principal unless `--principal` names another.

- **d13 — Moved to ADR-0028 d2.** The CLI's reader grant beside every writer grant and
  `--write-only` are decided in ADR-0028 d2, and they apply unchanged to `issue-agent` once this
  record's d1 adds the positional key name to it.

- **d14 — Moved to ADR-0028 d3.** The `oml-admin` plugin is decided in ADR-0028 d3, and once this
  record's d1 adds the positional key name to `issue-agent`, its skill passes the key's name there
  and names the key file after it.

- **d15 — Admin keys read workarea structure in their tenant, by key role and without grants,
  and never claim content.** An admin key reads, across its whole tenant, the workarea read
  (`GET /v2/workareas/{workarea_id}`), the lookup by path (`GET /v2/workareas?path=`), the tree,
  a node's workarea edges and its status read — names, ids, parents, paths, statuses, crew
  records, children, workarea edges, review policy objects and pending transitions — beside the
  crew records, keys, grants and denials it already reads as structure. It reads them by its key
  role, like every admin verb, so no grant and no denial enters: denials attenuate grants, and an
  admin key holds none. On the workarea read an admin key sees every child and every workarea
  edge of the node, `my_effective_verbs` empty (the field lists grant-role verbs) and its own
  permission epoch. It never reads claim content: recall, get, the scoped listing, the events
  tail, the lineage read, claim-edge reads, vocabulary reads and attachment fetch all need reader,
  which the admissibility table never admits on an admin key, so each still answers
  `access.verb_ceiling` at step 1 of the refusal order. The rule is declared once, in the
  [permission matrix](../architecture/roles-and-keys.md#permission-matrix).

    The verbs an admin already holds name workareas — issuance (ADR-0028 d1), grants, denials, reassign,
    re-parent, a workarea's crew — and the CLI that administers the tree runs under an admin
    context, yet every read that finds a workarea's name or id needed reader effective access, so
    an admin could create a node and never read it back. Structure is what an admin already
    decides, so reading it confers nothing over claims. The content ceiling of the two-key rule is
    unchanged: an admin key still carries no grant role, receives no serve, records no verdict and
    reads no claim's content, held or promoted, so "admin is never a reviewer" holds exactly as
    before, and R3 is untouched, since no structure read carries claim content or a claim count.
    This amends ADR-0010 d6 (admin's own verbs gain the structure reads) and ADR-0025 d1 and d14
    (workarea info needs reader effective access or the admin key role, and the workarea read
    answers an admin key).

## Consequences

**What callers see**

- An agent configures `X-OML-Workarea: metals-crew`, an operator runs `oml keys revoke
  claude-laptop`, and a plugin key is one command away: `eval "$(oml keys issue-agent
  claude-laptop --workarea metals-crew)"`.
- An unknown or unreadable name answers what its id would; a malformed one answers
  `request.validation`; only admins and operators ever see `name.taken`.
- Paths are no longer accepted where an id or a name is; `GET /v2/workareas?path=` is the one
  path lookup left.
- An operator in Claude Code asks the `oml-admin` skill (ADR-0028 d3) for a plugin key by name
  and gets two lines to run, neither carrying the value, which sits in a 0600 file.
- `oml workareas tree` and `show` run under an admin context; recall under it still answers
  `access.verb_ceiling`.

**Surface and versioning**

- New: the `name` error family with `name.taken`; the four crew operations; the `crew.created`,
  `crew.updated` and `system.names_assigned` event types; `name` on workarea, crew, key and tenant
  objects; `crew_id` on keys; `name` on the issuance, rotation and bootstrap-claim bodies; the
  `crews` table and the `name` columns with their unique indexes. `workarea.create` and
  `workarea.crew_updated` carry `crew_id` in place of the crew object, and `key.rotated` gains
  `new_name`.
- The crew record, its creation body, `crew.created` and the `crews` table are open shapes whose
  every field is optional but `name`, so a later record can add a crew kind and member profiles
  as new optional fields, columns and payload members — additive on `/v2` — without reshaping or
  renaming anything this record defines; this record decides neither.
- `/v1` changes on the request side only, and only additively: `X-OML-Workarea` and the listing's
  `workarea_id` accept names. Example ids of the four kinds in the OpenAPI document, the agent
  plugin and the reviewer-crews pages are rewritten to UUID text so that no example contradicts
  d3; the other example ids are untouched. `info.version` stays `0.2.0-design`: this record lands
  in the same unreleased revision as ADR-0025, whose bump to `0.2.0-design` covers it.
- `mcp-tools.json` changes the descriptions of the unserved tools that take a workarea; the
  served `oml_remember`, `oml_learn`, `oml_recall` and `oml_events_tail` keep their entries until
  the code that serves names lands, and `_meta.pending_design` names what this record decides for
  them. The tool count is unchanged.

**PoC behaviour that changes**

- The PoC names its root `root` and accepts duplicate and ungrammatical workarea and tenant names
  (migrations/0001_init.sql:22-44); every one of them is named under d11, and new ones are refused.
- The PoC's transport parses every workarea and key reference as a UUID — the
  `X-OML-Workarea` header, the `{key_id}` path parameter and the body fields
  (crates/transports/oml-transport-http/src/lib.rs:585, 831-867, 962-971) — and its CLI accepts
  only UUIDs (crates/bins/oml-cli/src/main.rs:253-259); both classify under d3 instead.
- The crew object stored on workareas (migrations/0004_grants.sql:54) becomes crew records.

**Costs that are not measured**

- Resolving a name is one indexed lookup per named value on the request path; its cost is
  unmeasured, like every request-path cost, and is tracked against [FR-25](../product/roadmap.md).
  No number is stated.
- Immutable, never-released names grow without bound with the rows that hold them; the growth is
  the growth of those rows, which [capacity](../operations/capacity.md) already sizes.

**What stays unbuilt after this iteration** (stated divergences in the README and code comments):
key rotation, and so `new_name`; the operator tier, and so tenant and operator-key names beyond
the PoC's single tenant; the independence axis's crew and manager comparisons.

**Process obligations**

- With the proposal: the record registers at its three points (this file, a
  [register](index.md) row and a `mkdocs.yml` nav entry); `llms.txt` moves to ADR-0001–ADR-0026;
  the D7, D9, D13 and D14 cells of the D-register name ADR-0026; one
  [changelog](../changelog.md) entry records the change.
- On acceptance, and not before: this record's Status line and frontmatter `status` become
  `accepted design` with the acceptance date, its register row's Status cell follows, the Status
  lines of ADR-0008, ADR-0010, ADR-0012, ADR-0018 and ADR-0022 gain "· amended by ADR-0026", and
  ADR-0025's gains it once ADR-0025 is itself accepted.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — Names unique among siblings only, paths as the key** | Two workareas in one tenant could share a name, so a bare name would need a path to mean anything, and a path changes on every re-parent. The maintainer fixed one flat namespace per kind. |
| **d1 — A rename verb, journalled** | A name that can change means a cached or quoted name can silently name another row after a later rename and re-creation. The maintainer fixed names as immutable. |
| **d1 — Names on every kind, artifacts and serves included** | An artifact is addressed by the claim it belongs to (`claim_key` plus `version`) and a serve is single-use; neither is typed by a person, and naming them would add namespaces nobody asked for. |
| **d2 — Unicode names, NFC-normalized and case-insensitive** | Case-insensitive matching needs a locale-independent case map and a fixed Unicode version, two names can look alike and differ in bytes, and none of it survives a URL path, a header or an `ltree` label unescaped. |
| **d2 — Lowercase the input** | A coercion: `Metals-Crew` would silently mean `metals-crew`, which errors.md forbids; refusing teaches the one spelling. |
| **d2 — Allow `_`, `.` or `/`** | `.` is the path separator (d7), `/` is a URL path delimiter that proxies treat differently once escaped, and `_` is not in a DNS label or a Kubernetes name, so a name could not double as either. |
| **d2 — Allow a leading digit (RFC 1123)** | `2027` or `4182` would read as a version or a journal seq in CLI arguments such as `<artifact>@<version>`; a letter first costs `q1-2027` instead of `2027-q1`. |
| **d2 — A longer limit, up to the 1000 characters `ltree` allows** | 63 is the limit of a DNS label and a Kubernetes name; a name longer than that is a description, which the label holds. |
| **d3 — Prefixed ids (`wa_01J…`), as the examples used** | A prefix would make an id self-describing, but the store's ids are UUIDs (postgres-schema.md, the PoC's transport), and a second id spelling is a second thing to parse. |
| **d3 — Accept every UUID spelling a parser takes (simple, braced, URN)** | Four spellings of one id give one request four digests and four cache keys; one accepted form, served lowercase, is the smaller contract. |
| **d3 — Classify by trying the name first, then the id** | A value's meaning would depend on whether a row with that name exists, which is an existence oracle; d2 makes the classes disjoint so no lookup decides the class. |
| **d4 — Resolve names in the MCP facade or the CLI** | A second resolver that can drift from the server's, and a facade that says more than the ring (ADR-0014 d1). |
| **d4 — Rename every `*_id` input to the kind's word (`workarea`, `key`)** | Every `/v1` client and every page changes for no behavioural gain, and a request field would stop matching the response field that carries its id. |
| **d4 — Digest the request with names replaced by ids** | It puts a state lookup ahead of the idempotency record; a client never needs to switch spelling inside one retry. |
| **d4 — Add a name field to claim items and events** | The derived path's last segment already is the name, and a `/v1` response field would grow to carry what is served. |
| **d5 — `request.validation` for a taken name** | It hides a state conflict that can never succeed on retry behind the code for a malformed request, and it would be a 400 where every other permanent-identifier collision is a 409. |
| **d5 — `access.not_found` for a taken name, so no one learns it exists** | Only admins and operators create, and they see the whole namespace anyway; telling them "not found" when they create would be false. |
| **d5 — A per-kind code (`workarea.name_taken`, `key.name_taken`, …)** | Four codes for one rule; `details.kind` names the kind. |
| **d6 — Name the root after its tenant** | Tenant names live in the deployment's namespace and workarea names in the tenant's, so a workarea named like the tenant would collide with the root; every path would start differently in every tenant. |
| **d6 — Keep the PoC's `root`** | `universe` is the design's word for the root (ADR-0018 d4) and the first segment of every path in the doc set. |
| **d7 — Accept a path as a third form** | Redundant once the leaf is unique, stale after every re-parent, and a path is never an authority (ADR-0008 d3); keeping paths out of every reference keeps that literal. |
| **d7 — Retire `GET /v2/workareas?path=`** | A client holding a derived path, from a log line or an export, still needs to find its node; the lookup costs nothing and confers nothing. |
| **d8 — Keep the crew as workarea metadata** | The independence axis has nothing to compare: no relation says two workareas share a crew or that a key belongs to one. |
| **d8 — A crew as a kind of workarea** | A crew is not a governed memory space; grants on it would be a second access mechanism (ADR-0008). |
| **d8 — A key's crew derived from its grants** | A reviewer crew's keys hold grants on other crews' workareas, and a key with grants in two crews would have no single crew. |
| **d8 — Create the crew inline on workarea create** | Two creation paths for one record, and two workareas creating "the same" crew would make two. |
| **d8 — A crew tool on MCP, or crew reads for agents** | Crew records are governance, an admin verb (R1, ADR-0014 d2); an agent reads its workarea's crew in the workarea read and probes nothing else. |
| **d8 — A verb to move a key between crews** | A key's crew fixed for its life is the smaller contract; the axis compares crews as journalled at promotion either way, and a member who changes crews holds a new key. |
| **d8 — Count a verdict from a key with no crew as independent** | An unestablished crew would pass a crew-inequality requirement by omission, which fails open. |
| **d9 — The principal as the name** | A principal holds several keys by design (the two-key rule), so names would collide, and principals carry `/`. |
| **d9 — Derive the name from the principal or the label** | A silent coercion, and two keys would race for one derived name. |
| **d9 — The successor inherits the name, or the name follows the rotation chain** | Both keys are live through the grace window, so one name would name two keys, or name different keys over time. |
| **d10 — Release a key's name at revocation, or after a retention window** | The journal, the evidence export and every runbook that quotes the name would then mean two keys by it, at different times. |
| **d11 — Fill the names in the fold without an event** | A state change with no journal event; an auditor would have to know the rule to learn a name's origin. |
| **d11 — A set-once naming verb for rows without names** | A second creation path for names, and unnamed rows in the meantime, so "accepted everywhere an id is" would not hold. |
| **d11 — Refuse to start until an operator supplies every name** | The upgrade would block on a manual step; a chosen name for a live key is one reissue away. |
| **d11 — Derive legacy key names from principals or labels** | They collide (one principal, several keys) or fail d2 (`crew-procure/agent-7`). |
| **d11 — One crew record per workarea with crew metadata** | Two workareas that recorded the same crew would get two records, which is the failure d8 exists to remove. |
| **d15 — A reader grant on the root for admin keys** | A reader grant confers recall, get and the events tail over the whole tenant: content, which the two-key rule keeps from admin keys, and the admissibility table admits no grant on an admin key at all. |
| **d15 — Keep admin keys blind to structure** | Admins name workareas in every verb they hold, and could not look up the name or id of the workarea they grant, re-parent or reassign, nor run `oml workareas tree` under the context that administers the tree. |
| **d15 — A sixth, structure-reader role** | The role set is fixed at five (ADR-0018 d4), and the admin already decides the structure it would read. |
