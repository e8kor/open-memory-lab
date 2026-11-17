-- no-transaction
-- Validates 0050's api_keys_linked_user check outside 0050's transaction
-- (ADR-0085 d16). sqlx runs this file as one implicit transaction of its
-- own. VALIDATE CONSTRAINT takes SHARE UPDATE EXCLUSIVE, which lets reads and
-- writes run while it scans api_keys. A lock is waited for at most 5 s, then
-- the migration fails and is run again.

SET lock_timeout = '5s';

ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_linked_user;

RESET lock_timeout;
