-- Valid time on memories (proposed ADR-0058 d2, d3, d7, d11).
--
-- A version may declare when it holds: valid_from and valid_until, each the
-- signed text the author sent (a period such as 2015 or 2015-03, a day, or
-- the fixed-width instant), and each bound in microseconds since the epoch,
-- valid_from_us the period's first instant and valid_until_us the instant
-- after its last. Every filter and index reads the integers: int8
-- comparisons are leakproof (pg_proc.proleakproof for int8lt, int8gt,
-- int8le and int8ge on PostgreSQL 18, checked 2026-11-05), so an index on
-- them serves the serving role under row security.
--
-- An existing row is open on both sides, so nothing is rewritten. 0025 is
-- ADR-0057's, on its branch; if another number lands first this file is
-- renumbered at merge.
ALTER TABLE memories
    ADD COLUMN valid_from text,
    ADD COLUMN valid_until text,
    ADD COLUMN valid_from_us int8,
    ADD COLUMN valid_until_us int8,
    ADD CONSTRAINT memories_valid_time_ends CHECK (
        (valid_from IS NULL) = (valid_from_us IS NULL)
        AND (valid_until IS NULL) = (valid_until_us IS NULL)
        AND (valid_from_us IS NULL OR valid_until_us IS NULL
             OR valid_from_us < valid_until_us));

-- The time leg's window probe (ADR-0058 d7) and the expiry probe of
-- bm25-postings (d3): each holds only the rows that declare that end.
CREATE INDEX memories_valid_from ON memories (tenant_id, valid_from_us)
    WHERE valid_from_us IS NOT NULL;
CREATE INDEX memories_valid_until ON memories (tenant_id, valid_until_us)
    WHERE valid_until_us IS NOT NULL;
