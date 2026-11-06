-- Valid time on memories (proposed ADR-0059 d2, d3, d7, d11).
--
-- A version may declare when it holds: valid_from and valid_until, each the
-- signed text the author sent (a period such as 2015 or 2015-03, a day, or
-- the fixed-width instant), and each bound in microseconds since the epoch,
-- UTC: valid_from_us the period's first instant and valid_until_us the
-- instant after its last. Every filter and index reads the integers, compared
-- with bound int8 parameters: int8 comparisons are leakproof
-- (pg_proc.proleakproof for int8lt, int8gt, int8le and int8ge on PostgreSQL
-- 18, checked 2026-11-05), so an index on them serves the serving role under
-- row security.
--
-- An existing row is open on both sides, so nothing is rewritten.
--
-- Safe on a live database, as 0025 is: a lock is waited for at most 5 s,
-- then the migration fails and is run again in a quieter moment. Adding
-- nullable columns is a catalog change. The check is added NOT VALID, which
-- takes no scan under the exclusive lock, then validated under SHARE UPDATE
-- EXCLUSIVE, which lets reads and writes run. The indexes are partial, so
-- empty at creation: each partition's is built on its own, then attached to
-- the parent's, so no write waits on more than one partition at a time.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories
    ADD COLUMN valid_from text,
    ADD COLUMN valid_until text,
    ADD COLUMN valid_from_us int8,
    ADD COLUMN valid_until_us int8;

ALTER TABLE memories
    ADD CONSTRAINT memories_valid_time_ends CHECK (
        (valid_from IS NULL) = (valid_from_us IS NULL)
        AND (valid_until IS NULL) = (valid_until_us IS NULL)
        AND (valid_from_us IS NULL OR valid_until_us IS NULL
             OR valid_from_us < valid_until_us)) NOT VALID;

ALTER TABLE memories VALIDATE CONSTRAINT memories_valid_time_ends;

-- The time leg's window probe (d7) and the expiry probe of bm25-postings
-- (d3): each holds only the rows that declare that end.
CREATE INDEX memories_valid_from
    ON ONLY memories (tenant_id, valid_from_us)
    WHERE valid_from_us IS NOT NULL;
CREATE INDEX memories_valid_until
    ON ONLY memories (tenant_id, valid_until_us)
    WHERE valid_until_us IS NOT NULL;

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
            'CREATE INDEX %I ON %s (tenant_id, valid_from_us) WHERE valid_from_us IS NOT NULL',
            name || '_valid_from', part);
        EXECUTE format('ALTER INDEX memories_valid_from ATTACH PARTITION %I',
                       name || '_valid_from');
        EXECUTE format(
            'CREATE INDEX %I ON %s (tenant_id, valid_until_us) WHERE valid_until_us IS NOT NULL',
            name || '_valid_until', part);
        EXECUTE format('ALTER INDEX memories_valid_until ATTACH PARTITION %I',
                       name || '_valid_until');
    END LOOP;
END $$;
