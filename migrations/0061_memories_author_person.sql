-- no-transaction
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
-- What it costs a live database, stated. The file is one DO statement run
-- outside a migration transaction, `-- no-transaction`, and it commits after
-- each step, so no lock outlives its own step:
--
-- 1. One nullable column with no default: a catalog change on the parent and
--    each partition under one short ACCESS EXCLUSIVE lock on memories,
--    committed before any row is read.
-- 2. One UPDATE of every version that has a person, per tenant under its row
--    security as 0020 enumerates them, each tenant committed alone: a
--    tenant's UPDATE holds ROW EXCLUSIVE on its own partition, so reads never
--    wait and writes wait only for the rows it touches.
--
-- The column is in no trigger's column list, so neither the postings, the
-- vocabulary nor the similar keys move. Each step waits at most 5 s for its
-- lock, then the migration fails and is run again: the column is added only
-- when missing and a backfill run twice writes what it wrote once, so a
-- retry resumes where the failure left off.

DO $$
DECLARE
    t uuid;
BEGIN
    SET LOCAL lock_timeout = '5s';
    ALTER TABLE memories ADD COLUMN IF NOT EXISTS author_person text;
    COMMENT ON COLUMN memories.author_person IS
        'The person the author key acted for at the write, by name, as same_user compares it; '
        'NULL for a key that acts for no person (ADR-0085 d16; 0061).';
    COMMIT;

    -- The tenant list is read under 0003's startup policy; the loop's first
    -- COMMIT materializes it, so the setting need only hold for its first
    -- fetch.
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        SET LOCAL lock_timeout = '5s';
        PERFORM set_config('oml.startup_check', 'on', true);
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE memories m SET author_person = p.person
          FROM (SELECT k.id,
                       COALESCE(k.session_for,
                                (SELECT u.name FROM workspaces u
                                  WHERE u.tenant_id = t AND u.user_root
                                    AND u.id = COALESCE(k.user_root_id, k.linked_user_id)))
                           AS person
                  FROM api_keys k WHERE k.tenant_id = t) p
         WHERE m.tenant_id = t AND m.author_key_id = p.id AND p.person IS NOT NULL
           AND m.author_person IS DISTINCT FROM p.person;
        COMMIT;
    END LOOP;
END
$$;
