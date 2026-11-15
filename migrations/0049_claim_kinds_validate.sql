-- no-transaction
-- Validates 0048's checks outside the migration transaction 0048 ran in
-- (ADR-0080 d1; ADR-0081 d1; ADR-0086 d1; ADR-0083 d13). sqlx runs this file
-- as one implicit transaction of its own, not each statement alone, so both
-- validations commit together. VALIDATE CONSTRAINT takes SHARE UPDATE
-- EXCLUSIVE, which lets reads and writes run while it scans memories, where
-- inside 0048's transaction the scans would have run under the ACCESS
-- EXCLUSIVE lock its ADD COLUMN took. A lock is waited for at most 5 s, then
-- the migration fails and is run again.

SET lock_timeout = '5s';

ALTER TABLE memories VALIDATE CONSTRAINT memories_kind_fields;

ALTER TABLE memories VALIDATE CONSTRAINT memories_status_check;

RESET lock_timeout;
