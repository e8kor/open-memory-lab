-- A memory version's origin.kind (ADR-0063 d1-d3).
--
-- The kind is the author's statement, signed in its submission event and
-- immutable per version. Versions written before this migration have none
-- and are never backfilled: the kind would not be in their signed event
-- (d2). The set is closed; a new kind is a new decision and a new check.
--
-- Numbered 0029, after valid time (0026), the similar keys (0027) and in progress (0028).
--
-- This file runs in one transaction and holds ACCESS EXCLUSIVE on memories
-- only for catalog changes: the nullable column and the check added NOT
-- VALID, which scan nothing. A lock is waited for at most 5 s. The scan that
-- validates the check, and the index builds, run in 0030 outside any
-- transaction, so they take SHARE UPDATE EXCLUSIVE and SHARE one statement
-- at a time while reads and writes go on.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories ADD COLUMN origin_kind text;

ALTER TABLE memories
    ADD CONSTRAINT memories_origin_kind CHECK (
        origin_kind IS NULL
        OR origin_kind IN ('repo', 'doc', 'code', 'lake', 'chat', 'web', 'manual')) NOT VALID;
