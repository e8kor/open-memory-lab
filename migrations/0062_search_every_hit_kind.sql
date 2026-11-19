-- no-transaction
-- 0062_search_every_hit_kind — every kind recall may serve as a hit gets
-- postings and counts, a skill is searched by its name, description and
-- body, and the catalog pages a workspace's rows of a kind by one index
-- (ADR-0092 d3, d4, d6, d9; d12 reading 1).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "search
-- postings (ADR-0052)".
--
-- Codes. The postings and counts store a version's kind as a small code:
-- 1 memory, 2 derived_fact, 3 document, 5 skill, 6 guardrail, 7 term,
-- 8 term_mapping, 9 vocab_release; 4 is left unused. Steering has none: it
-- is never ranked (ADR-0092 d2), so it holds no postings, as before. A test
-- has none either: a test case is never a hit and never counts in the
-- statistics (ADR-0086 d1, which ADR-0092 does not amend). Every other
-- kind has postings in every state a memory has; which kinds a recall
-- counts and reads is the recall's choice (d4), so the vocabulary kinds
-- have postings before ADR-0021 d9's opt-in reads them (d12 reading 2).
--
-- A skill's lexemes are its name, its description and its search text or
-- claim, in that order, and the fold runs again when the name or the
-- description changes (d12 reading 1).
--
-- What it costs a live database, stated. The file is one DO statement run
-- outside a migration transaction, `-- no-transaction`, and it commits after
-- each step, so no lock outlives its own step; each waits at most 5 s for
-- its lock, and a retry resumes where a failure left off (every step is
-- skipped when already done; a refill run twice writes what it wrote once).
--
-- 1. The kind functions and the vector fold are replaced in place, the
--    two triggers recreated with the skill columns, and the postings' kind
--    check replaced NOT VALID: catalog changes under one short ACCESS
--    EXCLUSIVE lock on memories and on search_postings.
-- 2. memories_catalog, (tenant_id, workspace_id, kind, created_at, id), is
--    created ON ONLY the parent, then built on each partition in name order
--    and attached, each build committed alone, as 0041 builds its index: a
--    partition's build holds SHARE, so reads never wait and that tenant's
--    writes wait for its own build alone.
-- 3. The kind check is validated under SHARE UPDATE EXCLUSIVE, which lets
--    reads and writes run while it reads the postings.
-- 4. Each tenant holding a row of a kind that had no postings before
--    (skill, guardrail and the vocabulary kinds) has its skills'
--    lexemes recomputed and its postings and counts refilled under its row
--    security, alone in its own transaction; a tenant with none is read by
--    one probe and left as it is. A refill rewrites every posting and count
--    of its tenant, not only the new kinds': measured at about 4.5 s for a
--    tenant of 20,000 versions, so about 225 s per million versions, during
--    which that tenant's writes wait on the rows it rewrites.

DO $mig$
DECLARE
    part regclass;
    name text;
    t uuid;
BEGIN
    SET LOCAL lock_timeout = '5s';

    CREATE OR REPLACE FUNCTION oml_search_state(status text, is_tip boolean, retirement boolean,
                                                kind text)
        RETURNS smallint
        LANGUAGE sql IMMUTABLE PARALLEL SAFE
        RETURN CASE
            WHEN retirement OR kind IS NULL
                 OR kind NOT IN ('memory', 'derived_fact', 'document', 'skill',
                                 'guardrail', 'term', 'term_mapping', 'vocab_release')
                THEN NULL
            WHEN status = 'held' THEN 0
            WHEN status = 'promoted' AND is_tip THEN 1
            WHEN status IN ('promoted', 'demoted', 'rejected', 'withdrawn') THEN 2
            ELSE NULL
        END;

    CREATE OR REPLACE FUNCTION oml_search_memory_kind(kind text) RETURNS smallint
        LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
        RETURN CASE kind
            WHEN 'memory' THEN 1 WHEN 'derived_fact' THEN 2 WHEN 'document' THEN 3
            WHEN 'skill' THEN 5 WHEN 'guardrail' THEN 6
            WHEN 'term' THEN 7 WHEN 'term_mapping' THEN 8 WHEN 'vocab_release' THEN 9
        END;

    -- The lexemes of a row are those of its search text, or of its claim
    -- when it has none; a skill's are its name's and description's first.
    CREATE OR REPLACE FUNCTION oml_search_vector_fold() RETURNS trigger
        LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $fold$
    BEGIN
        NEW.search_vector := oml_search_vector(
            CASE WHEN NEW.kind = 'skill'
                 THEN concat_ws(' ', NEW.skill_name, NEW.skill_description,
                                coalesce(NEW.search_text, NEW.claim))
                 ELSE coalesce(NEW.search_text, NEW.claim)
            END);
        NEW.search_len := length(NEW.search_vector);
        RETURN NEW;
    END
    $fold$;

    DROP TRIGGER IF EXISTS memories_search_vector ON memories;
    CREATE TRIGGER memories_search_vector
        BEFORE INSERT OR UPDATE OF claim, search_text, search_vector, search_len, skill_name,
                                   skill_description
        ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vector_fold();

    -- An UPDATE naming only the skill columns changes the stored lexemes
    -- through the fold above, which an AFTER trigger's column list does not
    -- see, so the postings trigger names them too.
    DROP TRIGGER IF EXISTS memories_search_postings ON memories;
    CREATE TRIGGER memories_search_postings
        AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, search_text, search_vector,
                                  workspace_id, kind, skill_name, skill_description
              OR DELETE
        ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_postings_fold();

    ALTER TABLE search_postings DROP CONSTRAINT IF EXISTS search_postings_memory_kind;
    ALTER TABLE search_postings
        ADD CONSTRAINT search_postings_memory_kind
        CHECK (memory_kind IN (1, 2, 3, 5, 6, 7, 8, 9)) NOT VALID;
    COMMIT;

    SET LOCAL lock_timeout = '5s';
    CREATE INDEX IF NOT EXISTS memories_catalog
        ON ONLY memories (tenant_id, workspace_id, kind, created_at, id);
    COMMIT;
    FOR part IN SELECT inhrelid::regclass FROM pg_inherits
                 WHERE inhparent = 'memories'::regclass
                 ORDER BY inhrelid::regclass::text
    LOOP
        SET LOCAL lock_timeout = '5s';
        -- A partition created after the parent index exists has its copy
        -- already, and a retry finds the ones it attached.
        CONTINUE WHEN EXISTS (
            SELECT 1 FROM pg_inherits i JOIN pg_index x ON x.indexrelid = i.inhrelid
             WHERE i.inhparent = 'memories_catalog'::regclass AND x.indrelid = part);
        name := replace(part::text, '"', '') || '_catalog';
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %I ON %s (tenant_id, workspace_id, kind, created_at, id)',
            name, part);
        EXECUTE format('ALTER INDEX memories_catalog ATTACH PARTITION %I', name);
        COMMIT;
    END LOOP;

    SET LOCAL lock_timeout = '5s';
    ALTER TABLE search_postings VALIDATE CONSTRAINT search_postings_memory_kind;
    COMMIT;

    -- The tenant list is read under 0003's startup policy: tenants forces
    -- row security, so an owner with neither SUPERUSER nor BYPASSRLS would
    -- read no tenant without it. The loop's first COMMIT materializes the
    -- list, so the setting need only hold for its first fetch.
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        SET LOCAL lock_timeout = '5s';
        -- The tenants are enumerated under 0003's startup policy, and each
        -- is read and refilled under its own row security, as 0021's fill.
        PERFORM set_config('oml.startup_check', 'on', true);
        PERFORM set_config('oml.tenant_id', t::text, true);
        IF EXISTS (SELECT 1 FROM memories m
                    WHERE m.tenant_id = t
                      AND m.kind IN ('skill', 'guardrail', 'term', 'term_mapping',
                                     'vocab_release')) THEN
            UPDATE memories m SET search_vector = NULL
             WHERE m.tenant_id = t AND m.kind = 'skill';
            PERFORM oml_search_postings_refill(t);
        END IF;
        COMMIT;
    END LOOP;
END $mig$;
