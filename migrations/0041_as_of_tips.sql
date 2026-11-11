-- no-transaction
-- The tip probe of as-of reads (ADR-0059 d15, d16, d20).
--
-- An as-of read takes a claim's tip at an instant as the version of
-- greatest `version` among the claim's versions whose promoted_at is at or
-- before it. A replaced candidate asks for that greatest version of its
-- (workarea_id, claim_key); this index answers by an index condition under
-- row security, since equality on uuid and text, int4gt and timestamptz_le
-- are leakproof (pg_proc.proleakproof, PostgreSQL 18, checked 2026-11-11).
-- It holds the promoted versions of every state today, so a successor
-- demoted or erased since still ends its predecessor's tip at the instants
-- after its promotion.
--
-- What it costs a live database, stated: unlike 0026's indexes, which were
-- empty at creation, this one holds every promoted version, so each
-- partition's build reads its tenant's rows under a SHARE lock that makes
-- that tenant's writes wait until the build ends. The file is one DO
-- statement run outside a migration transaction, `-- no-transaction`, and
-- it builds the partitions in name order, committing after each build and
-- attach, so no lock is held past its own partition's build. An INSERT,
-- which locks its tenant's partition alone, waits only for that build. A
-- statement planned against the parent with the tenant as a parameter,
-- such as a cached generic plan of an UPDATE through memories, can lock
-- every partition as it starts, and then waits behind whichever tenant's
-- build is running, for at most that one build. Reads never wait: SHARE
-- does not conflict with them. CREATE INDEX
-- CONCURRENTLY would let writes run, but it cannot run inside a DO block or
-- a multi-statement string, and the partitions are known only at run time.
-- Each transaction waits at most 5 s for its lock, then the migration fails
-- and is run again: every step is skipped when already done, so a retry
-- resumes where the failure left off. A tenant partition created later
-- inherits the index. Forward-only, as every migration here: undoing it is
-- DROP INDEX memories_tip_moves, which drops each partition's index with it.

DO $$
DECLARE
    part regclass;
    name text;
BEGIN
    SET LOCAL lock_timeout = '5s';
    CREATE INDEX IF NOT EXISTS memories_tip_moves
        ON ONLY memories (tenant_id, workarea_id, claim_key, version)
        INCLUDE (promoted_at)
        WHERE promoted_at IS NOT NULL;
    COMMIT;
    FOR part IN SELECT inhrelid::regclass FROM pg_inherits
                 WHERE inhparent = 'memories'::regclass
                 ORDER BY inhrelid::regclass::text
    LOOP
        SET LOCAL lock_timeout = '5s';
        -- A partition created after the parent index exists has its copy
        -- already, and a retry finds the ones it attached.
        CONTINUE WHEN EXISTS (
            SELECT 1 FROM pg_inherits i JOIN pg_index x ON x.indexrelid = i.inhrelid
             WHERE i.inhparent = 'memories_tip_moves'::regclass AND x.indrelid = part);
        name := replace(part::text, '"', '') || '_tip_moves';
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %s (tenant_id, workarea_id, claim_key, version) '
            'INCLUDE (promoted_at) WHERE promoted_at IS NOT NULL',
            name, part);
        EXECUTE format('ALTER INDEX memories_tip_moves ATTACH PARTITION %I', name);
        COMMIT;
    END LOOP;
END $$;
