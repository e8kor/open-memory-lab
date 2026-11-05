-- no-transaction
-- Validates 0030's origin_kind check and builds its index outside the
-- migration transaction (ADR-0063 d2; review L1). Each statement is its own
-- transaction: VALIDATE CONSTRAINT takes SHARE UPDATE EXCLUSIVE, which lets
-- reads and writes run while it scans. The DO block builds and attaches
-- every partition's index in one statement, under SHARE on each partition
-- until it ends; each index is empty, since no row written before 0030 has
-- a kind, so that statement does catalog work and little else. A lock is
-- waited for at most 5 s, then the migration fails and is run again.

SET lock_timeout = '5s';

ALTER TABLE memories VALIDATE CONSTRAINT memories_origin_kind;


-- The kind filter (d3) reads a tenant's rows of a kind; `manual`, the
-- default, is the common value and is left out, so the index stays the
-- size of the rows that name a source.
CREATE INDEX memories_origin_kind
    ON ONLY memories (tenant_id, origin_kind)
    WHERE origin_kind IS NOT NULL AND origin_kind <> 'manual';

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
            'CREATE INDEX %I ON %s (tenant_id, origin_kind) '
            'WHERE origin_kind IS NOT NULL AND origin_kind <> ''manual''',
            name || '_origin_kind', part);
        EXECUTE format('ALTER INDEX memories_origin_kind ATTACH PARTITION %I',
                       name || '_origin_kind');
    END LOOP;
END $$;

RESET lock_timeout;
