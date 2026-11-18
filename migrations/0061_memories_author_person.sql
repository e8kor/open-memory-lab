-- A version records its author's person at its write (ADR-0085 d16, amended
-- 2026-11-18; #448).
--
-- same_user, R2 by person and a person's withdrawal read the person a
-- version's author key acts for. Until now they read it through the key as it
-- stands at the verdict, so an unlink between the write and the verdict
-- cleared the mark and lifted the refusal. author_person holds that person by
-- name, the way same_user compares it (oml_domain::same_user): a session
-- key's session_for, else the name of the user root its user key names or its
-- linked tenant key is linked to; NULL for a key that acts for no person. The
-- write sets it from the author key at the write; nothing changes it after.
--
-- Backfill: every version written before this migration takes the person its
-- author key resolves to when the migration runs, read the same way. A key
-- unlinked before then gives its earlier versions no person, so the mark
-- those versions would have carried at their write is not recovered; the
-- ADR states it.
--
-- What it costs a live database: one nullable column with no default (a
-- catalog change on the parent and each partition), then one UPDATE of every
-- version that has a person, per tenant under its row security as 0020
-- enumerates them. The column is in no trigger's column list, so neither the
-- postings, the vocabulary nor the similar keys move. A lock on memories is
-- waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories ADD COLUMN author_person text;

COMMENT ON COLUMN memories.author_person IS
    'The person the author key acted for at the write, by name, as same_user compares it; '
    'NULL for a key that acts for no person (ADR-0085 d16; 0061).';

DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE memories m SET author_person = p.person
          FROM (SELECT k.id,
                       COALESCE(k.session_for,
                                (SELECT u.name FROM workspaces u
                                  WHERE u.tenant_id = t AND u.user_root
                                    AND u.id = COALESCE(k.user_root_id, k.linked_user_id)))
                           AS person
                  FROM api_keys k WHERE k.tenant_id = t) p
         WHERE m.tenant_id = t AND m.author_key_id = p.id AND p.person IS NOT NULL;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;
