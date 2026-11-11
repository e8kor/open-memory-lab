-- The tip probe of as-of reads (ADR-0059 d15, d16, d20).
--
-- An as-of read takes a claim's tip at an instant as the version of
-- greatest `version` among the claim's versions whose promoted_at is at or
-- before it. Each candidate version asks whether a later version of its
-- (workarea_id, claim_key) was promoted by then; this index answers that
-- probe by an index condition under row security, since equality on uuid
-- and text, int4gt and timestamptz_le are leakproof
-- (pg_proc.proleakproof, PostgreSQL 18, checked 2026-11-11). It holds the
-- promoted versions of every state today, so a successor demoted or erased
-- since still ends its predecessor's tip at the instants after its
-- promotion.
--
-- Safe on a live database, as 0026 is: a lock is waited for at most 5 s,
-- then the migration fails and is run again in a quieter moment. The index
-- is created ON ONLY the parent, each partition's is built on its own and
-- attached, so no write waits on more than one partition at a time. A
-- tenant partition created later inherits it.

SET LOCAL lock_timeout = '5s';

CREATE INDEX memories_tip_moves
    ON ONLY memories (tenant_id, workarea_id, claim_key, version)
    INCLUDE (promoted_at)
    WHERE promoted_at IS NOT NULL;

DO $$
DECLARE
    part regclass;
    name text;
BEGIN
    FOR part IN SELECT inhrelid::regclass FROM pg_inherits
                 WHERE inhparent = 'memories'::regclass
    LOOP
        name := replace(part::text, '"', '');
        EXECUTE format(
            'CREATE INDEX %I ON %s (tenant_id, workarea_id, claim_key, version) '
            'INCLUDE (promoted_at) WHERE promoted_at IS NOT NULL',
            name || '_tip_moves', part);
        EXECUTE format('ALTER INDEX memories_tip_moves ATTACH PARTITION %I',
                       name || '_tip_moves');
    END LOOP;
END $$;
