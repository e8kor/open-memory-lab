-- The v0.8.4 role rename (ADR-0093 d5): the key role writer becomes
-- contributor, and the grant role writer becomes submitter. The key role
-- submitter, which writes without recall, is new and no stored key holds it.
--
-- One transaction. No applied migration is edited: 0001-0065 keep their text
-- and their sqlx checksums, and this file is the one migration after them that
-- spells the former role, because it names what it renames (ADR-0093 d7).
--
-- What this rewrites was enumerated from pg_catalog on a database migrated
-- through 0065 (ADR-0093 d5):
--
--   role columns   api_keys.role and grants.role, the only columns that hold
--                  a role; their checks api_keys_role_check and
--                  grants_role_check are the only constraints that name it
--   required_roles no stored column holds a review policy outside
--                  journal.payload and outbox.payload, which are signed
--                  events and are read through the former-name map's rule 7
--                  (ADR-0093 d4), never rewritten
--   functions      no function body in public compares a role to the former
--                  value, so none is replaced; the lake writer's own names
--                  are not a role and are not read here
--
-- Every writer key becomes a contributor key, as rule 7 reads its key.issued,
-- so the database rebuilt from the journal equals the migrated one: a key
-- issued with --write-only, which carries no reader grant, becomes a
-- contributor without recall that oml doctor names (ADR-0093 d3).
--
-- What it costs a live database: one UPDATE of api_keys and grants, and the
-- two replaced checks validated in one scan each of those small tables. The
-- upgrade runs it with the server stopped (ADR-0093 d8), so the ACCESS
-- EXCLUSIVE locks it takes are free; a lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE api_keys DROP CONSTRAINT api_keys_role_check;
ALTER TABLE grants DROP CONSTRAINT grants_role_check;

------------------------------------------------------------------------------
-- Stored values, under each tenant's row security, the tenants enumerated
-- under 0003's startup policy as 0047 enumerates them. The startup marker is
-- set before the loop, since the tenants row is read under it.
------------------------------------------------------------------------------
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE api_keys SET role = 'contributor'
         WHERE tenant_id = t AND role = 'writer';
        UPDATE grants SET role = 'submitter'
         WHERE tenant_id = t AND role = 'writer';
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END $$;

------------------------------------------------------------------------------
-- The checks again, in the new values. Every row was rewritten above, so each
-- holds; the tables are small and the server is stopped.
------------------------------------------------------------------------------
-- lint:allow constraint api_keys and grants are small, every row was rewritten in this transaction, and the upgrade runs with the server stopped (ADR-0093 d8), so one scan each under the held lock is the cheaper path than a second migration
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_role_check
        CHECK (role IN ('reader', 'contributor', 'submitter', 'reviewer', 'admin', 'operator'));
ALTER TABLE grants
    ADD CONSTRAINT grants_role_check CHECK (role IN ('reader', 'submitter', 'reviewer'));
