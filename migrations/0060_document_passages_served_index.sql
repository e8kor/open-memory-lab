-- no-transaction
-- The second half of 0059 (ADR-0081 d4; the review's reading R14): the
-- served postings index rebuilt to carry a posting's passage ordinal, the
-- widened kind check validated, and the postings and counts of every
-- tenant that holds a document version refilled per unit.
--
-- What it costs a live database, stated. The file is one DO statement run
-- outside a migration transaction, `-- no-transaction`, and it commits after
-- each step, so no lock outlives its own step:
--
-- 1. The new index, search_postings_served_units, is created ON ONLY the
--    parent (a catalog change), then built on each partition in name order
--    and attached, each build committed before the next. A partition's
--    build holds its SHARE lock, so reads never wait. Every claim write of
--    that one tenant waits for the whole build of its partition: an insert,
--    a verdict, a promotion and an erasure alike, because the fold writes
--    the version's postings in the same transaction as the version. Other
--    tenants' writes do not wait. The review of PR #475 measured about 5 s
--    per 1.8M postings on one partition, so a tenant's write stall grows
--    with its postings; 0041 builds its index the same way. CREATE INDEX
--    CONCURRENTLY would let writes run, but PostgreSQL
--    refuses it inside a DO block and inside a multi-statement string, and
--    the partitions are known only at run time.
-- 2. The old index is dropped and the new one takes its name: an ACCESS
--    EXCLUSIVE lock on the parent and its partitions for a catalog change.
-- 3. The kind check 0059 added NOT VALID is validated under SHARE UPDATE
--    EXCLUSIVE, which lets reads and writes run while it reads the postings.
-- 4. Each tenant holding a `document` row is refilled under its row
--    security, alone in its own transaction; a tenant with none, which is
--    every tenant written before documents were, is read by one probe of
--    its memories and left as it is.
--
-- Each transaction waits at most 5 s for its lock, then the migration
-- fails and is run again: every step is skipped when already done, so a
-- retry resumes where the failure left off (a refill run twice writes what
-- it wrote once). A tenant partition created later inherits the index.
-- Forward-only, as every migration here.

DO $$
DECLARE
    part regclass;
    name text;
    t uuid;
BEGIN
    SET LOCAL lock_timeout = '5s';
    IF to_regclass('search_postings_served_units') IS NULL
       AND NOT EXISTS (SELECT 1 FROM pg_index x
                        JOIN pg_attribute a ON a.attrelid = x.indexrelid AND a.attname = 'ordinal'
                       WHERE x.indexrelid = to_regclass('search_postings_served')) THEN
        CREATE INDEX search_postings_served_units ON ONLY search_postings
            (kind, token_id, workspace_id)
            INCLUDE (memory_kind, memory_id, ordinal, weight, doc_len, tenant_id) WHERE state = 1;
    END IF;
    COMMIT;
    IF to_regclass('search_postings_served_units') IS NOT NULL THEN
        FOR part IN SELECT inhrelid::regclass FROM pg_inherits
                     WHERE inhparent = 'search_postings'::regclass
                     ORDER BY inhrelid::regclass::text
        LOOP
            SET LOCAL lock_timeout = '5s';
            -- A retry finds the partitions it attached.
            CONTINUE WHEN EXISTS (
                SELECT 1 FROM pg_inherits i JOIN pg_index x ON x.indexrelid = i.inhrelid
                 WHERE i.inhparent = 'search_postings_served_units'::regclass
                   AND x.indrelid = part);
            name := replace(part::text, '"', '') || '_served_units';
            EXECUTE format(
                'CREATE INDEX IF NOT EXISTS %I ON %s (kind, token_id, workspace_id) '
                'INCLUDE (memory_kind, memory_id, ordinal, weight, doc_len, tenant_id) '
                'WHERE state = 1', name, part);
            EXECUTE format('ALTER INDEX search_postings_served_units ATTACH PARTITION %I', name);
            COMMIT;
        END LOOP;
        SET LOCAL lock_timeout = '5s';
        DROP INDEX IF EXISTS search_postings_served;
        ALTER INDEX search_postings_served_units RENAME TO search_postings_served;
        COMMIT;
    END IF;
    SET LOCAL lock_timeout = '5s';
    ALTER TABLE search_postings VALIDATE CONSTRAINT search_postings_memory_kind;
    COMMIT;
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        SET LOCAL lock_timeout = '5s';
        -- The tenants are enumerated under 0003's startup policy, and each
        -- is read and refilled under its own row security, as 0021's fill.
        PERFORM set_config('oml.startup_check', 'on', true);
        PERFORM set_config('oml.tenant_id', t::text, true);
        IF EXISTS (SELECT 1 FROM memories m WHERE m.tenant_id = t AND m.kind = 'document') THEN
            PERFORM oml_search_postings_refill(t);
            DELETE FROM search_vocabulary v WHERE v.tenant_id = t;
            INSERT INTO search_vocabulary (tenant_id, workspace_id, word, rows)
            SELECT m.tenant_id, m.workspace_id, w, count(*)
              FROM memories m
             CROSS JOIN LATERAL unnest(oml_search_words(coalesce(m.search_text, m.claim))) w
             WHERE m.tenant_id = t AND m.status = 'promoted' AND m.is_tip AND NOT m.retirement
               AND m.kind IN ('memory', 'derived_fact', 'document')
             GROUP BY 1, 2, 3;
        END IF;
        COMMIT;
    END LOOP;
END $$;
