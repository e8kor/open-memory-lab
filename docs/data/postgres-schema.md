<!--
id: data/postgres-schema
tier: engineering
status: design draft for review · decided, not built
answers: the shape of every Postgres table in the system of record — columns, constraints, indexes — as a design sketch, not a migration
-->

# PostgreSQL schema

**Status:** design draft for review · decided, not built

PostgreSQL is the system of record: one transaction commits the signed journal event, the metadata
rows it implies, and a durable outbox row. The rules that make that true — the per-tenant hash
chain, the synchronous journal durability domain, the outbox protocol — are declared in
[storage](../architecture/storage.md); tenancy and RLS posture in
[security](../architecture/security.md). This page owns the table shapes. Every DDL block below is
a design sketch, not a migration: column names and types are normative in shape, and the
implementation may adjust spellings (e.g. enum types vs `CHECK` constraints) without a design
change. Nothing here is built or benchmarked.

Conventions used throughout:

- Every table carries `tenant_id` and an RLS policy (sketched [below](#row-level-security)). The
  journal's chain head gives each tenant an independent sequence; nothing orders events across
  tenants.
- A foreign key into a tenant table carries `tenant_id`, `(tenant_id, col)` → `(tenant_id, id)`,
  because a foreign-key check bypasses row security ([keys into tenant
  tables](#keys-into-tenant-tables)).
- Digest columns are `bytea` holding SHA-256 values. Journal and outbox payloads reference content
  only as digests or ciphertext references — never body plaintext (D1, declared in
  [storage](../architecture/storage.md)).
- `uuid` identifiers are server-assigned. The journal is the source of truth; most other tables
  are folds of it (classified in the [last section](#folds-of-the-journal-vs-operational-tables)).

## journal and chain heads

The append-only event log, one hash chain per tenant. Event types and payload shapes are catalogued
in [event-catalog](event-catalog.md).

```sql
CREATE TABLE journal (
    tenant_id    uuid        NOT NULL,
    seq          bigint      NOT NULL,   -- per-tenant; assigned under the chain-head lock
    event_id     uuid        NOT NULL,
    event_type   text        NOT NULL,   -- see event-catalog.md
    prev_hash    bytea       NOT NULL,
    hash         bytea       NOT NULL,
    sig          bytea       NOT NULL,   -- ed25519 over signed_bytes (deployment key)
    signed_bytes bytea       NOT NULL,   -- the envelope; covers digests, never bodies
    payload      jsonb       NOT NULL,   -- digest / ciphertext references only
    principal    text        NOT NULL,
    api_key_id   uuid,                   -- no foreign key: may name a provider-level key
    at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, seq),
    UNIQUE (event_id),
    UNIQUE (tenant_id, event_id)         -- target of the tenant-scoped event references
);

CREATE TABLE journal_chain_heads (
    tenant_id uuid   PRIMARY KEY,
    last_seq  bigint NOT NULL,
    last_hash bytea  NOT NULL,
    last_sig  bytea  NOT NULL    -- verified by the startup fork check
);
```

Chain assignment serializes on `pg_advisory_xact_lock` over the tenant's chain-head row, held to
commit, so commit visibility order equals `seq` order per tenant. The cost is stated plainly: a
tenant's journal writes are single-file; per-tenant write throughput is bounded by that lock and is
unmeasured ([FR-7](../product/roadmap.md), the Postgres scale ceiling).

Append-only enforcement is layered — privileges first, a guard trigger second:

```sql
REVOKE UPDATE, DELETE, TRUNCATE ON journal FROM PUBLIC;
REVOKE UPDATE, DELETE, TRUNCATE ON journal FROM oml_server, oml_lakewriter,
    oml_reconciler, oml_indexer;

CREATE FUNCTION journal_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'journal is append-only';
END $$;

CREATE TRIGGER journal_append_only
    BEFORE UPDATE OR DELETE ON journal
    FOR EACH ROW EXECUTE FUNCTION journal_guard();
CREATE TRIGGER journal_no_truncate
    BEFORE TRUNCATE ON journal
    FOR EACH STATEMENT EXECUTE FUNCTION journal_guard();
```

A superuser can still bypass both; the design treats superuser access as deployment-key-equivalent
compromise, stated in [security](../architecture/security.md).

## names and tenants

Workareas, crews, keys and tenants each carry a name: unique in its namespace, never changed,
never released ([names](../api/http-api.md#names-and-ids),
[ADR-0026](../adr/ADR-0026-names-are-keys.md)). The grammar is one domain, so every named table
checks the same rule; the unique indexes on each table are the authority `name.taken` reports,
and an insert that loses a race to one answers it with nothing journalled.

```sql
-- a lowercase RFC 1035 label, 1-63 characters, that never reads as an id:
-- not the RFC 9562 hex-and-dash form, and not 32 hex digits.
CREATE DOMAIN oml_name AS text CHECK (
    VALUE ~ '^[a-z]([a-z0-9-]{0,61}[a-z0-9])?$'
    AND VALUE !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    AND VALUE !~ '^[0-9a-f]{32}$'
);

-- the tenant registry: provider-level, folded from the provider chain's tenant.created
-- events; read by the operator path's own policy, never by a tenant session. The reserved
-- provider tenant has no tenant.created and is not a row: it carries no name.
CREATE TABLE tenants (
    id         uuid        PRIMARY KEY,
    name       oml_name    NOT NULL UNIQUE,   -- unique per deployment
    prior_name text,                          -- the PoC's creation-time name, kept as history;
                                              -- no reference reads it; NULL on a row created
                                              -- with a name
    created_at timestamptz NOT NULL
);
```

The domain admits the derived form `<prefix>-<32 hex digits>`, because the fold of
`system.names_assigned` writes it; every creation refuses that form in the name check the server
runs before it journals anything ([names](../api/http-api.md#names-and-ids)).

**The migration that adds names.** A migration cannot sign a journal event, and rows that predate
names have no name until the lead journals `system.names_assigned`
([rows created before names](../api/http-api.md#rows-created-before-names)). The PoC's
`tenants.name` and `workareas.name` are already `text NOT NULL` columns holding creation-time
names, duplicates and ungrammatical values among them, the root's `root` included
(migrations/0001_init.sql:22-44), so neither the domain nor a unique constraint can be laid on
them. The migration that adds names therefore:

1. renames `tenants.name` and `workareas.name` to `prior_name`, kept as unconstrained `text`
   history that no reference reads;
2. creates `crews`, and adds a new `name oml_name NULL` to `tenants`, `workareas`, `crews` and
   `api_keys`, `crew_id uuid NULL` to `workareas` and `api_keys`, `path ltree NULL` to
   `workareas`, which the PoC schema does not have, and `lifted_epoch bigint NULL` to
   `key_denials`, which the lift of every denial on an admin key
   ([rule 5](../api/http-api.md#rows-created-before-names)) sets in the commit that names the
   chain;
3. creates the unique constraints shown here on the new `name` columns only. Each admits any
   number of NULLs, `api_keys`' included: its `NULLS NOT DISTINCT` constraint, which makes the
   operator keys one namespace, would make two unnamed keys of a tenant collide, so it is created
   as a unique index on `(tenant_id, name) NULLS NOT DISTINCT WHERE name IS NOT NULL`, the same
   rule once every name is set. `name.taken` has its authority from the first request served.
4. adds the `universe` `CHECK` shown on `workareas`, which ties the name to the root. A check
   constraint "is satisfied if the check expression evaluates to true or the null value"
   ([Check Constraints](https://www.postgresql.org/docs/17/ddl-constraints.html#DDL-CONSTRAINTS-CHECK-CONSTRAINTS),
   fetched 2026-10-31), and the comparison is NULL on a row whose `name` is NULL, so the `CHECK`
   passes every row the migration finds and refuses a non-root `universe` under a root that has
   no name yet, as under one that holds `universe`.

The migration carries no creation-time name into `name`: `system.names_assigned` fills every `name`,
and its `prior_name` entries are read from the journal, never from the renamed columns. The lead,
before it serves, journals that event and folds it into the new columns, deriving each `path`. A
migration of a later schema version, which cannot run before that first start, then sets `name` and
`path` `NOT NULL`, the shape this page shows, and fails, holding that upgrade, if any NULL
remains. A lead that finds a NULL `name` in a chain whose
`system.names_assigned` is already journalled refuses to start, with the `names_assigned`
[doctor](../operations/observability.md#the-doctor-contract) line naming the table. Renaming columns
the earlier binaries read is outside the expand-contract rule, which is why this upgrade stops every
earlier binary before it migrates ([upgrades](../operations/upgrades.md#the-upgrade-order)).

## memories

The fold of submission, review and lifecycle events into the current state of every claim version.
A memory asserts a claim; `claim_key` groups its versions
([audit-and-lineage](../architecture/audit-and-lineage.md) owns the chain and tip semantics).

```sql
CREATE TABLE memories (
    artifact_id       uuid        NOT NULL,
    tenant_id         uuid        NOT NULL,
    workarea_id       uuid        NOT NULL REFERENCES workareas (id),
    claim_key         text        NOT NULL,
    version           integer     NOT NULL,
    state             text        NOT NULL
        -- 'rejected' is the fold of review.reject; review.reopen returns the
        -- row to 'held'. Rejected rows never reach the lake, so silver's
        -- state list is unchanged.
        CHECK (state IN ('held', 'promoted', 'demoted', 'rejected', 'erased')),
    is_tip            boolean     NOT NULL DEFAULT false,
    self_signed       boolean     NOT NULL DEFAULT false,  -- constitution development exception (ADR-0001) only
    writer_identity   text        NOT NULL,
    author_key_id     uuid        NOT NULL REFERENCES api_keys (id),
                                          -- the authoring key (envelope api_key_id of the submission):
                                          -- "own" is this column (ADR-0025 d3), never the principal
    retirement        boolean     NOT NULL DEFAULT false,
                                          -- structural, like term_status: survives shredding; a
                                          -- ratified retirement version as tip retires the claim
    replaces_artifact_id uuid,            -- a supersession's replaced version; NULL for version 1
    producer          text,
    provenance        text        NOT NULL
        CHECK (provenance IN ('agent', 'human', 'connector', 'simulation')),
    simulation_id     uuid        REFERENCES simulations (id),
    sim_derived_ids   uuid[]      NOT NULL DEFAULT '{}',   -- taint: simulations in lineage closure
    manifest_id       uuid,       -- reserved: dataset imports (ADR-0020, post-v1); NULL for organic claims
    review_scope      text,       -- reserved: 'sampled(<manifest>)' | 'individual' (ADR-0020); NULL = individual
    terms             text[]      NOT NULL DEFAULT '{}',  -- CURIE annotations (ADR-0021); clear,
                                          -- non-erasable metadata, listed in the erasure inventory
    term_status       text        CHECK (term_status IN ('active', 'deprecated')),
                                          -- kind=term only: the structural deprecation marker (ADR-0021)
    plaintext_digest  bytea,   -- journalled in the submission event: content identity
    body_bytes        integer     NOT NULL,  -- UTF-8 octets of the NFC body, from the submission
                                             -- event (submission.* or simulation.write);
                                             -- the budget-token count derives from it
                                             -- (retrieval Token budgets), and it survives shredding
    ciphertext_digest bytea,   -- journalled in the same event: the key-less reconciler's comparand
    body_ref          jsonb       NOT NULL,  -- {class: quarantine|object|lake, ref: …}
    kind              text        NOT NULL,  -- memory | derived_fact | steering | term |
                                             -- term_mapping | vocab_release (+ dataset_manifest, ADR-0020)
    created_at        timestamptz NOT NULL,
    promoted_at       timestamptz,
    PRIMARY KEY (tenant_id, artifact_id, version),
    CHECK (provenance <> 'simulation' OR simulation_id IS NOT NULL)
);

-- claim_key scope: one live claim per (tenant, workarea); a version number
-- is unique among held, promoted and demoted versions, so a claim whose
-- every version is rejected or erased releases its claim_key for a fresh
-- version 1, and a demoted one never does (ADR-0025 d18). A successor's
-- ordinal is 1 + the greatest version of the claim_key in any state, so
-- competing held successors of one tip carry distinct ordinals and coexist;
-- this index refuses only a capture that lost the ordinal race, which the
-- server retries, re-running the claim_key collision test: a lost version 1
-- answers submission.claim_key_conflict, and only a successor takes a fresh
-- ordinal (audit-and-lineage owns the rule).
CREATE UNIQUE INDEX memories_claim_version
    ON memories (tenant_id, workarea_id, claim_key, version)
    WHERE state IN ('held', 'promoted', 'demoted');

-- the default served tip: at most one version of a claim is what
-- non-audience readers get. Taint never enters this index: a tainted
-- version shadows the tip only for its simulation's audience (below).
CREATE UNIQUE INDEX memories_served_tip
    ON memories (tenant_id, workarea_id, claim_key)
    WHERE is_tip AND state = 'promoted'
      AND provenance <> 'simulation' AND sim_derived_ids = '{}';

-- audience overlay tips: per simulation, a tainted promoted version may
-- shadow a claim's tip for that simulation's audience only. Simulation end
-- demotes overlay rows; the default tip above never moved.
-- (a lineage-tainted row with NULL simulation_id keys its overlay by the
-- sole live simulation in sim_derived_ids, resolved at promotion time)
CREATE UNIQUE INDEX memories_overlay_tip
    ON memories (tenant_id, workarea_id, claim_key, simulation_id)
    WHERE is_tip AND state = 'promoted'
      AND (provenance = 'simulation' OR sim_derived_ids <> '{}');

CREATE INDEX memories_workarea ON memories (tenant_id, workarea_id, state);

-- the own-work listing: every row a key authored, newest first (ADR-0025 d13).
CREATE INDEX memories_author ON memories (tenant_id, author_key_id, created_at DESC);
```

The workarea-qualified index above is the enforceable spelling of the one-served-tip rule:
`claim_key` is namespaced by workarea, and a reassignment that would collide on `claim_key` is
refused unless the move event records a re-key, so `(tenant, workarea_id, claim_key)` is the
identity a tip is unique under — a `(tenant, claim_key)` shorthand would be either too strict
(one tip across workareas) or unenforceable.
Three invariants live in transaction logic, not DDL:

- A supersession's ratification compare-and-sets that its target is still the current tip of the
  successor's own `(workarea_id, claim_key)` before flipping `is_tip` and journalling
  `lifecycle.supersede`. The compare-and-set runs inside round trip two's data-modifying-CTE
  statement ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d1), never before `BEGIN`:
  it is the conditional
  `UPDATE memories SET is_tip = false WHERE artifact_id = $target AND workarea_id = $successor_workarea_id AND claim_key = $successor_claim_key AND is_tip AND state = 'promoted'`,
  with the successor's `workarea_id` and `claim_key` read from its row in that statement, and
  every other write of that statement — the journal insert, the fold that promotes the
  successor and makes it the tip, the `held_fts` DELETE, the outbox rows and the serve's
  consumption — is conditioned on the row it returns. A zero row count writes nothing: the
  transaction rolls back and the verdict answers `submission.tip_conflict`. A target moved to
  another workarea or re-keyed after the successor was captured fails the compare-and-set, and so
  does a successor moved or re-keyed away from its target: the verdict answers
  `submission.tip_conflict`, nothing is journalled, and the successor stays held until a reviewer
  rejects it. The `held_fts` DELETE's affected-row count is asserted in that statement
  ([row-level security](#row-level-security)). A tainted successor's ratification compares the
  target under the same predicate, leaves a default-tip target's `is_tip` as it is and sets the
  overlay tip instead ([audit-and-lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).
- A version ordinal is read before `BEGIN`, and a capture that `memories_claim_version` refuses
  because a concurrent capture took that ordinal is retried by the server, never answered as a
  constraint error; the retry re-runs the capture's pre-`BEGIN` reads, the `claim_key` collision
  test included, so a lost version 1 answers `submission.claim_key_conflict` and only a successor
  takes a fresh ordinal.
- A claim moved to a stricter-policy workarea re-enters `held`.

A superseded version keeps `state = 'promoted'` with `is_tip = false`
([audit-and-lineage](../architecture/audit-and-lineage.md) owns the tip semantics).

### Guardrail columns

**Status:** built in `migrations/0016_guardrails.sql` for the proposed
[ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
(d2, d8, d10, d14); the record binds nothing before its acceptance.

```sql
ALTER TABLE memories
    ADD COLUMN severity text CHECK (severity IN ('never', 'must', 'should')),
    ADD COLUMN rule     jsonb,  -- the oml-gr/1 rule as admitted; the submission journals its digest
    ADD CONSTRAINT memories_guardrail_fields_check
        CHECK ((severity IS NULL OR (kind = 'guardrail' AND NOT retirement))
               AND (severity IS NOT NULL OR kind <> 'guardrail' OR retirement)
               AND (rule IS NULL OR severity IS NOT NULL));
```

Every guardrail version but a retirement carries a severity, and no other row carries either
column. The build stores the rule inline beside the body, as it stores the body; d14's sealing of
the rule under the version's DEK waits for the encrypted body store.

## workareas and edges

**Status:** built for the status ladder: `workareas.status` is one stored column, written at
create (`development`, and `committed` on the universe root), moved by a ratified transition and
served back by every workarea read; `workarea_transitions` is built in
`migrations/0017_workarea_transitions.sql`, which also adds `workareas_universe_committed`, so a
root on any other rung is refused; its header says every earlier root was stored `committed`, which is false
for a root v0.1.0 created, so the lead stores each node's served status before 0017 runs
([upgrades](../operations/upgrades.md#upgrading-from-v051-or-earlier-the-upgrade-to-v054)); a proposal inserts its pending row and a ratification turns it
`ratified` and writes the node's status in the commit that journals the event, after the chain
lock's re-checks ([ADR-0009](../adr/ADR-0009-status-ladder.md) d1, d3, d4) · its checks run only
where a superuser mints the suites' role pairs, CI's `cargo test (Postgres)`
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d8) · re-parent, the
`path` rewrite and `workarea_edges` are decided, not built.

The governed tree (one per tenant, rooted at `universe`) plus the two edge tables. The algebra —
node-id grants, deny semantics, status ladder — is declared in
[workareas](../architecture/workareas.md); this page holds only the shapes.

```sql
CREATE EXTENSION IF NOT EXISTS ltree;

CREATE TABLE crews (                -- named records, recorded not operated (ADR-0026 d8)
    id               uuid        PRIMARY KEY,
    tenant_id        uuid        NOT NULL,
    name             oml_name    NOT NULL,   -- immutable; OML's key for the crew
    charter_uri      text,
    external_manager text,
    created_at       timestamptz NOT NULL,
    UNIQUE (tenant_id, name),
    UNIQUE (tenant_id, id)                   -- target of tenant-scoped crew references
);

CREATE TABLE workareas (
    id         uuid        PRIMARY KEY,
    tenant_id  uuid        NOT NULL,
    parent_id  uuid,                   -- NULL only for universe
    name       oml_name    NOT NULL,   -- unique in the tenant; never changed
    prior_name text,                   -- the PoC's creation-time name, kept as history;
                                       -- no reference reads it; NULL on a row created with a name
    path       ltree       NOT NULL,   -- DERIVED from names; rewritten in one transaction on re-parent
    status     text        NOT NULL
        CHECK (status IN ('development', 'committed', 'validated')),
    crew_id    uuid,                   -- the crew record this workarea names, or NULL
    user_root  boolean     NOT NULL DEFAULT false,
                                       -- universe.<user>, created by the first key naming its
                                       -- user; fixed at creation (ADR-0044 d1)
    namespace_root_id uuid,            -- the user root whose namespace holds the name; NULL in
                                       -- the shared namespace, user roots included (ADR-0044 d5)
    created_at timestamptz NOT NULL,
    UNIQUE (tenant_id, namespace_root_id, name),  -- unique within one user root's namespace
    UNIQUE (tenant_id, id),            -- target of the tenant-scoped grant and denial references
    FOREIGN KEY (tenant_id, parent_id) REFERENCES workareas (tenant_id, id),
    FOREIGN KEY (tenant_id, crew_id) REFERENCES crews (tenant_id, id),
    FOREIGN KEY (tenant_id, namespace_root_id) REFERENCES workareas (tenant_id, id),
    CHECK ((parent_id IS NULL) = (name = 'universe')),  -- the root, and only the root, is universe
    CHECK (NOT user_root OR (parent_id IS NOT NULL AND namespace_root_id IS NULL)),
    CONSTRAINT workareas_universe_committed             -- the root is permanently committed
        CHECK (parent_id IS NOT NULL OR status = 'committed')   -- (ADR-0009 d1; added by 0017)
);

-- unique within the shared namespace: universe, every user root and every
-- shared workarea (ADR-0044 d5; migrations/0014_user_roots.sql).
CREATE UNIQUE INDEX workareas_shared_name ON workareas (tenant_id, name)
    WHERE namespace_root_id IS NULL;

-- exactly one universe root per tenant; it is permanently 'committed'.
CREATE UNIQUE INDEX workareas_universe ON workareas (tenant_id)
    WHERE parent_id IS NULL;
CREATE INDEX workareas_path ON workareas USING gist (path);

CREATE TABLE workarea_edges (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL,
    src_id          uuid NOT NULL REFERENCES workareas (id),
    dst_id          uuid NOT NULL REFERENCES workareas (id),
    label           text NOT NULL
        CHECK (label IN ('relates_to', 'continues', 'derived_from', 'mirrors')),
    state           text NOT NULL CHECK (state IN ('pending', 'ratified')),
    proposer_key_id uuid NOT NULL REFERENCES api_keys (id),
    ratifier_key_id uuid REFERENCES api_keys (id),
    CHECK (src_id <> dst_id),
    CHECK (ratifier_key_id IS NULL OR ratifier_key_id <> proposer_key_id)
);

-- Built in migrations/0017_workarea_transitions.sql: status-ladder proposals (ADR-0009
-- d3), the build's reading on architecture/workareas.md. Several pending rows may share a workarea; ratification,
-- under the chain lock, re-checks from_status against workareas.status (a stale row is
-- refused) and, for a step to validated, that every child is validated. A stale row stays
-- 'pending'; the status read lists only pending rows whose from_status is the node's status.
CREATE TABLE workarea_transitions (
    id                 uuid   PRIMARY KEY,           -- the events' transition_id
    tenant_id          uuid   NOT NULL,
    workarea_id        uuid   NOT NULL,
    from_status        text   NOT NULL
        CHECK (from_status IN ('development', 'committed', 'validated')),
    to_status          text   NOT NULL
        CHECK (to_status IN ('development', 'committed', 'validated')),
    state              text   NOT NULL CHECK (state IN ('pending', 'ratified')),
    proposer_key_id    uuid   NOT NULL,
    proposer_principal text   NOT NULL,              -- R2 is checked on the principal too
    ratifier_key_id    uuid,
    proposed_seq       bigint NOT NULL,              -- seq of workarea.status_proposed
    ratified_seq       bigint,                       -- seq of workarea.status_transition
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, workarea_id) REFERENCES workareas (tenant_id, id),
    FOREIGN KEY (tenant_id, proposer_key_id) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, ratifier_key_id) REFERENCES api_keys (tenant_id, id),
    CHECK ((from_status, to_status) IN (('development', 'committed'),
                                        ('committed', 'validated'))),
    CHECK ((state = 'ratified') = (ratifier_key_id IS NOT NULL)),
    CHECK ((state = 'ratified') = (ratified_seq IS NOT NULL)),
    CHECK (ratifier_key_id IS NULL OR ratifier_key_id <> proposer_key_id)
);
CREATE INDEX workarea_transitions_by_state                  -- the status read's pending list
    ON workarea_transitions (tenant_id, workarea_id, state);

```

The key-inequality `CHECK` is the cheap spelling of R2 on edges; the full rule — ratifier principal
distinct from proposer principal, resolved at ratification time — is enforced in the ratification
transaction, because a principal can hold several keys and a column constraint cannot see that.
`path` is derived: grants bind to node ids, and every path is recomputable from `parent_id` and
the names, so a re-parent is a single-transaction descendant rewrite rather than a grant migration.
Every name is a valid `ltree` label — PostgreSQL 17 admits "alphanumeric characters, underscores,
and hyphens" up to 1000 characters per label
([ltree](https://www.postgresql.org/docs/17/ltree.html), fetched 2026-10-30) — so every path is.

### claim_edges

**Status:** built in `migrations/0018_claim_edges.sql` · its checks run only where a superuser
mints the suites' role pairs, CI's `cargo test (Postgres)`
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d8). `oml-pg`'s
claim-edge statements and `oml-app`'s proposal, ratification and page use cases that write and
read it are built; no HTTP route or MCP tool reaches them yet.

A labelled edge between two claims ([ADR-0008](../adr/ADR-0008-workarea-algebra.md) d5), `pending`
until a key that is not its proposer ratifies it, and never deleted:

```sql
CREATE TABLE claim_edges (
    id              uuid NOT NULL PRIMARY KEY,
    tenant_id       uuid NOT NULL REFERENCES tenants (id),
    src_artifact_id uuid NOT NULL,
    dst_artifact_id uuid NOT NULL,
    label           text NOT NULL,     -- derives_from | relates_to | oml:is_a | …
    state           text NOT NULL
        CONSTRAINT claim_edges_state CHECK (state IN ('pending', 'ratified')),
    proposer_key_id uuid NOT NULL,
    ratifier_key_id uuid,
    created_at      timestamptz(6) NOT NULL DEFAULT now(),  -- its claim_edge.proposed's at
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, src_artifact_id) REFERENCES memories (tenant_id, id),
    FOREIGN KEY (tenant_id, dst_artifact_id) REFERENCES memories (tenant_id, id),
    FOREIGN KEY (tenant_id, proposer_key_id) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, ratifier_key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT claim_edges_ratifier_is_not_proposer
        CHECK (ratifier_key_id IS NULL OR ratifier_key_id <> proposer_key_id),
    CONSTRAINT claim_edges_ratified_has_ratifier
        CHECK ((state = 'ratified') = (ratifier_key_id IS NOT NULL))
);

-- Statistics target 0 on every column, before any ANALYZE (FR-45).
ALTER TABLE claim_edges
    ALTER COLUMN id SET STATISTICS 0, ALTER COLUMN tenant_id SET STATISTICS 0,
    ALTER COLUMN src_artifact_id SET STATISTICS 0, ALTER COLUMN dst_artifact_id SET STATISTICS 0,
    ALTER COLUMN label SET STATISTICS 0, ALTER COLUMN state SET STATISTICS 0,
    ALTER COLUMN proposer_key_id SET STATISTICS 0, ALTER COLUMN ratifier_key_id SET STATISTICS 0,
    ALTER COLUMN created_at SET STATISTICS 0;

CREATE INDEX claim_edges_src ON claim_edges (tenant_id, src_artifact_id);
CREATE INDEX claim_edges_dst ON claim_edges (tenant_id, dst_artifact_id);
CREATE INDEX claim_edges_ratified_label ON claim_edges (tenant_id, label)
    WHERE state = 'ratified';                -- the cycle refusal's read (ADR-0021)
CREATE INDEX claim_edges_listing ON claim_edges (tenant_id, created_at DESC, id DESC);
                                             -- the graph read, newest first
```

- **Keys.** Each key into `memories` and `api_keys` carries `tenant_id`, so an edge naming
  another tenant's memory or key is refused as one naming no row is: `23503`, from the one
  constraint ([Keys into tenant tables](#keys-into-tenant-tables)). A NOT NULL `tenant_id` never
  matches a provider-level key, so an operator key neither proposes nor ratifies.
- **Checks.** `claim_edges_ratifier_is_not_proposer` is R2's key-inequality spelling
  ([ADR-0012](../adr/ADR-0012-admission-and-review.md)); the principal and lineage rule is
  the ratification transaction's. `claim_edges_ratified_has_ratifier` makes a row ratified
  exactly when it names a ratifier. Each refuses with `23514`.
- **Order.** `created_at` is the `at` of the edge's `claim_edge.proposed` event, and
  `GET /v2/edges?kind=claim` lists by it newest first, then by `id` descending, as every list
  endpoint does ([http-api.md](../api/http-api.md#pagination)).
- **States.** `pending` and `ratified` only. A `rejected` state and a `term_curie` column belong
  to ADR-0027, which is not accepted; the domain's `EdgeState::Rejected` is not stored.
- **Fence.** Row-level security is enabled and forced, under the `tenant_isolation` policy
  `memories` carries ([Row-level security](#row-level-security)); the serving role holds
  `SELECT`, `INSERT` and `UPDATE`, and no `DELETE` ([Privileges](#privileges)).
- **Not partitioned; no column statistics.** A label is tenant text: the domain admits any
  non-empty NFC label outside `oml:` ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) d5),
  such as `acme:merger_target`. In one unpartitioned table a column's statistics span every
  tenant, and EXPLAIN's estimate reads them under row security, since `texteq` and `uuid_eq` are
  leakproof, so a session set to tenant A would learn whether another tenant uses a label or
  holds an endpoint id: the FR-45 channel ADR-0034 d11 closed for `memories` with partitions.
  0018 sets statistics target 0 on every column before any `ANALYZE`; ANALYZE then collects
  nothing, and each predicate takes PostgreSQL's default selectivity whatever any tenant stores
  (`under_tenant_a_explain_estimates_tenant_bs_edge_label_as_it_estimates_an_unused_one`, with
  the default target as its control). The cost: edge reads plan on defaults, never on a label's
  or an endpoint's skew. **Stays open** (FR-45): `pg_class`'s `reltuples` and `relpages` count
  every tenant's edges together. This is a reading 0018's header records, not a decision of
  ADR-0034, which lists target 0 as an FR-45 candidate.

### brief_edges

**Status:** built in `migrations/0019_brief_edges.sql` since v0.5.2
([ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) d7, d8) · its checks run only where a superuser mints the suites'
role pairs, CI's `cargo test (Postgres)`.

```sql
CREATE TABLE brief_edges (
    id              uuid NOT NULL PRIMARY KEY,
    tenant_id       uuid NOT NULL,
    artifact_id     uuid NOT NULL,              -- the member claim's version
    workarea_id     uuid NOT NULL,              -- the workarea whose brief it joins
    state           text NOT NULL CHECK (state IN ('pending', 'ratified')),
    proposer_key_id uuid NOT NULL,
    ratifier_key_id uuid,
    ratified_seq    bigint,                     -- the review.ratify's seq: the brief's order
    created_at      timestamptz(6) NOT NULL,    -- brief_edge.proposed's at: the listing's order
    -- keys carry tenant_id into tenants, memories, workareas and api_keys;
    -- ratifier <> proposer; ratified <=> a ratifier <=> a ratified_seq
);
CREATE INDEX brief_edges_workarea ON brief_edges (tenant_id, workarea_id, created_at DESC, id DESC);
```

- **Apart from `claim_edges`.** A brief edge joins a claim to a workarea, not two claims, and its
  fold widens no lineage closure: membership in a brief says where a claim is read first, not
  what it derives from. Its label is `brief`, fixed, and not stored.
- **Fence, keys and statistics** as `claim_edges`: row security forced under
  `tenant_isolation`, `SELECT`, `INSERT` and `UPDATE` granted and no `DELETE`, every key carrying
  `tenant_id`, statistics target 0 on every column.
- **Limit.** The brief and the listing read a workarea's edges whole; the count per workarea is
  unbounded, tracked with the brief's budget as FR-77.

## api_keys, grants, key_denials, bans

Authorization state. Key values are never stored — only their hash; issuance is journalled as
issued-without-value (D9, declared in [roles-and-keys](../architecture/roles-and-keys.md)).

```sql
CREATE TABLE api_keys (
    id         uuid        PRIMARY KEY,
    tenant_id  uuid,                    -- NULL for provider-level operator keys
    hash       bytea       UNIQUE,      -- SHA-256 of the value; always NULL for a connector key,
                                        -- and NULL for a bearer key only after a re-fold
    credential text        NOT NULL     -- folded from key.issued.credential (ADR-0031 d7)
        CHECK (credential IN ('bearer', 'connection')),
    role       text        NOT NULL
        CHECK (role IN ('reader', 'writer', 'dreamer', 'admin', 'operator')),
    principal  text        NOT NULL,
    issued_by  uuid,                    -- issuer ceiling checked at issuance; no foreign key:
                                        -- an operator key issues a tenant's first admin key
    rotated_from uuid,                  -- successor link; predecessor revoked at grace end
    name       oml_name    NOT NULL,      -- the key's name; never changed, never released
    crew_id    uuid,                      -- the crew record named at issuance; fixed for life
    user_root_id uuid,                    -- the user root of a user key; NULL for a tenant key;
                                          -- fixed for life (ADR-0044 d2)
    held_by    text        NOT NULL DEFAULT 'agent'
        CHECK (held_by IN ('agent', 'human', 'service')),
                                          -- issuance-time attestation by the issuing admin
                                          -- (ADR-0021): verified as an attestation, never as fact;
                                          -- human_required_classes consumes it — only 'human'
                                          -- satisfies it; 'service' never counts as review capacity
    default_workarea_id uuid,
                                          -- provider-ring target (ADR-0022 d4): a /v1 request
                                          -- names no workarea — this, or a sent workarea, resolves it
    issued_seq bigint      NOT NULL,      -- journal seq of this key's key.issued, or of the
                                          -- key.rotated that minted it; the floor of its
                                          -- permission epoch (ADR-0025 d14)
    expiry     timestamptz,
    revoked_at timestamptz,
    UNIQUE NULLS NOT DISTINCT (tenant_id, name),
                                          -- unique per tenant; operator keys (tenant_id NULL)
                                          -- share the provider's one namespace
    UNIQUE (tenant_id, id),               -- target of the tenant-scoped grant and denial references
    FOREIGN KEY (tenant_id, rotated_from) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, default_workarea_id) REFERENCES workareas (tenant_id, id),
    FOREIGN KEY (tenant_id, crew_id) REFERENCES crews (tenant_id, id),
    FOREIGN KEY (tenant_id, user_root_id) REFERENCES workareas (tenant_id, id),
    CHECK (credential = 'bearer' OR hash IS NULL)  -- a connector key never has a hash
);

CREATE TABLE grants (
    id               uuid   PRIMARY KEY,
    tenant_id        uuid   NOT NULL,
    key_id           uuid   NOT NULL,
    role             text   NOT NULL
        CHECK (role IN ('reader', 'writer', 'dreamer')),  -- admissible per key role:
                                          -- roles-and-keys Grant admissibility (ADR-0025 d1)
    workarea_node_id uuid   NOT NULL,               -- node id, never a path
    deny             jsonb  NOT NULL DEFAULT '[]',  -- subtree masks inside THIS grant only
    covers           text   NOT NULL DEFAULT 'subtree'
        CHECK (covers IN ('subtree', 'node')),      -- the node's subtree, or the node alone
                                                    -- (ADR-0044 d4)
    epoch            bigint NOT NULL,               -- journal seq of this grant's grant.issued,
                                                    -- or of the key.rotated that reproduced it
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, workarea_node_id) REFERENCES workareas (tenant_id, id),
    CHECK (covers = 'subtree' OR deny = '[]'::jsonb)  -- a node grant masks nothing
);

CREATE TABLE key_denials (          -- global per-key deny: deny-wins across all grants
    id               uuid        PRIMARY KEY,
    tenant_id        uuid        NOT NULL,
    key_id           uuid        NOT NULL,
    workarea_node_id uuid        NOT NULL,
    epoch            bigint      NOT NULL,   -- journal seq of this denial's key.denied
    lifted_epoch     bigint,                 -- journal seq of the key.denial_lifted that
                                             -- lifted it; NULL while the denial is in force
    created_by       uuid        NOT NULL,
    created_at       timestamptz NOT NULL,
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, workarea_node_id) REFERENCES workareas (tenant_id, id)
);

CREATE TABLE bans (
    id           uuid        PRIMARY KEY,
    tenant_id    uuid        NOT NULL,
    subject_kind text        NOT NULL CHECK (subject_kind IN ('principal', 'source')),
    subject      text        NOT NULL,
    banned_by    uuid        NOT NULL,
    banned_at    timestamptz NOT NULL,
    lifted_by    uuid,
    lifted_at    timestamptz
);

-- one active ban per subject; a lifted ban stays as history.
CREATE UNIQUE INDEX bans_active ON bans (tenant_id, subject_kind, subject)
    WHERE lifted_at IS NULL;
```

Effective access is computed, never stored, on every request: union of a key's grants, each
attenuated by its own `deny` entries, then masked by any `key_denials` row whose `lifted_epoch` is
NULL. A lifted denial masks nothing and its row stays as history
([event catalog](event-catalog.md#keydenial_lifted)). The `epoch` column is
the journal seq of the event that wrote the row, and `lifted_epoch` the seq of the lift, which
makes every historical computation reproducible from journalled grants ([workareas](../architecture/workareas.md) owns the
permission-epoch semantics).
A grant's and a denial's key and node references carry the row's `tenant_id`. In PostgreSQL,
"referential integrity checks, such as unique or primary key constraints and foreign key
references, always bypass row security"
([Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html), fetched
2026-10-30), so a reference on `id` alone would accept a row naming another tenant's key or node.
A provider-level key (`tenant_id` NULL) matches no such reference, and carries no grant
([grant admissibility](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)).
Bans are checked at admission; a ban's reach is previewable (`--dry-run`) before it is imposed.

`api_keys.guardrail_scope text NOT NULL DEFAULT 'ancestors' CHECK (guardrail_scope IN
('workarea', 'ancestors'))`, built in `migrations/0016_guardrails.sql`, holds the scope the key's
issuance set (proposed ADR-0037 d4). A key issued before it holds `ancestors`, as its
`key.issued` without the field folds; no journal event is rewritten.

## Keys into tenant tables

**Status:** built in `migrations/0008_tenant_keys.sql` · its checks run only where a superuser
mints the suites' role pairs, CI's `cargo test (Postgres)`, and in CI's `rls-spike`
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d8, d9).

A foreign key into a table that has a `tenant_id` column pairs the referencing row's `tenant_id`
with the referenced row's, the form the grant, denial and crew references above take. A foreign-key
check bypasses row security (the quotation above), so a key on `id` alone accepts another tenant's
row while it refuses an id no row holds, and the difference tells a tenant whether an id exists
anywhere. Under the
pair, a reference to another tenant's row is refused as one to no row is: `23503`, from the one
constraint (ADR-0034 d12). The keys built this way:

| Reference | Key | Target |
|---|---|---|
| `workareas.parent_id` | `(tenant_id, parent_id)` | `workareas (tenant_id, id)` |
| `api_keys.default_workarea_id` | `(tenant_id, default_workarea_id)` | `workareas (tenant_id, id)` |
| `api_keys.rotated_from` | `(tenant_id, rotated_from)` | `api_keys (tenant_id, id)` |
| `api_keys.user_root_id` | `(tenant_id, user_root_id)` | `workareas (tenant_id, id)` |
| `workareas.namespace_root_id` | `(tenant_id, namespace_root_id)` | `workareas (tenant_id, id)` |
| `memories.created_event_id` | `(tenant_id, created_event_id)` | `journal (tenant_id, event_id)` |
| `review_queue.memory_id` | `(tenant_id, memory_id)` | `memories (tenant_id, id)` |
| `claim_edges.src_artifact_id`, `dst_artifact_id` | `(tenant_id, src_artifact_id)`, `(tenant_id, dst_artifact_id)` | `memories (tenant_id, id)` (0018) |
| `claim_edges.proposer_key_id`, `ratifier_key_id` | `(tenant_id, proposer_key_id)`, `(tenant_id, ratifier_key_id)` | `api_keys (tenant_id, id)` (0018) |

- **The targets.** 0008 adds `UNIQUE (tenant_id, event_id)` on `journal` and `UNIQUE (tenant_id,
  id)` on `memories`; `workareas` and `api_keys` have theirs from 0004. Each key keeps its 0001
  constraint name. 0009 makes `(tenant_id, id)` `memories`' primary key, which its partitions
  require, drops 0008's constraint on the same columns with the old table, and points
  `review_queue_memory_id_fkey` at the primary key ([per-tenant
  partitions](#the-built-fts-index-per-tenant-partitions)).
- **Provider-level keys.** On an `api_keys` row whose `tenant_id` is NULL, an operator key's, "the
  row is not required to have a match"
  ([CREATE TABLE](https://www.postgresql.org/docs/17/sql-createtable.html), fetched 2026-10-31):
  the keys are `MATCH SIMPLE`.
- **No foreign key.** `journal.api_key_id` and `api_keys.issued_by` name a provider-level key by
  design, as when an operator key issues a tenant's first admin key
  ([HTTP API](../api/http-api.md)), so no key carrying `tenant_id` could match them. 0008 drops
  both, as 0002 dropped `review_queue.held_by_key`'s; each holds the id of the key its request
  authenticated with, which the use case has read and no client names.
- **Existing rows.** Adding a key validates every row, without row security, so a database that
  already holds a cross-tenant reference fails 0008 with `23503`, naming the constraint and the
  key, and commits nothing.
- **Designed references.** A sketch on this page that still shows `REFERENCES t (id)` into a
  tenant table is a column built with no key, or not built; it takes the pair when its migration
  lands.

## connections (ADR-0031)

**Status:** built in `migrations/0012_connections.sql` (`api_keys.credential`, `connect_codes`,
`oauth_connections`, `oauth_tokens`) and `migrations/0013_oauth_clients.sql` (`oauth_clients`,
`oauth_authorizations`), and in `oml-pg`'s connection store · its checks run only where a superuser
mints the suites' role pairs, CI's `cargo test (Postgres)`
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md),
[ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d8).

Connect codes, connections, their tokens, and the deployment-level state the authorization server
keeps before a code names a key ([roles and keys](../architecture/roles-and-keys.md#connections),
[HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)). No code, token or
authorization code is stored, only its SHA-256, and no idempotency record holds a connect code
([HTTP API — idempotency](../api/http-api.md#idempotency)).

```sql
CREATE TABLE oauth_clients (           -- clients the authorization server knows; deployment-level
    client_id       text        PRIMARY KEY,   -- a metadata document's https URL, or server-minted
    tenant_id       uuid        NOT NULL,      -- always the reserved provider tenant
    kind            text        NOT NULL CHECK (kind IN ('cimd', 'dcr')),
    client_name     text        NOT NULL,
    redirect_uris   text[]      NOT NULL,      -- each inside the redirect allowlist
    metadata_digest bytea       NOT NULL,      -- SHA-256 of the document or registration as received
    fetched_at      timestamptz,               -- cimd: when the cached document was fetched;
                                               -- kept at most 24 hours from it; at most 10 000
                                               -- cimd rows, the least recently used one with no
                                               -- connected_at evicted first (ADR-0031 d5)
    max_age         integer     CHECK (max_age BETWEEN 0 AND 86400),
                                               -- cimd: its Cache-Control lifetime in seconds
                                               -- from fetched_at, capped at 24 hours; past it
                                               -- the copy is served only when a fresh fetch
                                               -- fails or a fetch cap is full (ADR-0031 d5)
    last_used_at    timestamptz,               -- cimd: the eviction order
    connected_at    timestamptz,               -- the first Connect that named this client; a
                                               -- refetch keeps it; read by eviction and by the
                                               -- removal of a dcr row (ADR-0031 d5)
    created_at      timestamptz NOT NULL
);

CREATE TABLE oauth_authorizations (    -- an authorization request whose connect code verified;
                                       -- before that the request is only its sealed handle and
                                       -- has no row (ADR-0031 d9)
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL,       -- always the reserved provider tenant
    handle_hash    bytea       NOT NULL UNIQUE,-- SHA-256 of the confirmation's 256-bit handle
    client_id      text        NOT NULL,       -- no reference: a cached document may be evicted
    client_kind    text        NOT NULL CHECK (client_kind IN ('cimd', 'dcr')),
    metadata_digest bytea      NOT NULL,       -- copied from the sealed handle
    redirect_uri   text        NOT NULL,
    redirect_uri_sent boolean  NOT NULL,       -- whether the request carried redirect_uri,
                                               -- for the token endpoint's rule
    code_challenge text        NOT NULL,       -- S256 only, 43 base64url characters
    state          text,                       -- at most 512 bytes; echoed verbatim
    expires_at     timestamptz NOT NULL,       -- the sealed handle's: ten minutes after the GET
    code_id        uuid        NOT NULL,       -- the verified connect code; plain uuids, since
    code_tenant_id uuid        NOT NULL,       -- the code is another tenant's row: these three
    key_id         uuid        NOT NULL        -- are copied from it when it verifies
);

CREATE TABLE connect_codes (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    key_id      uuid        NOT NULL,
    hash        bytea       NOT NULL UNIQUE,   -- SHA-256 of the 12 decoded symbols
    issued_seq  bigint      NOT NULL,          -- journal seq of its connection.code_issued
    expires_at  timestamptz NOT NULL,
    redeemed_at timestamptz,                   -- its first correct entry on the page
    voided_seq  bigint,                        -- seq of the connection.code_issued,
                                               -- connection.revoked or key.revoked that voided it
                                               -- (roles-and-keys Connections; ADR-0031 d8)
    UNIQUE (tenant_id, id),                    -- target of oauth_connections' code reference
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id)
);

-- one live code per key: minting a code voids every unredeemed one of the key in its commit.
CREATE UNIQUE INDEX connect_codes_live ON connect_codes (tenant_id, key_id)
    WHERE redeemed_at IS NULL AND voided_seq IS NULL;

CREATE TABLE oauth_connections (     -- a row is created at Connect, state 'authorized'
    id                   uuid        PRIMARY KEY,
    tenant_id            uuid        NOT NULL,
    key_id               uuid        NOT NULL,
    code_id              uuid        NOT NULL,
    client_id            text        NOT NULL,  -- no reference: oauth_clients is the provider tenant's;
                                                -- the token endpoint's client (ADR-0031 d5)
    client_kind          text        NOT NULL CHECK (client_kind IN ('cimd', 'dcr')),
    client_metadata_digest bytea     NOT NULL,  -- these two copied from the authorization request at
                                                -- Connect, for connection.opened
    redirect_uri         text        NOT NULL,
    redirect_uri_sent    boolean     NOT NULL,  -- copied from the authorization request
    resource             text        NOT NULL,  -- <issuer>/mcp at Connect; connection.opened
                                                -- journals it; a token works only while it is
                                                -- the current <issuer>/mcp (ADR-0031 d10, d11)
    code_challenge       text        NOT NULL,
    auth_code_hash       bytea       UNIQUE,    -- SHA-256 of the authorization code; kept after
                                                -- redemption so a second redemption is detected
                                                -- (ADR-0031 d10)
    auth_code_expires_at timestamptz,
    auth_code_redeemed_at timestamptz,          -- set at the token exchange; a presented code
                                                -- whose row has it set goes to the reuse rule
                                                -- (roles-and-keys Connections)
    state                text        NOT NULL CHECK (state IN ('authorized', 'open', 'revoked')),
    opened_seq           bigint,                -- journal seq of its connection.opened
    revoked_seq          bigint,                -- its connection.revoked, or the connection.opened
                                                -- that replaced it
    revoked_cause        text        CHECK (revoked_cause IN
                             ('admin', 'refresh_reuse', 'code_reuse', 'replaced')),
    created_at           timestamptz NOT NULL,
    UNIQUE (tenant_id, id),                     -- target of oauth_tokens' connection reference
    UNIQUE (tenant_id, code_id),                -- one connect code, at most one connection
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, code_id) REFERENCES connect_codes (tenant_id, id)
);

-- at most one open connection per key (ADR-0031 d7).
CREATE UNIQUE INDEX oauth_connections_open ON oauth_connections (tenant_id, key_id)
    WHERE state = 'open';

CREATE TABLE oauth_tokens (
    hash          bytea       PRIMARY KEY,      -- SHA-256 of the token string
    tenant_id     uuid        NOT NULL,
    connection_id uuid        NOT NULL,
    kind          text        NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at    timestamptz NOT NULL,         -- set at minting; rotation never changes it
    rotated_at    timestamptz,                  -- refresh only: presenting it after this is reuse
    created_at    timestamptz NOT NULL,
    FOREIGN KEY (tenant_id, connection_id) REFERENCES oauth_connections (tenant_id, id)
);
```

`connect_codes`, `oauth_connections` and `oauth_tokens` carry their key's `tenant_id` and the
tenant's RLS. Each of their uniqueness rules on an id, `connect_codes_live`,
`oauth_connections_open` and `(tenant_id, code_id)`, leads with `tenant_id`: a unique check reads
past row security and runs before the foreign-key check, so a rule on the bare id would refuse
another tenant's insert naming a key with a live code or an open connection, or a redeemed code,
as `23505`, and show that tenant's state to anyone holding the uuid. Led by `tenant_id`, such an
insert is refused by the tenant-scoped key, `23503`, exactly as one naming no row is. The
semantics are unchanged, since a key and a code each belong to one tenant through those keys.
`oauth_clients` and `oauth_authorizations` hold state from before any code names a key, so they
belong to the reserved provider tenant, under its RLS, and hold no tenant content: a client's
self-description and a page's request in progress. The code, authorization-code and token
digests are found by the same pre-tenant authentication lookup that finds `api_keys.hash`; the
matched row's `tenant_id` then sets `oml.tenant_id` for everything after. At `/mcp` the lookup
matches only a row of kind `access`, and at a refresh only a row of kind `refresh`
([roles and keys](../architecture/roles-and-keys.md#connections)). The token endpoint never reads
`oauth_clients`: the client is the connection's `client_id`, so evicting a cached `cimd` row, or
its document lapsing, never affects a connection. A confirmation, Connect or Cancel, deletes
its `oauth_authorizations` row by `handle_hash` under the provider tenant (`DELETE … RETURNING`),
then sets `oml.tenant_id` from the returned `code_tenant_id` before it reads `connect_codes` and
writes `oauth_connections`, in one transaction. Connect, before the switch, also stamps
`oauth_clients.connected_at` for the returned `client_id` where it is null, and the stamp commits
only with the connection row. A later confirmation with that handle finds no row and answers
`oauth.authorization_expired`.

**The client caps are counted under a lock.** Caching a new document and inserting a registration
each take a transaction-scoped advisory lock of their own before they count, so two racing inserts
cannot both pass a cap at its last free row. A new document in a cache holding 10 000 `cimd` rows
first deletes the least recently used one whose `connected_at` is null (`last_used_at` nulls
first), selected `FOR UPDATE` so a Connect that stamped it meanwhile keeps it; finding none, it
writes nothing and the document serves its request uncached. A refetch updates its row in place,
keeping `connected_at` and `created_at`. A registration counts the `dcr` rows whose `connected_at`
is null and whose `created_at` is within 24 hours, and is refused at 10 000 as `limit.rate`; the
removal of unused registrations deletes those 24 hours old or more (ADR-0031 d5).

**Redemption and rotation are conditional updates.** Two racing requests with one code or one
refresh token cannot both pass, because each mints only when its own update matched a row. The
token exchange, in the transaction that commits `connection.opened` under the per-tenant chain
lock, first reads the row by `auth_code_hash` and checks the client, the verifier, the
`redirect_uri`, the code's voiding, the key and the resource, and a failed check consumes nothing;
then:

```sql
UPDATE oauth_connections
   SET auth_code_redeemed_at = now(), state = 'open', opened_seq = $seq
 WHERE auth_code_hash = $h AND state = 'authorized'
   AND auth_code_redeemed_at IS NULL AND auth_code_expires_at > now()
RETURNING id;
```

A presented code whose row has `auth_code_redeemed_at` set, or whose update returns no row because
a racing exchange redeemed it first, is judged by the reuse rule of
[roles and keys](../architecture/roles-and-keys.md#connections); when it is reuse, the token
endpoint revokes the connection (`connection.revoked`, cause `code_reuse`). A refresh runs in one
transaction that
does not take the chain lock:

```sql
SELECT connection_id FROM oauth_tokens WHERE hash = $h AND kind = 'refresh';   -- $c
SELECT id FROM oauth_connections WHERE id = $c FOR UPDATE;
UPDATE oauth_tokens SET rotated_at = now()
 WHERE hash = $h AND kind = 'refresh' AND rotated_at IS NULL AND expires_at > now()
RETURNING connection_id;
-- one row: insert the new access and refresh rows for $c, then commit
```

When the update returns no row while the row with that hash has `rotated_at` set, the token was
rotated already: that is reuse, and the revocation (`connection.revoked`, cause `refresh_reuse`)
commits in a transaction that takes the per-tenant chain lock. When it returns no row because the
token expired, the answer is `invalid_grant` and nothing is revoked. Before the update, under the
row lock, the refresh checks that the connection is open, that the request's `client_id` is the
connection's, and the key's state and the connection's resource; a failed check answers
`invalid_grant` and rotates nothing. Removal is
operational, run by the lead at its start and every hour after, journalling nothing (ADR-0031
d5, d9, d10, d12): an authorization request's row at Connect or Cancel, in that step's commit, or 24
hours after it lapses unused; a registered client 24 hours
after its registration while its `connected_at` is null; an `authorized` connection row once its
`auth_code_expires_at` has passed unexchanged; a token row only at its own `expires_at` or at its
connection's revocation, so a rotated refresh token keeps its row, and reuse of it inside that
window is always recognized; and `auth_code_hash`, cleared with the connection's token rows once
the connection is revoked, so a second redemption is recognized for as long as the connection
stands.

## web (ADR-0045)

*Since v0.5.0 (migration `0015_web`) ·
[ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d2, d4, d5*

```sql
CREATE TABLE web_emails (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    email_hash     bytea       NOT NULL,       -- SHA-256 over a purpose string and the email
    email          text        NOT NULL,       -- ASCII-lower-cased, at most 254 bytes
    user_name      oml_name    NOT NULL,       -- at most 56 characters
    admin          boolean     NOT NULL,
    google_subject text,                       -- pinned at the first Google sign-in
    github_subject text,                       -- pinned at the first GitHub sign-in
    created_at     timestamptz NOT NULL,
    added_by       uuid        NOT NULL,       -- the admin key that added it; issues the session keys
    PRIMARY KEY (tenant_id, email_hash),
    CONSTRAINT web_emails_email_hash UNIQUE (email_hash)   -- one entry per email, deployment-wide
);

CREATE TABLE web_sessions (
    token_hash bytea       PRIMARY KEY,        -- SHA-256 of the session token, never the token
    tenant_id  uuid        NOT NULL,
    email_hash bytea       NOT NULL,
    user_name  oml_name    NOT NULL,
    admin      boolean     NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    FOREIGN KEY (tenant_id, email_hash) REFERENCES web_emails (tenant_id, email_hash)
);
```

`web_emails` is folded from `web.email_allowed`, `web.email_changed` and `web.email_removed`; the
journal carries `email_digest`, which is `email_hash`, never the address, so a re-fold restores
every entry's digest, user and admin flag but not its address, which the admin adds again. A
change or a removal ends the entry's sessions in the same commit. The provider subjects are an
operational write at each provider's first sign-in, journalled nothing. A sign-in finds its entry
by `email_hash`, and a request finds its session by `token_hash`, before any tenant is known,
under `key_resolution` policies of 0001's form; every other statement runs under the tenant's
`tenant_isolation`. `web_sessions` is operational state: a row is written at an admitted callback,
deleted at sign-out, at the read that finds it expired, by the hourly sweep once expired, and with
its entry. The serving role holds `SELECT, INSERT, UPDATE, DELETE` on `web_emails` and
`SELECT, INSERT, DELETE` on `web_sessions`.

The same migration lets `api_keys.credential` be `session` and adds `api_keys.session_for`
(`oml_name`), the allowlist user a session key serves, set exactly on a session key
(`api_keys_session_user`); a session key, like a connector key, has no hash.

## vocabularies (ADR-0021)

Vocabulary state is claims plus two folds of the journal and one per-release materialization:

```sql
-- the prefix registry: tenant-scoped, RLS like every table; a prefix is permanent —
-- registered in the vocabulary workarea's create act, never reused or reassigned.
CREATE TABLE vocab_prefixes (
    tenant_id           uuid NOT NULL,
    prefix              text NOT NULL,
    workarea_id         uuid NOT NULL REFERENCES workareas (id),
    registered_event_id uuid NOT NULL REFERENCES journal (event_id),
    PRIMARY KEY (tenant_id, prefix)        -- tenant-scoped uniqueness: a global unique
);                                         -- would leak existence across tenants

-- per-release materialized is_a closure (built at vocab_release ratification; the
-- serialize-don't-compute contract holds because traversal reads this, never walks):
CREATE TABLE vocab_closure (
    tenant_id   uuid NOT NULL,
    release_id  uuid NOT NULL,              -- the vocab_release claim's artifact_id
    ancestor    text NOT NULL,              -- CURIE
    descendant  text NOT NULL,              -- CURIE
    depth       integer NOT NULL,
    PRIMARY KEY (tenant_id, release_id, ancestor, descendant)
);  -- O(terms × depth) rows per release; cost priced in FR-20
```

Terms, mappings and releases themselves are `memories` rows (kinds `term`, `term_mapping`,
`vocab_release`); deprecation successors are ratified `claim_edges` labelled `oml:replaced_by` /
`oml:consider`, declared in the deprecation submission and ratified with it. A term lineage is
never fully erased: the definition body is shreddable, the lineage row and `claim_key` persist —
a CURIE, once a term is promoted, resolves forever (the minted-at-first-promotion rule,
ADR-0021).

## simulations

The fence's mechanical existence (D10, declared in
[simulations](../architecture/simulations.md)).

```sql
CREATE TABLE simulations (
    id        uuid        PRIMARY KEY,
    tenant_id uuid        NOT NULL,
    state     text        NOT NULL CHECK (state IN ('open', 'ended')),
    audience  jsonb       NOT NULL,   -- journalled list of key ids / grant refs
    opened_at timestamptz NOT NULL,
    ended_at  timestamptz
);
```

Every read path applies the predicate `provenance <> 'simulation' OR key ∈ audience`; the
`memories.simulation_id` and `sim_derived_ids` columns above carry the taint this predicate and
end-of-simulation demotion act on.

## outbox, dead_letter, idempotency

The delivery machinery between the commit point and the lake, plus request replay protection.

```sql
CREATE TABLE outbox (
    seq              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,  -- single drain stream;
                                          -- assigned at insert, visible only at commit
    tenant_id        uuid   NOT NULL,
    kind             text   NOT NULL
        CHECK (kind IN ('bronze-event', 'materialize', 'retract', 'export',
                        'import-materialize')),  -- reserved: ADR-0020, post-v1
    journal_event_id uuid   NOT NULL REFERENCES journal (event_id),
    payload          jsonb  NOT NULL,   -- digest / ciphertext references only
    created_at       timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE lead_lease (        -- cluster-mode lead election (ADR-0023 d14); NEVER load-bearing —
    singleton        boolean PRIMARY KEY DEFAULT true CHECK (singleton),  -- one row
    holder_node_id   text        NOT NULL,
    address          text        NOT NULL,   -- followers discover the lead here
    oauth_issuer     text,                   -- the lead's oauth.issuer, NULL when unset; written when
                                             -- it takes the lease; followers serve the OAuth surface
                                             -- from it and have none of their own (ADR-0031 d14)
    lease_epoch      bigint      NOT NULL,
    expires_at       timestamptz NOT NULL    -- renewed on heartbeat; takeover journals
);                                           -- system.lead_acquired; the chain lock is the fence

CREATE TABLE lake_frontier (     -- CACHE of the per-table Delta txn version; never authority
    table_name       text   PRIMARY KEY,
    last_applied_seq bigint NOT NULL,
    updated_at       timestamptz NOT NULL
);

CREATE TABLE dead_letter (
    outbox_seq       bigint      PRIMARY KEY,
    tenant_id        uuid        NOT NULL,
    kind             text        NOT NULL,
    payload          jsonb       NOT NULL,
    attempts         integer     NOT NULL,
    last_error       text        NOT NULL,
    skipped_event_id uuid        NOT NULL,  -- the journalled materialize-skipped event
    dead_lettered_at timestamptz NOT NULL
);

CREATE TABLE idempotency (
    key_id         uuid        NOT NULL,
    workarea_id    uuid        NOT NULL,  -- the resolved node id; the nil UUID
                                          -- (00000000-0000-0000-0000-000000000000) for endpoints
                                          -- that name no workarea; for a /v1 workarea name
                                          -- that names no row, its version-8 unresolved value
                                          -- (http-api.md, the unresolved value), never an id
    idem_key       text        NOT NULL,
    request_digest bytea       NOT NULL,
    state          text        NOT NULL CHECK (state IN ('in_progress', 'committed')),
    response       jsonb,                 -- a connect-code response is stored without
                                          -- the code (http-api Idempotency)
    created_at     timestamptz NOT NULL,
    PRIMARY KEY (key_id, workarea_id, idem_key)
);
```

Every outbox insert happens inside the same serialized transaction as its journal event, so outbox
`seq` order is consistent with each tenant's chain order. Across tenants nothing is serialized, and
`seq` is assigned at insert while visibility arrives only at commit — so a smaller `seq` can become
visible after a larger one has been read, and the identity column has permanent gaps (aborted
transactions) as well as transient ones (still-in-flight transactions). The lake-writer therefore
drains only the **stable prefix**: rows whose inserting transaction is older than every transaction
still active at scan time (below the oldest active txid of the scan's snapshot), as declared in
[storage](../architecture/storage.md) — a late-committing row is cut into a later batch, never
skipped. The Delta transaction version — not `lake_frontier` — is the recovery authority, which is
why the frontier table is labelled a cache (D2, declared in
[storage](../architecture/storage.md)). An idempotency record is claimed in
its own transaction before any side effect: same `idem_key` with a different `request_digest` is a
typed error; replay with the same digest returns the stored `response`.

## read_events

The read log lives outside the hash chain, partitioned by time, retention-bounded, with per-batch
digests anchored into the journal (D12, declared in
[retrieval](../architecture/retrieval.md)).

```sql
CREATE TABLE read_events (
    tenant_id    uuid        NOT NULL,
    key_id       uuid        NOT NULL,
    role         text        NOT NULL,
    workarea_id  uuid,
    memory_ids   uuid[],          -- what was served, OR:
    query_digest bytea,           -- digest of the query for search reads
    at           timestamptz NOT NULL
) PARTITION BY RANGE (at);

-- e.g. monthly partitions; the retention act is DROP PARTITION, which is why
-- the table is partitioned at all. Partition granularity is an implementation choice.
CREATE TABLE read_events_2026_11 PARTITION OF read_events
    FOR VALUES FROM ('2026-11-01') TO ('2026-12-01');
CREATE INDEX ON read_events_2026_11 (tenant_id, key_id, at);
```

The retention window is tenant policy; evidence exports scope their read log to that window.
Dropping a partition removes read events wholesale — the digest anchors in the journal remain, so
an auditor can see that a batch existed and was intact even after its rows are gone, but not what
it contained. That trade (bounded storage against bounded reconstructability) is deliberate and
stated in [audit-and-lineage](../architecture/audit-and-lineage.md).

## artifact_keys

Per-artifact DEKs, wrapped by the KEK. Erasure is DEK destruction (D4, declared in
[audit-and-lineage](../architecture/audit-and-lineage.md)).

```sql
CREATE TABLE artifact_keys (
    tenant_id    uuid  NOT NULL,
    artifact_id  uuid  NOT NULL,
    wrapped_dek  bytea,            -- NULL after destruction
    kek_id       text  NOT NULL,   -- which KEK wrapped it (rotation handle)
    destroyed_at timestamptz,      -- set by the journalled erase event's fold
    PRIMARY KEY (tenant_id, artifact_id),
    CHECK (wrapped_dek IS NOT NULL OR destroyed_at IS NOT NULL)
);
```

The erase transaction journals the erase event, sets `wrapped_dek` to NULL and `destroyed_at`, and
removes plaintext-derived index rows (FTS, embeddings) in the same commit. Ciphertext copies
elsewhere (lake parquet versions, object-store versions, Postgres backups) become unreadable, not
absent; the full copy-class inventory and its honest boundary live in
[audit-and-lineage](../architecture/audit-and-lineage.md).

## Full-text search and embeddings

The serving tier's two indexes over promoted rows, and the author-fenced index of held text. All
three are plaintext-derived, so all three are in the erasure inventory and are removed at erase
time. Which indexed rows recall serves is declared in
[retrieval](../architecture/retrieval.md#the-serving-tier).

```sql
CREATE TABLE memories_fts (
    tenant_id   uuid    NOT NULL,
    artifact_id uuid    NOT NULL,
    version     integer NOT NULL,
    tsv         tsvector NOT NULL,
    PRIMARY KEY (tenant_id, artifact_id, version)
);
CREATE INDEX memories_fts_gin ON memories_fts USING gin (tsv);

-- decided, not built (ADR-0025 d5): held text, fenced to its author.
-- A row exists only while its version is held: every transition into held
-- inserts it (capture, with tsv computed from the NFC plaintext before BEGIN;
-- reopen, and a reassignment that re-enters held, with tsv computed on the
-- serving path), every transition out of held removes it, and a reassignment
-- updates workarea_id in its own commit (retrieval, Held text containment).
CREATE TABLE held_fts (
    tenant_id     uuid     NOT NULL,
    memory_id     uuid     PRIMARY KEY,           -- the held version's artifact_id
    author_key_id uuid     NOT NULL REFERENCES api_keys (id),
    workarea_id   uuid     NOT NULL REFERENCES workareas (id),
    tsv           tsvector NOT NULL
);
CREATE INDEX held_fts_gin ON held_fts USING gin (tsv);

CREATE TABLE memory_embeddings (
    tenant_id     uuid    NOT NULL,
    artifact_id   uuid    NOT NULL,
    version       integer NOT NULL,
    workarea_path ltree   NOT NULL,   -- filter column for subtree-scoped queries
    embedding     vector  NOT NULL,   -- dimension bound to the embedding model: open RS
    embedder      text    NOT NULL,   -- plugin identity+version that produced the row (ADR-0019)
    PRIMARY KEY (tenant_id, artifact_id, version)
) PARTITION BY LIST (tenant_id);

-- per tenant partition, created with the tenant:
CREATE TABLE memory_embeddings_t_<tenant> PARTITION OF memory_embeddings
    FOR VALUES IN ('<tenant_id>');
CREATE INDEX ON memory_embeddings_t_<tenant>
    USING hnsw (embedding vector_cosine_ops);
```

List-partitioning by tenant gives each tenant its own HNSW index (bounded build cost and blast
radius) at the cost of a partition-per-tenant management obligation; RLS still applies on top.
Iterative index scans are enabled so a post-filtered query keeps scanning rather than returning
short. Recall quality, latency, and exact-vs-approximate behaviour are unmeasured — tracked as
roadmap FR rows with exit criteria, and no number is asserted until that measurement runs. The
embedding model (and therefore the `vector` dimension and distance operator) is an open RS in the
[roadmap](../product/roadmap.md). Simulation rows never enter these shared indexes; they are
indexed per simulation, which keeps the read-path predicate out of the hot index at the cost of
duplicate index machinery per open simulation.

### The built FTS index: per-tenant partitions

**Status:** built in `migrations/0009_memories_partitions.sql` and `oml-pg`'s
`create_tenant_partitions` · its checks run only where a superuser mints the suites' role pairs,
CI's `cargo test (Postgres)`, and in CI's `rls-spike`
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d8, d9, d11, d13).
The tables above are designed, not built; the PoC's FTS index is `memories_claim_fts`, an
expression index over `memories.claim`.

`memories` is list-partitioned by `tenant_id`, one partition per tenant, so every index on it is
a partitioned index and each tenant's partition holds its own: no FTS index, and no index's
statistics, span tenants (ADR-0007 d4).

```sql
CREATE TABLE memories (
    …,                                    -- the columns, defaults and checks of 0001–0005
    PRIMARY KEY (tenant_id, id),          -- a unique key must include the partition key
    FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    FOREIGN KEY (tenant_id, created_event_id) REFERENCES journal (tenant_id, event_id)
) PARTITION BY LIST (tenant_id);          -- no DEFAULT partition
CREATE INDEX memories_claim_fts ON memories USING gin (to_tsvector('simple', claim));
-- memories_workarea, memories_claim_version, memories_served_tip, memories_author as before;
-- RLS, FORCE and tenant_isolation as on every table.

-- per tenant, before the commit whose fold creates its row, one owner transaction:
SET LOCAL lock_timeout = '5s';
CREATE TABLE memories_t_<tenant id, 32 hex> (LIKE memories INCLUDING ALL);
ALTER TABLE memories_t_<…> ENABLE ROW LEVEL SECURITY;
ALTER TABLE memories_t_<…> FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON memories_t_<…> USING (…) WITH CHECK (…);  -- the parent's
ALTER TABLE memories ATTACH PARTITION memories_t_<…> FOR VALUES IN ('<tenant id>');
```

- **The name** is the tenant's id, never its name, which would put every tenant's name in
  `pg_class` for every role.
- **Who creates it.** The owner, on a connection opened for that call alone and closed after it,
  while tenant creation holds no serving transaction open; 0009 creates each existing tenant's
  and fills it under that tenant's context. `ATTACH PARTITION` takes `ShareUpdateExclusiveLock`
  on `memories`, where `CREATE TABLE … PARTITION OF` would take `AccessExclusiveLock` and stop
  every tenant's reads and writes. The attach clones `memories`' foreign keys, so it also takes
  `ShareRowExclusiveLock` on `journal`, `tenants` and `review_queue` (probed on 18.6,
  2026-11-03): every tenant's commits, each of which inserts into `journal`, wait for it, bounded
  by the timeout, and a commit that inserts into `tenants` then `journal`, as a genesis does, can
  deadlock with it.
- **Failure.** A DDL that fails fails tenant creation before any commit: nothing is journalled
  and the bootstrap claim spends no enrollment token. A lock contest the DDL lost, a lock timeout
  (`55P03`) or a deadlock the detector broke by aborting it (`40P01`), answers
  `system.unavailable`, retryable, `details.reason` `partition_lock_timeout`; any other failure
  answers `system.internal`. A serving commit the detector aborts instead answers
  `system.unavailable` as any backend failure does, retryable. A partition whose DDL committed is kept when the genesis commit then
  fails: it is empty, the owner's and fenced, so the checks after migrating accept it, and the
  retry mints a fresh id.
- **Reads.** Every adapter statement on `memories` binds `tenant_id = $n` to the tenant it sets
  in `oml.tenant_id`, so the plan keeps that tenant's partition alone, at planning or as it
  starts; row security stays the fence. No function is marked `LEAKPROOF`, so `@@` cannot become
  an index condition under the policy and an FTS read scans its tenant's partition (ADR-0034
  d13; FR-25 measures its latency per tenant size).
- **Privileges.** A partition is reached only through `memories`, whose grants are 0007's
  ([Privileges](#privileges)); a partition grants the serving role nothing, so it sees no
  partition's `pg_stats`.
- **Stays open** (FR-45): each partition's `reltuples`, bound and name give every role one
  tenant's row count and id.

### search (ADR-0049)

**Status:** introduced in v0.5.5, built in `migrations/0020_search.sql`
([ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) d12).

```sql
-- one normaliser for the query, the trigram retriever and the vocabulary: the text, then its identifier parts
CREATE FUNCTION oml_search_split(t text) RETURNS text ...;   -- CamelCase and . _ / : # @ - split
CREATE FUNCTION oml_search_text(t text) RETURNS text RETURN t || ' ' || oml_search_split(t);
-- the bm25 lexemes, each occurrence once: simple, then the parts the text lacks, then the english
-- stems neither holds, stemmed from the text and from the parts it lacks
CREATE FUNCTION oml_search_exact(t text) RETURNS tsvector
    RETURN to_tsvector('simple', t)
        || ts_delete(to_tsvector('simple', oml_search_split(t)), tsvector_to_array(to_tsvector('simple', t)));
CREATE FUNCTION oml_search_stems(t text) RETURNS tsvector
    RETURN to_tsvector('english', t)
        || ts_delete(to_tsvector('english', oml_search_split(t)), tsvector_to_array(to_tsvector('english', t)));
CREATE FUNCTION oml_search_vector(t text) RETURNS tsvector
    RETURN oml_search_exact(t) || ts_delete(oml_search_stems(t), tsvector_to_array(oml_search_exact(t)));
ALTER TABLE memories ADD COLUMN search_vector tsvector NOT NULL, ADD COLUMN search_len integer NOT NULL;
-- set once per write; an update naming the claim or either column recomputes them
CREATE TRIGGER memories_search_vector BEFORE INSERT OR UPDATE OF claim, search_vector, search_len
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vector_fold();
-- did_you_mean's words: per workarea, the simple words of its promoted served tips of kind memory or derived_fact
CREATE TABLE search_vocabulary (
    tenant_id   uuid    NOT NULL REFERENCES tenants (id),
    workarea_id uuid    NOT NULL,
    word        text    NOT NULL,
    rows        integer NOT NULL CHECK (rows > 0),   -- served tips that say it
    PRIMARY KEY (tenant_id, workarea_id, word)
);   -- ENABLE and FORCE ROW LEVEL SECURITY, tenant_isolation; SELECT, INSERT, UPDATE, DELETE to the serving role
CREATE TRIGGER memories_search_vocabulary AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim,
    workarea_id, kind OR DELETE ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vocabulary_fold();
CREATE EXTENSION IF NOT EXISTS pg_trgm;   -- where the owner may; for bm25-trigram and did_you_mean only
ALTER TABLE tenants ADD COLUMN search_default text;   -- folded from search.default_set
```

`search_vector` and `search_len` are set by `oml_search_vector_fold`, a `BEFORE` trigger, so the
vector is computed once per write and no read rebuilds it; a write naming either column gets its
claim's lexemes regardless. 0020 fills both for every existing row, each tenant under its own row
security, then sets them `NOT NULL`; a partition created later takes the columns through `LIKE …
INCLUDING ALL` and both triggers through `ATTACH PARTITION`. Both trigger functions are `SECURITY
INVOKER` with `search_path` pinned to `public, pg_temp`, so the role split's definer check holds.
`memories_claim_fts` stays: the `keyword-all` and `keyword-any` pipelines read it. The `bm25`
statement reads `search_vector` and `search_len` and no row's claim; it computes the row count,
the average length and each form's document frequency per query over the rows the candidate scope
admits, never from a stored statistic, so no table holds term counts across workareas a key may
not read.

No text or trigram index is built on these columns or on the vocabulary: `@@`, `%` and `<%` are
not leakproof, so under row security the planner would use none for the serving role's reads,
which scan the scope (ADR-0034 d13), and each would cost every write. The measurements behind
this are in ADR-0049 d12.

`search_vocabulary` is kept by `oml_search_vocabulary_fold` inside the write's own transaction:
a version of kind `memory` or `derived_fact` adds one to each of its words when it becomes a
promoted served tip that is no retirement version, and takes one away when it stops being one
(replaced, retired, demoted); a word leaves at zero. Words are taken in byte order, and every fold
runs under the tenant's commit lock, so two writes never race on a count. It is derived and never
journalled; 0020 fills it for the rows already promoted, each tenant under its own row security.
A held row's words, another workarea's, another kind's and a replaced version's are never in a
workarea's vocabulary, so `did_you_mean`, which reads only the readable nodes' vocabulary, never
names a word only an unreadable row says or recall cannot find.

The stored lexemes depend on the `english` text-search configuration and the database's
character classes; changing either needs them recomputed, which an update of each row's claim to
itself does through the trigger. Erasure, when it is built, must rewrite the claim, so that the
trigger rewrites the lexemes; erasing by status alone would leave the text in `search_vector`.

`pg_trgm` is a trusted extension; where the owner role cannot create it or the contrib package is
absent, 0020 goes on without it, `did_you_mean` answers `[]`, the default pipelines (`keyword-any`,
`bm25`) are served, and a configuration naming `bm25-trigram` stops the start naming the
extension. `tenants.search_default` is the tenant's default pipeline by name, `NULL` for the
deployment's.

### search postings (ADR-0052)

*Introduced in v0.5.6, built in `migrations/0021_search_postings.sql`
([ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)).*

Under row security no text operator reaches an index (`@@`, `%`, `ts_rank` are not leakproof), but
integer and `uuid` equality are, so 0021 keeps each version's words a second time as integer
postings whose `token_id = ANY(…)` stays an index condition for the serving role. The
`bm25-postings` pipeline reads them; `bm25` and the stored lexemes stay, as its fallback and its
comparison.

```sql
-- a lexeme's id: the first 8 bytes of SHA-256 over its UTF-8 bytes, a signed big-endian bigint;
-- computed, never looked up: no table maps a word to an id (oml-pg's token_id is held equal)
CREATE FUNCTION oml_token_id(t text) RETURNS bigint IMMUTABLE ...;
CREATE TABLE search_postings (
    tenant_id   uuid     NOT NULL,   -- the partition key and the policy's column
    memory_id   uuid     NOT NULL,   -- the version the posting derives from
    workarea_id uuid     NOT NULL,   -- copied at write
    token_id    bigint   NOT NULL,   -- kind 1: oml_token_id(lexeme)
    weight      real     NOT NULL,   -- kind 1: the lexeme's term frequency
    doc_len     integer  NOT NULL,   -- kind 1: the version's search_len
    kind        smallint NOT NULL,   -- 1 lexeme term frequency; 2 learned sparse weight (0022)
    memory_kind smallint NOT NULL,   -- 1 memory, 2 derived_fact
    state       smallint NOT NULL    -- 0 held, 1 served (promoted tip, no retirement), 2 off
) PARTITION BY LIST (tenant_id);     -- search_postings_t_<tenant id, 32 hex>
-- the served postings, by token then workarea, carrying what a score needs: an index-only read
CREATE INDEX search_postings_served ON search_postings (kind, token_id, workarea_id)
    INCLUDE (memory_kind, memory_id, weight, doc_len, tenant_id) WHERE state = 1;
CREATE INDEX search_postings_memory ON search_postings (memory_id);
-- the served versions' counts per workarea and memory kind, summed over a recall's readable nodes
CREATE TABLE search_term_rows (tenant_id, workarea_id, kind, memory_kind, token_id, rows,
    PRIMARY KEY (tenant_id, workarea_id, kind, memory_kind, token_id));
CREATE TABLE search_workarea_rows (tenant_id, workarea_id, kind, memory_kind, rows, total_len,
    PRIMARY KEY (tenant_id, workarea_id, kind, memory_kind));
-- each ENABLE and FORCE ROW LEVEL SECURITY, tenant_isolation; SELECT, INSERT, UPDATE, DELETE to the
-- serving role; the two count tables SET STATISTICS 0 on every column, and the postings parent
-- (ONLY) on token_id, memory_id and workarea_id, each partition keeping its own (FR-45)
CREATE TRIGGER memories_search_postings AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim,
    search_vector, workarea_id, kind OR DELETE ON memories
    FOR EACH ROW EXECUTE FUNCTION oml_search_postings_fold();
-- the fill, per tenant; the owner's repair when doctor names a fault
CREATE FUNCTION oml_search_postings_refill(t uuid) RETURNS void SECURITY INVOKER ...;
```

Only versions of kind `memory` and `derived_fact` that are no retirement version get postings, one
per lexeme of the stored `search_vector`, written with the version in its own transaction; no text
is parsed again. Promotion sets their `state` to 1 and adds the version to the counts; supersession,
demotion, rejection and retirement set it to 2 and take the version back out; a count reaching zero
deletes its row. A recall reads a token's served postings on its readable workareas from
`search_postings_served` alone, an index-only scan once vacuum has marked the pages visible, so a
leaf scope reads its own workareas' postings and not the tenant's; a flip in or out of served adds
or removes that entry, so it is no heap-only update. Only an erase
(`status = 'erased'`) or a row's deletion deletes the postings, and with them the counts' share:
both are plaintext-derived, so ADR-0005 d4's erasure inventory lists them beside the stored
lexemes. A version whose lexemes, workarea or kind change gives its postings back and takes new
ones. The trigger function is `SECURITY INVOKER` with `search_path` pinned.

The owner creates each tenant's postings partition in the same transaction as its memories
partition, first, so the tenant's first write finds it; a lead built before 0021 creates the
memories partition alone, which leaves that tenant's writes failing, so every start, after
migrating, creates and fills the postings partition of any tenant that lacks one, and doctor names
the gap until then; 0021 creates one for every existing
tenant, then creates the trigger, then fills every existing version's postings and the served
versions' counts from the stored lexemes under each tenant's row security, with
`oml_search_postings_refill(tenant)` for each. Writes wait for the fill, since the trigger's
lock on `memories` is held to its end; a batched, resumable fill is FR-103.

The same function is the repair doctor names when a tenant's postings are incomplete or their
counts drift (ADR-0052 d12; [observability](../operations/observability.md#the-doctor-contract)).
Only the owner may run it (`EXECUTE` is revoked from `PUBLIC`; `oml_search_count`, which the
trigger calls as the writing role, is granted to the serving role alone). It takes that tenant's
chain advisory lock, the one every memory write of the tenant takes first, so the tenant's writes
wait for it and no other tenant's do. The owner runs it in a transaction whose `oml.tenant_id` is
that tenant, which it checks:

```sql
BEGIN;
SELECT set_config('oml.tenant_id', '<tenant id>', true);
SELECT oml_search_postings_refill('<tenant id>');   -- holds this tenant's writes to the commit
COMMIT;
```

A recall sums the counts over its readable nodes and memory kinds, adds the caller's own held
rows (read by the `memories_author` index) when it asked for them, and so knows each word's
readable document frequency before it reads a posting. A tenant-wide count is never kept: it
would rank by rows the key cannot read. Frequent words are pruned from the postings read and
rescored for the candidates the others reach; when every word is pruned the `bm25` scan answers
(`search.access: "scan"`); [retrieval](../architecture/retrieval.md) has the rule and its
constants.

### learned sparse postings and their queue (ADR-0052 d14, d15)

*Introduced in v0.5.7, built in `migrations/0022_search_sparse_queue.sql`.*

Kind 2 rows of `search_postings` hold the learned sparse encoder's weights: `token_id` is the
model's vocabulary index (0 to 30,521, the same for every tenant, so no tenant's word is in it),
`weight` the model's weight of that token for the version (at least 0.1, at most 128 per version),
and `doc_len` 1 when the version's own tokens hold the token, 0 when the model added it (its
expansion, which a hit's `matched.expanded` names). Kind 2 keeps no counts: the query side's weights
ship with the model, so no corpus statistic is read. The encoder writes them after the version's
own write commits, through a queue the same trigger fills:

```sql
CREATE TABLE search_sparse_queue (
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    memory_id   uuid        NOT NULL,
    enqueued_at timestamptz NOT NULL DEFAULT clock_timestamp(),  -- its turn, and its write's guard
    attempts    integer     NOT NULL DEFAULT 0,                  -- failed encodes
    next_at     timestamptz NOT NULL DEFAULT clock_timestamp(),  -- the next try, 2^attempts s, at most 1 h
    last_error  text,                                            -- the last failure, which doctor names
    parked_at   timestamptz,                                     -- a dead letter after 5 failures
    PRIMARY KEY (tenant_id, memory_id)
);
CREATE INDEX search_sparse_queue_order ON search_sparse_queue (tenant_id, enqueued_at, memory_id)
    WHERE parked_at IS NULL;                                     -- oldest first
CREATE TABLE search_sparse_state (
    tenant_id      uuid PRIMARY KEY REFERENCES tenants (id),
    enabled        boolean NOT NULL,       -- the trigger and the refill enqueue only while on
    backfill_after uuid,                   -- the last version id the backfill read
    backfilled_at  timestamptz             -- when the backfill finished
);
-- both: tenant_isolation, and a SELECT-only lead_sparse_enumeration policy under
-- oml.startup_check, so the encoder finds every tenant with work in one read
-- ENABLE and FORCE ROW LEVEL SECURITY, tenant_isolation; SELECT, INSERT, UPDATE, DELETE to the
-- serving role; SET STATISTICS 0 (FR-45)
CREATE OR REPLACE FUNCTION oml_search_postings_fold() ...;     -- 0021's, enqueueing too
CREATE OR REPLACE FUNCTION oml_search_postings_refill(t uuid) ...;  -- kind 1 rebuilt, kind 2 kept
```

`oml_search_postings_fold` enqueues a version in the transaction that writes its kind 1 postings,
enqueues it again with a new `enqueued_at` when its words change, and takes it off the queue when
its postings leave (erasure, deletion, no longer a hit kind). A state flip moves every kind's
postings. The encoder deletes an entry only when its `enqueued_at` is the one it read, reads the
version's state `FOR SHARE`, and writes the kind 2 rows with that state, all in one transaction.
The refill rebuilds kind 1 from the stored lexemes, brings kind 2's states to their versions',
drops kind 2 rows of versions that have no postings any more, and queues any version with none.
0022 queues nothing: a lead serving no sparse pipeline turns every tenant's state off and
empties the queue at start, and one serving a sparse pipeline turns each tenant's on in its
encoder's first pass and backfills the versions written before, 1,000 per pass, resuming after
the last id read; the step that turns a state on takes `SHARE` on `memories` first, so no write in
flight is missed by both the trigger and the backfill. The drain holds the version `FOR SHARE` before it takes the entry, as the
trigger does, takes entries `FOR UPDATE SKIP LOCKED` under a 5-minute lease, and parks an entry
after 5 failed encodes.
### recall telemetry (ADR-0054)

*Since v0.5.7, migration 0023 · [ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) d2, d3, d4, d5.* Telemetry, not claims:
no memory's text, no version, no verdict and no journal event, nothing derived from it, and
every row gone 30 days after its recall.

```sql
ALTER TABLE tenants ADD COLUMN telemetry_query_text_since timestamptz(6);  -- from telemetry.settings_set

CREATE TABLE recall_events (           -- one row per recall
    tenant_id, recall_id, created_at,  -- PRIMARY KEY; created_at the week key
    key_id, principal, scope_workarea_id, source_workarea_id,
    pipeline text, access text,        -- the closed names
    query_digest bytea,                -- the request's digest, always
    query_text text,                   -- only for created_at >= the tenant's opt-in instant
    hit_ids uuid[], hit_scores real[]  -- served, in rank order, at most 20
) PARTITION BY LIST (tenant_id);
CREATE TABLE recall_outcomes (         -- what the reader did next
    tenant_id, recall_id, created_at,  -- the event's, so a label lives in its week
    artifact_id uuid,                  -- NULL for requery and no_use
    label text, source text,           -- inferred | explicit
    at timestamptz,
    UNIQUE NULLS NOT DISTINCT (tenant_id, recall_id, created_at, artifact_id, label)
) PARTITION BY LIST (tenant_id);
```

Each table is list-partitioned by tenant alone, `recall_events_t_<tenant id hex>` and
`recall_outcomes_t_<hex>`, each with `ENABLE` and `FORCE ROW LEVEL SECURITY` and
`tenant_isolation`, and no statistics on the columns holding a reader's words or names. 0023
creates every existing tenant's; the lead creates a new tenant's with its memories partition,
and at its start repairs a tenant a lead before 0023 created. No owner DDL runs on a schedule
(ADR-0034 d4). Both tables carry a `(tenant_id, created_at)` index, which the purge and the
`no_use` pass read. The serving role holds `SELECT`, `INSERT`, `UPDATE` and `DELETE` on both and
owns neither: it inserts, labels, nulls text on opt-out and erase, and every minute deletes rows
older than 30 days in batches of 5,000, each committed. Every read filters
`created_at > now() - 30 days`.

The opt-in is the database's rule. `recall_events_query_text_rule`, an owner-owned `BEFORE
INSERT OR UPDATE OF query_text, created_at` trigger running `oml_telemetry_query_text_rule()`
(`SECURITY INVOKER`), nulls `query_text` unless the tenant's `telemetry_query_text_since` is set
and not after the row's `created_at`. It reads the tenant's row `FOR SHARE`, so an opt-out
committing while a writer's batch is open waits for it and then nulls its text. The serving
role holds no `TRIGGER` privilege.

`oml_telemetry_erase(tenant, version_ids)`, `SECURITY INVOKER`, granted to the serving role, is
the rule erase's build calls: it nulls the text and zeroes the query digest of each event that
served an erased version, removes the version from `hit_ids` and `hit_scores`, and deletes its
labels.

## upload_sessions

Resumable attachment uploads (D3, declared in [storage](../architecture/storage.md)).

```sql
CREATE TABLE upload_sessions (
    id              uuid        PRIMARY KEY,
    tenant_id       uuid        NOT NULL,
    key_id          uuid        NOT NULL REFERENCES api_keys (id),
    expected_digest bytea       NOT NULL,   -- SHA-256 of plaintext, declared up front
    state           text        NOT NULL
        CHECK (state IN ('open', 'complete', 'aborted', 'expired')),
    parts           jsonb       NOT NULL DEFAULT '[]',  -- [{part_number, size_bytes, part_digest}]
    created_at      timestamptz NOT NULL,
    expires_at      timestamptz NOT NULL
);
```

A retry resumes the session or restarts it; a session completes only when the assembled bytes match
`expected_digest`, at which point the digest — not the session — is what the submission envelope
binds.

## Row-level security

Every table above carries tenant RLS; the posture (session roles, no BYPASSRLS, privileged-service
scoping) is declared in [security](../architecture/security.md). The sketch, repeated per table:

```sql
ALTER TABLE memories ENABLE ROW LEVEL SECURITY;
ALTER TABLE memories FORCE ROW LEVEL SECURITY;   -- binds the table owner too

CREATE POLICY tenant_isolation ON memories
    USING (tenant_id = current_setting('oml.tenant_id')::uuid)
    WITH CHECK (tenant_id = current_setting('oml.tenant_id')::uuid);
```

- `oml-server` sets `oml.tenant_id` from the API key at the start of every transaction — tenant is
  derived server-side, never from the request.
- `oml-lakewriter` iterates tenants one at a time under a SET-tenant session role; it never sees
  two tenants in one statement.
- `oml-reconciler` runs read-only under the same per-tenant discipline and emits repairs through
  the outbox via `oml-server`, so it needs no write policy at all.
- Provider-level rows (operator keys with `tenant_id IS NULL`) are reachable only by a separate
  policy on `api_keys` for the operator path; no tenant session can see them.
- `held_fts` (decided, not built) carries a second policy besides `tenant_isolation`, so a session
  reads only the held text its own key set wrote — containment by construction
  ([retrieval](../architecture/retrieval.md#held-text-containment)). The set is the caller's
  [own](../architecture/roles-and-keys.md#own) keys: `oml-server` sets `oml.own_key_ids` — the
  authenticated key and every key in its journalled `rotated_from` chain, comma-separated — at
  the start of every transaction, exactly as it sets `oml.tenant_id`, and never from the client:

    ```sql
    ALTER TABLE held_fts ENABLE ROW LEVEL SECURITY;
    ALTER TABLE held_fts FORCE ROW LEVEL SECURITY;
    CREATE POLICY author_only ON held_fts AS RESTRICTIVE
        USING (author_key_id = ANY (string_to_array(current_setting('oml.own_key_ids'), ',')::uuid[]))
        WITH CHECK (author_key_id = ANY (string_to_array(current_setting('oml.own_key_ids'), ',')::uuid[]));
    ```

    The capture commit runs under the author's own key, so its insert passes the policy. A commit
    that moves a version into or out of `held` under another key — a promotion, rejection,
    erasure, reopen or reassignment — sets `oml.own_key_ids`, transaction-local
    (`set_config(…, true)`), to the version's author key set inside round trip one's statement,
    beside `pg_advisory_xact_lock` and the chain-head read
    ([ADR-0024](../adr/ADR-0024-performance-foundation.md) d1), for the whole transaction; no
    restore is needed, since that transaction reads no other held text. Only `oml-server` sets
    that setting, never a client, so the fence binds every read path without blocking the
    transitions. Because the policy skips a row it does not match without an error, each of
    those commits checks that its `held_fts` statement affected exactly one row and aborts
    otherwise ([retrieval](../architecture/retrieval.md#held-text-containment)).

A `current_setting`-based policy is simple and index-friendly; its cost is that a service bug that
sets the wrong tenant GUC becomes a cross-tenant read. That risk and its mitigations (one
code path sets the GUC; the STRIDE model treats hostile co-tenants) are owned by
[security](../architecture/security.md).

## Privileges

**Status:** built in `migrations/0007_role_split.sql`, and the lead's owner URL and startup
refusals in `oml-server-lead` and `oml-pg` · their checks run only where a superuser mints the
suites' role pairs, CI's `cargo test (Postgres)`
([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d4, d5, d8).

Two roles reach a database: the **owner**, which owns the database and every object the
migrations create and runs every migration, and the **serving role**, which owns nothing and
holds only the DML the adapter's statements use (ADR-0034 d2, d3). Each table's migration grants
the serving role its row below, naming the grantee through the setting `oml.serving_role`, which
the lead sets on every owner connection to its serving URL's `current_user`; a granting migration
run without it fails at `current_setting` (`42704`). A table a later migration adds refuses the
serving role until that migration grants its row here, under the sync rule.

| Table | `SELECT` | `INSERT` | `UPDATE` | `DELETE` | Sequence |
|---|---|---|---|---|---|
| `tenants` | ✓ | ✓ | ✓ | | |
| `workareas` | ✓ | ✓ | ✓ | | |
| `api_keys` | ✓ | ✓ | ✓ | | |
| `journal` | ✓ | ✓ | | | |
| `journal_chain_head` | ✓ | ✓ | ✓ | | |
| `outbox` | | ✓ | | | `USAGE` on `outbox_seq_seq` |
| `memories` | ✓ | ✓ | ✓ | | |
| `review_queue` | ✓ | ✓ | ✓ | ✓ | |
| `idempotency` | ✓ | ✓ | | | |
| `grants` | ✓ | ✓ | | | |
| `key_denials` | ✓ | ✓ | ✓ | | |
| `crews` | ✓ | ✓ | ✓ | | |
| `connect_codes` | ✓ | ✓ | ✓ | | |
| `oauth_connections` | ✓ | ✓ | ✓ | ✓ | |
| `oauth_tokens` | ✓ | ✓ | ✓ | ✓ | |
| `oauth_clients` | ✓ | ✓ | ✓ | ✓ | |
| `oauth_authorizations` | ✓ | ✓ | | ✓ | |
| `workarea_transitions` | ✓ | ✓ | ✓ | | |
| `claim_edges` | ✓ | ✓ | ✓ | | |
| `brief_edges` | ✓ | ✓ | ✓ | | |
| `_sqlx_migrations` | | | | | |

- **`journal`** takes `SELECT` and `INSERT` alone, so a serving `UPDATE` or `DELETE` fails on
  privilege (`42501`) before the guard trigger runs (ADR-0002 d4).
- **`review_queue`** is deleted from at a verdict; **`oauth_connections`** and **`oauth_tokens`**
  only by the operational removal that [connections](#connections-adr-0031) states, which the lead
  runs hourly (0012);
  **`oauth_clients`** by the cache's eviction and that sweep's removal of an unused registration, and
  **`oauth_authorizations`** by the confirmation's `DELETE … RETURNING` and the removal 24 hours
  after a row lapses unused (0013). An `oauth_authorizations` row is never updated.
- **`outbox`** is written, never read, by the adapter; its `INSERT` draws `seq` from the
  `bigserial` sequence.
- **`workarea_transitions`** is granted by its own migration, 0017, in 0007's form: the
  proposal's `INSERT`, the ratification's `UPDATE` and the status read's `SELECT`.
- **No table** grants `TRUNCATE`, `REFERENCES`, `TRIGGER` or `MAINTAIN`, whole-table operations
  row security does not bind.
- **`_sqlx_migrations`** is the one table without row-level security: sqlx's record of which
  migrations ran holds no tenant data and no `tenant_id` to key a policy on. Its fence is
  privilege: 0007 revokes every privilege on it from `PUBLIC` and the serving role holds none
  (ADR-0034 d6).
- **`public`**: 0007 revokes `CREATE` from `PUBLIC`, so only the owner creates there; the serving
  role keeps `PUBLIC`'s `CONNECT` and `USAGE`.

## Folds of the journal vs operational tables

Two kinds of table live in this schema, and the distinction decides what reconciliation
([reconciliation](../architecture/reconciliation.md)) may repair by re-folding:

**Source of truth** — `journal`, `journal_chain_heads`. Never rebuilt from anything.

**Folds of the journal** — deterministically recomputable by replaying events from genesis (or
incrementally from a cursor); a metadata repair is exactly such a journalled re-fold:

| Table | Folded from |
|---|---|
| `memories` | submission, review, lifecycle, simulation events |
| `workareas` | workarea create / re-parent / status / crew events, `system.names_assigned` |
| `crews` | `crew.created`, `crew.updated`, `system.names_assigned` |
| `tenants` | the provider chain's `tenant.created`, `system.names_assigned`; `search_default` from the tenant chain's `search.default_set`; `telemetry_query_text_since` from `telemetry.settings_set` |
| `workarea_edges`, `claim_edges` | relate / ratify events |
| `workarea_transitions` | `workarea.status_proposed`, `workarea.status_transition` |
| `vocab_prefixes` | workarea.create events carrying a prefix (ADR-0021) |
| `vocab_closure` | ratified `oml:is_a` claim edges, materialized per `vocab_release` ratification |
| `grants`, `key_denials`, `bans` | keys/grants family events |
| `simulations` | simulation open / end events |
| `connect_codes`, `oauth_connections` (state columns, `resource` among them) | `connection.code_issued`, `connection.opened`, `connection.revoked`, and `key.revoked` for a voided code |
| `api_keys` (state columns, `credential` among them) | key issue / rotate / revoke events, `system.names_assigned` |
| `web_emails` (all but the address and the pinned subjects) | `web.email_allowed`, `web.email_changed`, `web.email_removed` |

A chain's `system.names_assigned` sits later in the chain than the creation events of the rows it
names, and those rows' `name` and `path` are `NOT NULL`. So a fold of a chain from genesis is two
passes: it reads the chain's `system.names_assigned` first, when the chain holds one, and then
replays the chain in order, folding each creation event that predates names with the name that
event assigns, the path derived from those names and, for a workarea, the `crew_id` of the crew
record the event lists the workarea under, which the fold inserts before the first workarea that
names it. A row is never folded with a NULL name, so the final shape keeps `name` and `path`
`NOT NULL`. The incremental fold at the lead's first start on the names schema is the one that
fills NULL columns, on rows the PoC wrote before the migration.

One honest exception inside that last row: `api_keys.hash` is operational secret material. The
journal records issuance **without the key value** (show-once ceremony), so a re-fold reconstructs
every key's existence, role, credential, issuer and revocation — but not its hash; `credential`,
not a null hash, is what tells a connector key from a re-folded bearer key. Losing the `api_keys`
table means re-issuing keys, not re-folding them. `connect_codes.hash` and
`oauth_connections.auth_code_hash` are the same kind of material, never journalled: a re-fold
restores which codes and connections existed and when they ended, not the digests.

**Operational** — state the journal does not define; losing it costs availability or history, not
truth: `outbox` (deliveries not yet applied are re-derivable from the journal fold, which is what a
lake repair does), `lake_frontier` (cache), `lead_lease` (holder id, address, the lead's
`oauth.issuer`, epoch, expiry — the
cluster-mode lead election row, [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md);
never load-bearing, the chain lock is), `dead_letter`, `idempotency` (replay protection; loss
re-opens a replay window bounded by key behaviour), `read_events` (retention-bounded by design),
`artifact_keys` (key material — loss is unintended erasure, which is why its durability posture
matters), `memories_fts` and `memory_embeddings` (rebuildable from promoted bodies while DEKs
exist), `upload_sessions` (in-flight uploads restart), `oauth_tokens` (loss ends every open
connection at its next request or refresh, and each needs a new connect code),
`oauth_authorizations` (pages in progress start again), `oauth_clients` (documents are fetched
again; registered clients register again) and `web_sessions` (every person signs in again).

### The review queue's order (0024)

```sql
CREATE INDEX review_queue_order ON review_queue (tenant_id, held_since, memory_id);
```

`review next` and the held listing serve the queue oldest first, `held_since` then
`memory_id`; walked in this order, a call reads the queue's head and stops at the first row the
reviewer may judge, where before it joined and sorted every held row of the tenant: 100 ms
against 0.17 ms at 45,000 held
([ADR-0055](../adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
d9).


### Origin kind (0030)

```sql
ALTER TABLE memories ADD COLUMN origin_kind text;  -- repo, doc, code, lake, chat, web, manual
CREATE INDEX memories_origin_kind ON memories (tenant_id, origin_kind)
    WHERE origin_kind IS NOT NULL AND origin_kind <> 'manual';
```

`origin_kind` holds the kind the version's submission event signed, written at capture and never
updated; a check keeps it to the closed set. Versions written before 0030 hold `NULL` and are never
backfilled, so every kind filter leaves them out. The filter is one more predicate of the origin
filters, in the candidate statement before ranking's cut ([ADR-0063](../adr/ADR-0063-a-chat-turn-becomes-memory-only-when-the-agent-in-it-writes-the-fact-and-every-memory-says-what-kind-of-source-it-came-from.md) d1-d3). The number
0030 skips the 0026 to 0029 the parallel v0.6.0 lanes use.

### Source anchors and source checks (0025)

```sql
ALTER TABLE memories ADD COLUMN anchor_repo text, … anchor_hasher text,
                     ADD COLUMN source_check_rev text, … source_check_at text;
CREATE INDEX memories_anchor ON memories (tenant_id, anchor_repo, anchor_path text_pattern_ops)
    WHERE anchor_repo IS NOT NULL;
```

Seven nullable `anchor_*` columns hold a version's anchor as its submission event signed it:
written at capture, never updated, and kept by erasure as clear metadata. Six `source_check_*`
columns hold the version's latest `source.checked`, set only on a promoted row; `stale` is read
from them alone, so a served version is never marked from a held one. Checks keep the shape: one
of `anchor_repo` and `anchor_uri`, a path exactly with a repository, `anchor_hash` with
`anchor_hasher`, a closed set of results, and a moved path only on `moved`. The origin filters
read the partial index under the same row security as every read of `memories`: the listing
names them in its statement; a filtered recall reads its lists whole and keeps the admitted rows
in order before the cut, so it never under-returns
([ADR-0057](../adr/ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
d1–d5, d18; FR-115 measures what reading the lists whole costs).
## Migrations

The schema reaches a database only as **sqlx migrations**: plain-SQL files under `migrations/`,
numbered `NNNN_name` (`0001_journal`, `0002_memories`, …), applied in order by `sqlx migrate`.
The rules:

- **Forward-only.** No down migrations exist. A mistake is corrected by the next numbered
  migration, never by rolling one back — the journal it sits next to is append-only, and a schema
  history that can silently rewind would be the one part of the system allowed to lie about its
  past. Restoring an older database state is a restore ceremony
  ([upgrades](../operations/upgrades.md)), not a migration direction.
- **Run by the bootstrap workload, under the owner role, on first install and on every
  upgrade.** The same bootstrap job that performs first-install setup applies pending migrations
  at every version change, connecting as the schema **owner** role — the owner/migration-role
  split that M0 spike 3 verifies ([critical path](../product/critical-path.md)) and
  [security](../architecture/security.md) requires: application roles cannot alter schema, and
  the owner role is used by nothing else. The upgrade ordering (migrate, then roll workloads) is
  owned by the [upgrades](../operations/upgrades.md) page. The bootstrap workload is decided, not
  built: until it is, the lead migrates on boot under `postgres.owner_url`, closes that pool and
  serves under `postgres.url`, and opens an owner connection afterwards only for a new tenant's
  partition ([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d4,
  d11).
- **The sync rule.** This page is **normative in shape**; the migrations are **normative in
  bytes**. A schema change is one PR that lands both: the migration that makes it true and the
  edit here that keeps this page a faithful sketch of the result. Neither surface may drift
  ahead of the other — once code exists, CI greps the two for drift (no such check exists today;
  until then the rule is held by review, stated per
  [ADR-0018](../adr/ADR-0018-the-documentation-set.md) d6's convention of naming that gap). The
  sqlx offline query check ([CI and toolchain](../engineering/ci-and-toolchain.md)) covers the
  third surface, queries-versus-migrations, by construction.
