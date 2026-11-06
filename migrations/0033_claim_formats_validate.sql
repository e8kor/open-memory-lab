-- no-transaction
-- Validates 0032's three checks outside the migration transaction (ADR-0056
-- d4; review of #90, H2). Each statement is its own transaction:
-- VALIDATE CONSTRAINT takes SHARE UPDATE EXCLUSIVE, which lets reads and
-- writes run while it scans, where inside 0032's transaction the scans would
-- have run under the ACCESS EXCLUSIVE lock its ADD COLUMN took. A lock is
-- waited for at most 5 s, then the migration fails and is run again.

SET lock_timeout = '5s';

ALTER TABLE memories VALIDATE CONSTRAINT memories_format_shape;
ALTER TABLE memories VALIDATE CONSTRAINT memories_title_line;
ALTER TABLE memories VALIDATE CONSTRAINT memories_search_text_by;

RESET lock_timeout;
