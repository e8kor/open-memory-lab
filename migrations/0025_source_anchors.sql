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
-- The origin filters (d4) read the index below inside the candidate scope,
-- under the same row security as every other read of memories.

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
    ADD COLUMN source_check_at       text,
    ADD CONSTRAINT memories_anchor_origin CHECK (
        (anchor_repo IS NULL OR anchor_uri IS NULL)
        AND (anchor_repo IS NULL) = (anchor_path IS NULL)
        AND (anchor_repo IS NOT NULL OR anchor_uri IS NOT NULL
             OR (anchor_rev IS NULL AND anchor_fragment IS NULL AND anchor_hash IS NULL))),
    ADD CONSTRAINT memories_anchor_hash CHECK ((anchor_hash IS NULL) = (anchor_hasher IS NULL)),
    ADD CONSTRAINT memories_source_check CHECK (
        (source_check_result IS NULL) = (source_check_rev IS NULL)
        AND (source_check_result IS NULL) = (source_check_at IS NULL)
        AND (source_check_result IS NULL
             OR source_check_result IN ('fresh', 'equivalent', 'moved', 'changed', 'removed'))
        AND (source_check_path IS NULL OR source_check_result = 'moved'));

-- The origin filters: a repository, and a byte prefix of the path within
-- it (text_pattern_ops, so a prefix is a range). A partial index: only
-- anchored rows are in it, so unanchored tenants pay nothing.
CREATE INDEX memories_anchor
    ON memories (tenant_id, anchor_repo, anchor_path text_pattern_ops)
    WHERE anchor_repo IS NOT NULL;
