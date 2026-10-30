-- 0001_init — PoC (M1) slice of the OML system of record.
--
-- Forward-only sqlx migration (docs/data/postgres-schema.md, "Migrations"):
-- no down migration exists; a mistake is corrected by the next numbered file.
-- Normative shapes: docs/data/postgres-schema.md (tables), docs/data/chain-format.md
-- (envelope bytes), docs/data/event-catalog.md (event kinds). This file covers only
-- the M1 slice — submit → review verdict → promote → recall over the standalone
-- profile — with the per-tenant signed journal chain as the core. Tables the full
-- schema defines but M1 does not touch (grants, simulations, lake/dead_letter,
-- read_events, artifact_keys, embeddings, upload_sessions, …) land in later
-- numbered migrations.
--
-- Roles are cluster-level and are minted by the bootstrap workload, not by a
-- per-database migration (docs/operations/deployment.md, docs/operations/upgrades.md):
-- the owner/application role split (C6) is enforced there. gen_random_uuid() is
-- built in on the pinned Postgres majors; no extension is required by this slice.

------------------------------------------------------------------------------
-- tenants — the tenant registry (docs/data/postgres-schema.md; provider acts
-- that create tenants are journalled per docs/data/event-catalog.md tenant.created).
------------------------------------------------------------------------------
CREATE TABLE tenants (
    id         uuid           PRIMARY KEY DEFAULT gen_random_uuid(),
    name       text           NOT NULL,
    created_at timestamptz(6) NOT NULL DEFAULT now()
);
COMMENT ON TABLE tenants IS
    'Tenant registry. Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- workareas — the governed tree with the status ladder
-- (docs/data/postgres-schema.md "workareas and edges"; the ladder semantics are
-- docs/architecture/workareas.md's). M1 carries no ltree path column; grants
-- bind to node ids, and the path machinery lands with the workarea milestone.
------------------------------------------------------------------------------
CREATE TABLE workareas (
    id         uuid           PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id  uuid           NOT NULL REFERENCES tenants (id),
    parent_id  uuid           REFERENCES workareas (id),   -- NULL only for universe
    name       text           NOT NULL,
    status     text           NOT NULL
        CHECK (status IN ('development', 'committed', 'validated')),
    created_at timestamptz(6) NOT NULL DEFAULT now()
);

-- exactly one universe root per tenant (docs/data/postgres-schema.md).
CREATE UNIQUE INDEX workareas_universe ON workareas (tenant_id)
    WHERE parent_id IS NULL;

COMMENT ON TABLE workareas IS
    'Governed workarea tree, status ladder development→committed→validated. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- api_keys — authorization state. Key values are never stored, only their
-- SHA-256 hash; issuance is journalled as issued-without-value
-- (docs/data/postgres-schema.md "api_keys, grants, key_denials, bans").
------------------------------------------------------------------------------
CREATE TABLE api_keys (
    id                  uuid           PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           uuid           REFERENCES tenants (id),
                                       -- NULL for provider-level operator keys
    hash                bytea          NOT NULL UNIQUE,
    role                text           NOT NULL
        CHECK (role IN ('reader', 'writer', 'dreamer', 'admin', 'operator')),
    principal           text           NOT NULL,
    held_by             text           NOT NULL DEFAULT 'agent'
        CHECK (held_by IN ('agent', 'human', 'service')),
                                       -- issuance-time attestation (ADR-0021)
    default_workarea_id uuid           REFERENCES workareas (id),
                                       -- provider-ring target (ADR-0022 d4)
    rotated_from        uuid           REFERENCES api_keys (id),
    created_at          timestamptz(6) NOT NULL DEFAULT now(),
    revoked_at          timestamptz(6)
);
COMMENT ON TABLE api_keys IS
    'API keys: hash only, never the value. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- journal — the append-only, per-tenant signed hash chain
-- (docs/data/postgres-schema.md "journal and chain heads"; byte rules in
-- docs/data/chain-format.md; event kinds in docs/data/event-catalog.md).
-- signed_bytes_hash is the chain 'hash': SHA-256 of the RFC 8785 canonical
-- signed_bytes; the stored-signed_bytes column itself joins with the chain
-- verifier work in a later migration.
------------------------------------------------------------------------------
CREATE TABLE journal (
    tenant_id         uuid           NOT NULL REFERENCES tenants (id),
    seq               bigint         NOT NULL CHECK (seq >= 1),
                                     -- per-tenant; assigned under the chain-head lock
    event_id          uuid           NOT NULL UNIQUE,
    event_type        text           NOT NULL,   -- docs/data/event-catalog.md spelling
    prev_hash         bytea          NOT NULL,   -- 32 zero bytes at genesis (seq 1)
    principal         text           NOT NULL,
    api_key_id        uuid           REFERENCES api_keys (id),
                                     -- NULL for events no API key authored
    at                timestamptz(6) NOT NULL DEFAULT now(),
    payload           jsonb          NOT NULL,   -- digest references only, never bodies (D1)
    signed_bytes_hash bytea          NOT NULL,   -- SHA-256(signed_bytes), the chained hash
    sig               bytea          NOT NULL,   -- pure Ed25519 over signed_bytes (RFC 8032)
    PRIMARY KEY (tenant_id, seq)
);
COMMENT ON TABLE journal IS
    'Append-only per-tenant signed hash chain; the system of record. '
    'Owning pages: docs/data/postgres-schema.md, docs/data/chain-format.md.';

-- Append-only enforcement is layered (ADR-0002 d4): privileges first, a guard
-- trigger second. Per-application-role REVOKEs join when bootstrap mints the
-- roles; a superuser bypass is treated as deployment-key-equivalent compromise
-- (docs/architecture/security.md).
REVOKE UPDATE, DELETE, TRUNCATE ON journal FROM PUBLIC;

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

------------------------------------------------------------------------------
-- journal_chain_head — one head row per tenant, read under the advisory
-- chain-head lock at append time (docs/data/postgres-schema.md).
------------------------------------------------------------------------------
CREATE TABLE journal_chain_head (
    tenant_id uuid   PRIMARY KEY REFERENCES tenants (id),
    seq       bigint NOT NULL CHECK (seq >= 1),
    head_hash bytea  NOT NULL
);
COMMENT ON TABLE journal_chain_head IS
    'Per-tenant chain head (last seq + hash); serialization point of the chain. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- outbox — the durable delivery row committed in the same transaction as its
-- journal event (docs/data/postgres-schema.md "outbox, dead_letter, idempotency").
-- bigserial seq is assigned at insert and visible only at commit; consumers
-- drain the stable prefix only (docs/architecture/storage.md).
------------------------------------------------------------------------------
CREATE TABLE outbox (
    seq        bigserial      PRIMARY KEY,   -- single drain stream, permanent gaps allowed
    tenant_id  uuid           NOT NULL,
    tenant_seq bigint         NOT NULL,      -- the journal event this row rides with
    kind       text           NOT NULL
        CHECK (kind IN ('bronze-event', 'materialize', 'retract', 'export',
                        'import-materialize')),  -- reserved: ADR-0020, post-v1
    payload    jsonb          NOT NULL,      -- digest references only, never bodies
    created_at timestamptz(6) NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, tenant_seq) REFERENCES journal (tenant_id, seq)
);
COMMENT ON TABLE outbox IS
    'Transactional outbox between the commit point and downstream consumers. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- memories — the PoC fold of submission/review/lifecycle events into current
-- claim state (docs/data/postgres-schema.md "memories"; the full versioned,
-- digest-only shape with body_ref/claim_key lands with the quarantine and
-- lake milestones — the PoC stores the claim text inline).
------------------------------------------------------------------------------
CREATE TABLE memories (
    id               uuid           PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        uuid           NOT NULL REFERENCES tenants (id),
    workarea_id      uuid           NOT NULL REFERENCES workareas (id),
    claim            text           NOT NULL,
    claim_digest     bytea          NOT NULL,  -- SHA-256 of the claim bytes
    status           text           NOT NULL
        CHECK (status IN ('held', 'promoted', 'rejected', 'superseded')),
    created_event_id uuid           NOT NULL REFERENCES journal (event_id),
    superseded_by    uuid           REFERENCES memories (id),
    terms            text[]         NOT NULL DEFAULT '{}',  -- CURIE annotations (ADR-0021)
    created_at       timestamptz(6) NOT NULL DEFAULT now()
);

CREATE INDEX memories_workarea ON memories (tenant_id, workarea_id, status);

-- Recall: the bound FTS decision — a simple-configuration GIN expression index
-- (docs/data/postgres-schema.md "Full-text search and embeddings"; the separate
-- memories_fts/embeddings tables join at the retrieval milestone).
CREATE INDEX memories_claim_fts ON memories
    USING gin (to_tsvector('simple', claim));

COMMENT ON TABLE memories IS
    'Fold of submission/review/lifecycle journal events into current claim state (PoC shape). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- review_queue — held submissions awaiting a verdict
-- (the review ceremony: docs/data/event-catalog.md "Review family";
-- docs/architecture/admission.md owns the policy).
------------------------------------------------------------------------------
CREATE TABLE review_queue (
    memory_id   uuid           PRIMARY KEY REFERENCES memories (id),
    tenant_id   uuid           NOT NULL REFERENCES tenants (id),
    held_since  timestamptz(6) NOT NULL DEFAULT now(),
    held_by_key uuid           REFERENCES api_keys (id),
    expires_at  timestamptz(6)
);
COMMENT ON TABLE review_queue IS
    'Held submissions awaiting review verdicts. '
    'Owning pages: docs/data/event-catalog.md (review family), docs/architecture/admission.md.';

------------------------------------------------------------------------------
-- idempotency — request replay protection
-- (docs/data/postgres-schema.md "outbox, dead_letter, idempotency"). The 24h
-- retention floor is policy enforced by a sweeper, not by this schema.
------------------------------------------------------------------------------
CREATE TABLE idempotency (
    tenant_id      uuid           NOT NULL REFERENCES tenants (id),
    key            text           NOT NULL,
    request_digest bytea          NOT NULL,
    response       jsonb,
    committed_at   timestamptz(6),
    PRIMARY KEY (tenant_id, key)
);
COMMENT ON TABLE idempotency IS
    'Idempotency records: same key + different request_digest is a typed error; '
    'replay with the same digest returns the stored response. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- Row-level security — every tenant-scoped table, FORCE so the owner is bound
-- too (docs/architecture/security.md; sketch in docs/data/postgres-schema.md
-- "Row-level security"). oml-server derives oml.tenant_id from the API key and
-- SET LOCALs it at the start of every transaction — never from the request.
--
-- current_setting(..., true) (missing_ok) is used so an unset GUC yields NULL
-- and therefore NO rows — fail closed without erroring — which is what lets the
-- pre-tenant key-resolution transaction (below) run at all.
------------------------------------------------------------------------------
ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenants FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON tenants
    USING (id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE workareas ENABLE ROW LEVEL SECURITY;
ALTER TABLE workareas FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON workareas
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON api_keys
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
-- Key resolution happens before the tenant is known: the server SET LOCALs
-- oml.api_key_hash (hex of the presented key's SHA-256), resolves exactly that
-- row, then sets oml.tenant_id for the rest of the transaction. No tenant
-- session can enumerate another tenant's keys; provider-level rows
-- (tenant_id IS NULL) are reachable only through this exact-hash path.
CREATE POLICY key_resolution ON api_keys FOR SELECT
    USING (hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

ALTER TABLE journal ENABLE ROW LEVEL SECURITY;
ALTER TABLE journal FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON journal
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE journal_chain_head ENABLE ROW LEVEL SECURITY;
ALTER TABLE journal_chain_head FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON journal_chain_head
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE outbox ENABLE ROW LEVEL SECURITY;
ALTER TABLE outbox FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON outbox
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE memories ENABLE ROW LEVEL SECURITY;
ALTER TABLE memories FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON memories
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE review_queue ENABLE ROW LEVEL SECURITY;
ALTER TABLE review_queue FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON review_queue
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE idempotency FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON idempotency
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
