-- no-transaction
-- Validates 0048's checks outside the migration transaction (ADR-0080 d1;
-- ADR-0081 d1; ADR-0086 d1; ADR-0083 d13). Each statement is its own
-- transaction: VALIDATE CONSTRAINT takes SHARE UPDATE EXCLUSIVE, which lets
-- reads and writes run while it scans memories. A lock is waited for at most
-- 5 s, then the migration fails and is run again.

SET lock_timeout = '5s';

ALTER TABLE memories VALIDATE CONSTRAINT memories_kind_fields;

ALTER TABLE memories VALIDATE CONSTRAINT memories_status_check;
