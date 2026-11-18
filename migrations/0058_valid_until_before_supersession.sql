-- A demotion restores the end a version had before the supersession it undoes
-- (ADR-0089 d16, note of 2026-11-18; ADR-0059 d4; #454).
--
-- A ratified supersession whose successor declares a start ends the version
-- it replaces there (ADR-0059 d4): supersede_tip writes valid_until. A
-- demotion that returns that version as the tip must give it back the end it
-- had before, or recall would hide the restored tip as expired. The end it
-- had is kept here when the ratification replaces it:
--
--   valid_until_ratified        true while valid_until is the end a
--                               ratification gave the version;
--   valid_until_before(_us)     the end it had before, NULL for an open end.
--
-- The restore writes valid_until back from them and clears all three. A
-- version a ratification ended before this migration carries false, so a
-- demotion restoring it keeps the end it has, as before.
--
-- What it costs a live database: three columns with constant defaults, a
-- catalog change on the parent and each partition; no row is rewritten. A
-- lock on memories is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories
    ADD COLUMN valid_until_ratified boolean NOT NULL DEFAULT false,
    ADD COLUMN valid_until_before text,
    ADD COLUMN valid_until_before_us int8;

COMMENT ON COLUMN memories.valid_until_ratified IS
    'Whether valid_until is the end a ratified supersession gave the version (ADR-0059 d4); a '
    'demotion that restores it as the tip writes valid_until_before back (ADR-0089 d16).';
COMMENT ON COLUMN memories.valid_until_before IS
    'The end the version had before a ratified supersession replaced it; NULL for an open end. '
    'Read only while valid_until_ratified is true (0058).';
COMMENT ON COLUMN memories.valid_until_before_us IS
    'valid_until_before as Unix microseconds, as valid_until_us is valid_until (0058).';
