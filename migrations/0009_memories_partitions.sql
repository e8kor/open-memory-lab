-- 0009_memories_partitions — the FTS index is per tenant: memories is
-- list-partitioned by tenant_id, each partition with its own GIN index
-- (ADR-0034 d11, d13, d10 step 4). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0008 are never edited,
-- a correction is the next numbered file. Normative shape:
-- docs/data/postgres-schema.md "Full-text search and embeddings", the
-- per-tenant partitions.
--
-- One memories_claim_fts index over every tenant's rows gave the owner's
-- pg_stats view, and EXPLAIN's estimate to every role, other tenants'
-- lexemes (spike 3, S3.1 and S3.4). Rebuilt as PARTITION BY LIST
-- (tenant_id), the table keeps its columns, checks, indexes, keys and
-- tenant_isolation policy; its key becomes (tenant_id, id), since a unique
-- key on a partitioned table must include the partition key
-- (PostgreSQL 17, "Table Partitioning"), so 0008's memories_tenant_id_id,
-- the same columns, is dropped with the old table and review_queue's key
-- takes the primary key instead. memories_claim_fts becomes a partitioned
-- index: each partition holds its own.
--
-- Each tenant gets a partition named memories_t_<its id, 32 hex>, never its
-- name, which would put every tenant's name in pg_class for every role. It is
-- built as the lead builds a new tenant's (oml-pg PgStore's
-- create_tenant_partitions): CREATE TABLE … (LIKE memories INCLUDING ALL),
-- ENABLE and FORCE ROW LEVEL SECURITY and the parent's tenant_isolation
-- policy, then ALTER TABLE memories ATTACH PARTITION … FOR VALUES IN (…),
-- then filled from the old table under that tenant's own context, listed
-- through 0003's startup policy, as 0004 and 0005 do; no policy is lifted.
-- No DEFAULT partition exists, so a row whose tenant has no partition is
-- refused (23514), never stored where every tenant's rows meet.
--
-- No function is marked LEAKPROOF (ADR-0034 d13): only a superuser can, and
-- the owner that runs this is not one. An FTS read scans its tenant's
-- partition under row-level security.
--
-- Run by the owner role. The serving role's DML on memories is 0007's,
-- granted again on the new parent through oml.serving_role; a partition is
-- reached only through its parent, so no partition grants anything
-- (postgres-schema.md "Privileges").

------------------------------------------------------------------------------
-- The old table steps aside: its only inbound key goes, and its indexes and
-- key constraints are dropped so the new table takes their names.
------------------------------------------------------------------------------
ALTER TABLE review_queue DROP CONSTRAINT review_queue_memory_id_fkey;

ALTER TABLE memories RENAME TO memories_0008;
ALTER TABLE memories_0008
    DROP CONSTRAINT memories_pkey,
    DROP CONSTRAINT memories_tenant_id_id;
DROP INDEX memories_workarea, memories_claim_fts, memories_claim_version,
           memories_served_tip, memories_author;

------------------------------------------------------------------------------
-- The partitioned parent: the same columns in the same order, defaults and
-- checks (LIKE), keyed (tenant_id, id).
------------------------------------------------------------------------------
CREATE TABLE memories (
    LIKE memories_0008 INCLUDING DEFAULTS INCLUDING CONSTRAINTS INCLUDING STORAGE,
    CONSTRAINT memories_pkey PRIMARY KEY (tenant_id, id),
    CONSTRAINT memories_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT memories_created_event_id_fkey
        FOREIGN KEY (tenant_id, created_event_id) REFERENCES journal (tenant_id, event_id)
) PARTITION BY LIST (tenant_id);

CREATE INDEX memories_workarea ON memories (tenant_id, workarea_id, status);
CREATE INDEX memories_claim_fts ON memories
    USING gin (to_tsvector('simple', claim));
CREATE UNIQUE INDEX memories_claim_version
    ON memories (tenant_id, workarea_id, claim_key, version)
    WHERE status IN ('held', 'promoted', 'demoted');
CREATE UNIQUE INDEX memories_served_tip
    ON memories (tenant_id, workarea_id, claim_key)
    WHERE is_tip AND status = 'promoted';
CREATE INDEX memories_author ON memories (tenant_id, author_key_id, created_at DESC);

ALTER TABLE memories ENABLE ROW LEVEL SECURITY;
ALTER TABLE memories FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON memories
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE memories IS
    'Fold of submission/review/lifecycle journal events into current claim state (PoC shape), '
    'list-partitioned by tenant_id, one partition memories_t_<tenant id hex> per tenant. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- One partition per tenant, fenced before it is attached, then filled under
-- its own tenant's context.
------------------------------------------------------------------------------
DO $$
DECLARE
    t         uuid;
    partition text;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        partition := 'memories_t_' || replace(t::text, '-', '');
        EXECUTE format('CREATE TABLE %I (LIKE memories INCLUDING ALL)', partition);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', partition);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', partition);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I '
            'USING (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid) '
            'WITH CHECK (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid)',
            partition);
        EXECUTE format('ALTER TABLE memories ATTACH PARTITION %I FOR VALUES IN (%L)',
                       partition, t);

        PERFORM set_config('oml.tenant_id', t::text, true);
        INSERT INTO memories SELECT * FROM memories_0008 WHERE tenant_id = t;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END $$;

-- Every row's tenant_id names a tenants row (memories_tenant_id_fkey), so
-- the loop copied every row; the old table goes.
DROP TABLE memories_0008;

------------------------------------------------------------------------------
-- review_queue's key into memories, as 0008 wrote it, now onto the
-- partitioned table's primary key.
------------------------------------------------------------------------------
ALTER TABLE review_queue
    ADD CONSTRAINT review_queue_memory_id_fkey
        FOREIGN KEY (tenant_id, memory_id) REFERENCES memories (tenant_id, id);

------------------------------------------------------------------------------
-- 0007's grant on memories, on the new parent.
------------------------------------------------------------------------------
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON memories TO %I',
                   current_setting('oml.serving_role'));
END $$;
