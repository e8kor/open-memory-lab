-- The rule set a memory version was last scanned under (ADR-0065 d1, d15;
-- ADR-0074 d28).
--
-- Admission sets the server's rule set; `oml admin rescan --record-held`
-- sets it on each held version it reads, journalled as
-- scan.findings_recorded. Every row stored before this migration was scanned
-- under `oml-scan/1`, or by no rule set at all, so it reads `oml-scan/1`:
-- inside an in-progress scope another author's held version is served only
-- when `scanned_under` is the server's rule set, so a held version written
-- before the upgrade stays its author's until the recording rescan reads it.
--
-- Numbered 0038, after open sign-up (0037).
--
-- The column is added with a constant default, which PostgreSQL 11 and later
-- keep in the catalog and serve for every existing row without rewriting the
-- table; dropping the default afterwards leaves those rows' value and makes a
-- row inserted without one NULL, as a scanner that pins no rule set leaves
-- it. Catalog changes only; a lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories ADD COLUMN scanned_under text DEFAULT 'oml-scan/1';
ALTER TABLE memories ALTER COLUMN scanned_under DROP DEFAULT;
