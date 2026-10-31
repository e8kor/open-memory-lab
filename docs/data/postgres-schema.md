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
    api_key_id   uuid,
    at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, seq),
    UNIQUE (event_id)
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

## workareas and edges

The governed tree (one per tenant, rooted at `universe`) plus the two edge tables. The algebra —
node-id grants, deny semantics, status ladder — is declared in
[workareas](../architecture/workareas.md); this page holds only the shapes.

```sql
CREATE EXTENSION IF NOT EXISTS ltree;

CREATE TABLE workareas (
    id         uuid        PRIMARY KEY,
    tenant_id  uuid        NOT NULL,
    parent_id  uuid        REFERENCES workareas (id),   -- NULL only for universe
    path       ltree       NOT NULL,   -- DERIVED; rewritten in one transaction on re-parent
    status     text        NOT NULL
        CHECK (status IN ('development', 'committed', 'validated')),
    crew       jsonb,                  -- {name, charter_uri, external_manager}
    created_at timestamptz NOT NULL,
    UNIQUE (tenant_id, id)             -- target of the tenant-scoped grant and denial references
);

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

CREATE TABLE claim_edges (
    id              uuid PRIMARY KEY,
    tenant_id       uuid NOT NULL,
    src_artifact_id uuid NOT NULL,
    dst_artifact_id uuid NOT NULL,
    label           text NOT NULL,     -- derives_from | relates_to | …
    state           text NOT NULL CHECK (state IN ('pending', 'ratified')),
    proposer_key_id uuid NOT NULL,
    ratifier_key_id uuid
);
```

The key-inequality `CHECK` is the cheap spelling of R2 on edges; the full rule — ratifier principal
distinct from proposer principal, resolved at ratification time — is enforced in the ratification
transaction, because a principal can hold several keys and a column constraint cannot see that.
`path` is derived: grants bind to node ids, and every path is recomputable from `parent_id`, so a
re-parent is a single-transaction descendant rewrite rather than a grant migration.

## api_keys, grants, key_denials, bans

Authorization state. Key values are never stored — only their hash; issuance is journalled as
issued-without-value (D9, declared in [roles-and-keys](../architecture/roles-and-keys.md)).

```sql
CREATE TABLE api_keys (
    id         uuid        PRIMARY KEY,
    tenant_id  uuid,                    -- NULL for provider-level operator keys
    hash       bytea       UNIQUE,      -- SHA-256 of the value; NULL exactly for a connector
                                        -- key, which has no value (ADR-0031 d7)
    role       text        NOT NULL
        CHECK (role IN ('reader', 'writer', 'dreamer', 'admin', 'operator')),
    principal  text        NOT NULL,
    issued_by  uuid        REFERENCES api_keys (id),  -- issuer ceiling checked at issuance
    rotated_from uuid      REFERENCES api_keys (id),  -- successor link; predecessor revoked at grace end
    held_by    text        NOT NULL DEFAULT 'agent'
        CHECK (held_by IN ('agent', 'human', 'service')),
                                          -- issuance-time attestation by the issuing admin
                                          -- (ADR-0021): verified as an attestation, never as fact;
                                          -- human_required_classes consumes it — only 'human'
                                          -- satisfies it; 'service' never counts as review capacity
    default_workarea_id uuid REFERENCES workareas (id),
                                          -- provider-ring target (ADR-0022 d4): a /v1 request
                                          -- names no workarea — this, or X-OML-Workarea, resolves it
    issued_seq bigint      NOT NULL,      -- journal seq of this key's key.issued, or of the
                                          -- key.rotated that minted it; the floor of its
                                          -- permission epoch (ADR-0025 d14)
    expiry     timestamptz,
    revoked_at timestamptz,
    UNIQUE (tenant_id, id)                -- target of the tenant-scoped grant and denial references
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
    epoch            bigint NOT NULL,               -- journal seq of this grant's grant.issued,
                                                    -- or of the key.rotated that reproduced it
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id),
    FOREIGN KEY (tenant_id, workarea_node_id) REFERENCES workareas (tenant_id, id)
);

CREATE TABLE key_denials (          -- global per-key deny: deny-wins across all grants
    id               uuid        PRIMARY KEY,
    tenant_id        uuid        NOT NULL,
    key_id           uuid        NOT NULL,
    workarea_node_id uuid        NOT NULL,
    epoch            bigint      NOT NULL,   -- journal seq of this denial's key.denied
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
attenuated by its own `deny` entries, then masked by any `key_denials` row. The `epoch` column is
the journal seq of the event that wrote the row, which makes every historical computation
reproducible from journalled grants ([workareas](../architecture/workareas.md) owns the
permission-epoch semantics).
A grant's and a denial's key and node references carry the row's `tenant_id`. In PostgreSQL,
"referential integrity checks, such as unique or primary key constraints and foreign key
references, always bypass row security"
([Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html), fetched
2026-10-30), so a reference on `id` alone would accept a row naming another tenant's key or node.
A provider-level key (`tenant_id` NULL) matches no such reference, and carries no grant
([grant admissibility](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)).
Bans are checked at admission; a ban's reach is previewable (`--dry-run`) before it is imposed.

## connections (ADR-0031)

Connect codes, connections, their tokens, and the deployment-level state the authorization server
keeps before a code names a key ([roles and keys](../architecture/roles-and-keys.md#connections),
[HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)). No code, token or
authorization code is stored, only its SHA-256.

```sql
CREATE TABLE oauth_clients (           -- clients the authorization server knows; deployment-level
    client_id       text        PRIMARY KEY,   -- a metadata document's https URL, or server-minted
    tenant_id       uuid        NOT NULL,      -- always the reserved provider tenant
    kind            text        NOT NULL CHECK (kind IN ('cimd', 'dcr')),
    client_name     text        NOT NULL,
    redirect_uris   text[]      NOT NULL,      -- each inside the redirect allowlist
    metadata_digest bytea       NOT NULL,      -- SHA-256 of the document or registration as received
    fetched_at      timestamptz,               -- cimd: when the cached document was fetched
    created_at      timestamptz NOT NULL
);

CREATE TABLE oauth_authorizations (    -- one row per authorization request on the page
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL,       -- always the reserved provider tenant
    handle_hash    bytea       NOT NULL UNIQUE,-- SHA-256 of the handle the page's form carries
    client_id      text        NOT NULL REFERENCES oauth_clients (client_id),
    redirect_uri   text        NOT NULL,
    code_challenge text        NOT NULL,       -- S256 only
    state          text,                       -- echoed verbatim
    attempts_left  integer     NOT NULL,       -- five at creation
    expires_at     timestamptz NOT NULL,       -- ten minutes after creation
    connection_id  uuid                        -- set when a connect code verifies; plain uuid,
                                               -- since the connection is another tenant's row
);

CREATE TABLE connect_codes (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    key_id      uuid        NOT NULL,
    hash        bytea       NOT NULL UNIQUE,   -- SHA-256 of the 12 decoded symbols
    issued_seq  bigint      NOT NULL,          -- journal seq of its connection.code_issued
    expires_at  timestamptz NOT NULL,
    redeemed_at timestamptz,                   -- its first correct entry on the page
    voided_seq  bigint,                        -- seq of the connection.code_issued that replaced it
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id)
);

-- one live code per key: minting a code voids every unredeemed one of the key in its commit.
CREATE UNIQUE INDEX connect_codes_live ON connect_codes (key_id)
    WHERE redeemed_at IS NULL AND voided_seq IS NULL;

CREATE TABLE oauth_connections (
    id                   uuid        PRIMARY KEY,
    tenant_id            uuid        NOT NULL,
    key_id               uuid        NOT NULL,
    code_id              uuid        NOT NULL REFERENCES connect_codes (id),
    client_id            text        NOT NULL,  -- no reference: oauth_clients is the provider tenant's
    redirect_uri         text        NOT NULL,
    code_challenge       text        NOT NULL,
    auth_code_hash       bytea       UNIQUE,    -- the authorization code, until it is redeemed
    auth_code_expires_at timestamptz,
    state                text        NOT NULL CHECK (state IN ('authorized', 'open', 'revoked')),
    opened_seq           bigint,                -- journal seq of its connection.opened
    revoked_seq          bigint,                -- its connection.revoked, or the connection.opened
                                                -- that replaced it
    revoked_cause        text        CHECK (revoked_cause IN
                             ('admin', 'refresh_reuse', 'code_reuse', 'replaced')),
    created_at           timestamptz NOT NULL,
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id)
);

-- at most one open connection per key (ADR-0031 d7).
CREATE UNIQUE INDEX oauth_connections_open ON oauth_connections (key_id) WHERE state = 'open';

CREATE TABLE oauth_tokens (
    hash          bytea       PRIMARY KEY,      -- SHA-256 of the token string
    tenant_id     uuid        NOT NULL,
    connection_id uuid        NOT NULL REFERENCES oauth_connections (id),
    kind          text        NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at    timestamptz NOT NULL,
    rotated_at    timestamptz,                  -- refresh only: presenting it after this is reuse
    created_at    timestamptz NOT NULL
);
```

`connect_codes`, `oauth_connections` and `oauth_tokens` carry their key's `tenant_id` and the
tenant's RLS. `oauth_clients` and `oauth_authorizations` hold state from before any code names a
key, so they belong to the reserved provider tenant, under its RLS, and hold no tenant content: a
client's self-description and a page's request in progress. The code, authorization-code and token
digests are found by the same pre-tenant authentication lookup that finds `api_keys.hash`; the
matched row's `tenant_id` then sets `oml.tenant_id` for everything after. Removal is operational:
an authorization request's row 24 hours after it lapses, a registered client 24 hours after its
registration when no connection used it, a token row once it has expired or its connection was
revoked.

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
    workarea_id    uuid        NOT NULL,  -- the nil UUID (00000000-0000-0000-0000-000000000000)
                                          -- for endpoints that name no workarea
    idem_key       text        NOT NULL,
    request_digest bytea       NOT NULL,
    state          text        NOT NULL CHECK (state IN ('in_progress', 'committed')),
    response       jsonb,
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

## Folds of the journal vs operational tables

Two kinds of table live in this schema, and the distinction decides what reconciliation
([reconciliation](../architecture/reconciliation.md)) may repair by re-folding:

**Source of truth** — `journal`, `journal_chain_heads`. Never rebuilt from anything.

**Folds of the journal** — deterministically recomputable by replaying events from genesis (or
incrementally from a cursor); a metadata repair is exactly such a journalled re-fold:

| Table | Folded from |
|---|---|
| `memories` | submission, review, lifecycle, simulation events |
| `workareas` | workarea create / re-parent / status / crew events |
| `workarea_edges`, `claim_edges` | relate / ratify events |
| `vocab_prefixes` | workarea.create events carrying a prefix (ADR-0021) |
| `vocab_closure` | ratified `oml:is_a` claim edges, materialized per `vocab_release` ratification |
| `grants`, `key_denials`, `bans` | keys/grants family events |
| `simulations` | simulation open / end events |
| `api_keys` (state columns) | key issue / revoke events |
| `connect_codes`, `oauth_connections` (state columns) | `connection.code_issued`, `connection.opened`, `connection.revoked` |

One honest exception inside that last row: `api_keys.hash` is operational secret material. The
journal records issuance **without the key value** (show-once ceremony), so a re-fold reconstructs
every key's existence, role, issuer and revocation — but not its hash. Losing the `api_keys` table
means re-issuing keys, not re-folding them. `connect_codes.hash` and
`oauth_connections.auth_code_hash` are the same kind of material, never journalled: a re-fold
restores which codes and connections existed and when they ended, not the digests.

**Operational** — state the journal does not define; losing it costs availability or history, not
truth: `outbox` (deliveries not yet applied are re-derivable from the journal fold, which is what a
lake repair does), `lake_frontier` (cache), `lead_lease` (holder id, address, epoch, expiry — the
cluster-mode lead election row, [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md);
never load-bearing, the chain lock is), `dead_letter`, `idempotency` (replay protection; loss
re-opens a replay window bounded by key behaviour), `read_events` (retention-bounded by design),
`artifact_keys` (key material — loss is unintended erasure, which is why its durability posture
matters), `memories_fts` and `memory_embeddings` (rebuildable from promoted bodies while DEKs
exist), `upload_sessions` (in-flight uploads restart), `oauth_tokens` (loss ends every open
connection at its next request or refresh, and each needs a new connect code),
`oauth_authorizations` (pages in progress start again) and `oauth_clients` (documents are fetched
again; registered clients register again).

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
  owned by the [upgrades](../operations/upgrades.md) page.
- **The sync rule.** This page is **normative in shape**; the migrations are **normative in
  bytes**. A schema change is one PR that lands both: the migration that makes it true and the
  edit here that keeps this page a faithful sketch of the result. Neither surface may drift
  ahead of the other — once code exists, CI greps the two for drift (no such check exists today;
  until then the rule is held by review, stated per
  [ADR-0018](../adr/ADR-0018-the-documentation-set.md) d6's convention of naming that gap). The
  sqlx offline query check ([CI and toolchain](../engineering/ci-and-toolchain.md)) covers the
  third surface, queries-versus-migrations, by construction.
