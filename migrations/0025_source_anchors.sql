-- Source anchors and source checks (ADR-0057 d1-d5, d18).
--
-- A version's anchor is the origin its author declares: signed in its
-- submission event, immutable per version, clear metadata that erasure
-- never removes (d2). Exactly one of anchor_repo and anchor_uri when
-- anchored; the grammar is the application's (d3), and these checks keep
-- the shape a row may take.
--
-- The latest source check of a promoted version is folded onto its row by
-- `source.checked` (d5): `stale` is computed from it alone, so a served
-- version is never marked from a held one. The journal keeps every check.
--
-- Safe on a live database: a lock is waited for at most 5 s, then the
-- migration fails and is run again in a quieter moment. Adding nullable
-- columns is a catalog change. The checks are added NOT VALID, which takes
-- no scan under the exclusive lock, then validated under SHARE UPDATE
-- EXCLUSIVE, which lets reads and writes run. The indexes are partial, so
-- empty at creation: each partition's is built on its own, then attached to
-- the parent's, so no write waits on more than one partition at a time.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories
    ADD COLUMN anchor_repo     text,
    ADD COLUMN anchor_uri      text,
    ADD COLUMN anchor_rev      text,
    ADD COLUMN anchor_path     text,
    ADD COLUMN anchor_fragment text,
    ADD COLUMN anchor_hash     text,
    ADD COLUMN anchor_hasher   text,
    ADD COLUMN source_check_rev      text,
    ADD COLUMN source_check_result   text,
    ADD COLUMN source_check_hash     text,
    ADD COLUMN source_check_path     text,
    ADD COLUMN source_check_fragment text,
    ADD COLUMN source_check_at       text;

ALTER TABLE memories
    ADD CONSTRAINT memories_anchor_origin CHECK (
        (anchor_repo IS NULL OR anchor_uri IS NULL)
        AND (anchor_repo IS NULL) = (anchor_path IS NULL)
        AND (anchor_repo IS NOT NULL OR anchor_uri IS NOT NULL
             OR (anchor_rev IS NULL AND anchor_fragment IS NULL AND anchor_hash IS NULL))) NOT VALID,
    ADD CONSTRAINT memories_anchor_hash CHECK ((anchor_hash IS NULL) = (anchor_hasher IS NULL)) NOT VALID,
    ADD CONSTRAINT memories_source_check CHECK (
        (source_check_result IS NULL) = (source_check_rev IS NULL)
        AND (source_check_result IS NULL) = (source_check_at IS NULL)
        AND (source_check_result IS NULL
             OR source_check_result IN ('fresh', 'equivalent', 'moved', 'changed', 'removed'))
        AND (source_check_path IS NULL OR source_check_result = 'moved')) NOT VALID;

ALTER TABLE memories VALIDATE CONSTRAINT memories_anchor_origin;
ALTER TABLE memories VALIDATE CONSTRAINT memories_anchor_hash;
ALTER TABLE memories VALIDATE CONSTRAINT memories_source_check;

-- The origin filters (d4): a repository by equality and a path prefix as a
-- range under text_pattern_ops, whose ~>=~ and ~<~ are leakproof, so the
-- index serves the serving role under row security. memories_anchor serves
-- a repository with or without a prefix; memories_anchor_path a prefix
-- alone.
CREATE INDEX memories_anchor
    ON ONLY memories (tenant_id, anchor_repo, anchor_path text_pattern_ops)
    WHERE anchor_repo IS NOT NULL;
CREATE INDEX memories_anchor_path
    ON ONLY memories (tenant_id, anchor_path text_pattern_ops)
    WHERE anchor_path IS NOT NULL;

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
            'CREATE INDEX %I ON %s (tenant_id, anchor_repo, anchor_path text_pattern_ops) '
            'WHERE anchor_repo IS NOT NULL', name || '_anchor', part);
        EXECUTE format('ALTER INDEX memories_anchor ATTACH PARTITION %I', name || '_anchor');
        EXECUTE format(
            'CREATE INDEX %I ON %s (tenant_id, anchor_path text_pattern_ops) '
            'WHERE anchor_path IS NOT NULL', name || '_anchor_path', part);
        EXECUTE format('ALTER INDEX memories_anchor_path ATTACH PARTITION %I',
                       name || '_anchor_path');
    END LOOP;
END $$;
