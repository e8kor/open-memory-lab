-- A tenant key linked to a person's sign-in (ADR-0085 d16; ADR-0082 d7).
--
-- An admin links a tenant key used by a person to that person, journalled
-- as key.linked_person, and ends it with key.unlinked_person. The key then
-- acts for the person as a user key does: the same-person rule reads the
-- link (ADR-0082 d7), and the console offers the key for acts in the
-- workspaces it grants. linked_user_id names the person's user root, as
-- user_root_id does on a user key (0014). It is NULL on every key written
-- before this migration and on every key never linked; the check below keeps
-- it off session keys and user keys, which already name their person.
--
-- No foreign key to workspaces: a user root is never deleted (ADR-0001 R4),
-- and one would take a SHARE ROW EXCLUSIVE lock on workspaces. The store's
-- fold checks the root is a user root of the tenant.
--
-- Numbered 0050, after the claim kinds (0048, 0049).
--
-- What it costs a live database: catalog changes only. The column is
-- nullable with no default and the check is added NOT VALID, so nothing scans
-- api_keys here; 0051 validates it outside this transaction. A lock is waited
-- for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE api_keys ADD COLUMN linked_user_id uuid;

ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_linked_user CHECK (
        linked_user_id IS NULL OR (user_root_id IS NULL AND credential <> 'session')
    ) NOT VALID;
