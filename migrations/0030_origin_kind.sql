-- A memory version's origin.kind (ADR-0063 d1-d3).
--
-- The kind is the author's statement, signed in its submission event and
-- immutable per version. Versions written before this migration have none
-- and are never backfilled: the kind would not be in their signed event
-- (d2). The set is closed; a new kind is a new decision and a new check.
--
-- Numbered 0030, past the 0026 to 0029 the parallel v0.6.0 lanes use.
--
-- Safe on a live database, as 0025 is: a lock is waited for at most 5 s;
-- the nullable column is a catalog change; the check is added NOT VALID
-- and validated under SHARE UPDATE EXCLUSIVE; the partial index is built a
-- partition at a time and attached to the parent's.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories ADD COLUMN origin_kind text;

ALTER TABLE memories
    ADD CONSTRAINT memories_origin_kind CHECK (
        origin_kind IS NULL
        OR origin_kind IN ('repo', 'doc', 'code', 'lake', 'chat', 'web', 'manual')) NOT VALID;
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
