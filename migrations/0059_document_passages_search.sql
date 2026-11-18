-- 0059_document_passages_search — a document's passages are its search
-- units (ADR-0081 d3, d4; the build's reading R6): each passage carries its
-- own search text, lexemes, length and served byte count; the postings carry
-- the passage's ordinal; the fold writes a document version's postings and
-- counts one unit per passage; and `document` joins the kinds the postings,
-- the counts and the vocabulary hold (ADR-0092 d4, search units).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "search
-- postings (ADR-0052)" and "documents (ADR-0081)".
--
-- A search unit is a (version, ordinal) pair: ordinal -1 is a whole
-- version, which every row of every other kind is, and a document version
-- whose text holds no passage (R1 can leave none) or whose passages are not
-- cut yet; ordinal 0 and above is one passage of a document version. A
-- version with no document is one unit with ordinal -1, so its postings,
-- counts and every score read from them are what they were before this
-- migration.
--
-- Numbered 0059, reserved for this change in docs/NUMBERS.md.
--
-- What it costs a live database (ADR-0081 R14): no lock this file takes
-- blocks a read of memories or search_postings for longer than a catalog
-- change. document_passages is empty (nothing wrote it before this change),
-- so its new NOT NULL columns cost a catalog change. search_postings gains
-- a column with a constant default, which rewrites nothing, and its
-- memory_kind check is replaced NOT VALID, which reads no posting; 0060
-- validates it. search_workspace_rows, one row per tenant, workspace and
-- kind, gains its units count, set from its rows. The fold's, the refill's
-- and the vocabulary's functions are replaced in place and their triggers
-- kept. Nothing is refilled here: 0060 rebuilds the served index to carry
-- the ordinal, partition by partition, and refills the tenants that hold a
-- document row, each committed alone, so a document version written before
-- this change is searched by no list until the lead cuts its passages at
-- startup, before it serves. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

------------------------------------------------------------------------------
-- document_passages — each passage's search text (the build's reading R2:
-- the title, then its heading path joined by ` > `, then its lines), its
-- lexemes and their count, computed once at write by the trigger below as
-- memories' are (0020), and the byte count of the text a hit on it serves
-- (ADR-0081 d4: `body_bytes` and the budget are the passage's).
------------------------------------------------------------------------------
-- lint:allow column-check document_passages is empty before this migration (nothing wrote it before #405), so the inline check validates no row
ALTER TABLE document_passages
    ADD COLUMN body_bytes    integer  NOT NULL CHECK (body_bytes >= 0),
    ADD COLUMN search_text   text     NOT NULL,
    ADD COLUMN search_vector tsvector NOT NULL,
    ADD COLUMN search_len    integer  NOT NULL;

COMMENT ON COLUMN document_passages.body_bytes IS
    'The UTF-8 bytes of the passage''s lines as a hit serves them (ADR-0081 d4). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN document_passages.search_text IS
    'The title, the heading path joined by > and the passage''s lines (ADR-0081, reading R2). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN document_passages.search_vector IS
    'oml_search_vector(search_text), set at write by document_passages_search_vector '
    '(ADR-0049 d12). Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN document_passages.search_len IS
    'The count of distinct lexemes in search_vector, BM25''s unit length (ADR-0049 d12). '
    'Owning page: docs/data/postgres-schema.md.';

CREATE FUNCTION oml_passage_search_vector_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    NEW.search_vector := oml_search_vector(NEW.search_text);
    NEW.search_len := length(NEW.search_vector);
    RETURN NEW;
END
$$;

CREATE TRIGGER document_passages_search_vector
    BEFORE INSERT OR UPDATE OF search_text, search_vector, search_len
    ON document_passages FOR EACH ROW EXECUTE FUNCTION oml_passage_search_vector_fold();

------------------------------------------------------------------------------
-- The kinds the postings and the counts hold: `document` is code 3 and has
-- postings in every state a memory has (ADR-0092 d4).
------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION oml_search_state(status text, is_tip boolean, retirement boolean, kind text)
    RETURNS smallint
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    RETURN CASE
        WHEN retirement OR kind IS NULL OR kind NOT IN ('memory', 'derived_fact', 'document')
            THEN NULL
        WHEN status = 'held' THEN 0
        WHEN status = 'promoted' AND is_tip THEN 1
        WHEN status IN ('promoted', 'demoted', 'rejected', 'withdrawn') THEN 2
        ELSE NULL
    END;

CREATE OR REPLACE FUNCTION oml_search_memory_kind(kind text) RETURNS smallint
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN CASE kind WHEN 'memory' THEN 1 WHEN 'derived_fact' THEN 2 WHEN 'document' THEN 3 END;

-- The widened check is added NOT VALID, so no posting is read under this
-- transaction's lock; 0060 validates it under SHARE UPDATE EXCLUSIVE.
ALTER TABLE search_postings DROP CONSTRAINT search_postings_memory_kind;
ALTER TABLE search_postings
    ADD CONSTRAINT search_postings_memory_kind CHECK (memory_kind IN (1, 2, 3)) NOT VALID;

------------------------------------------------------------------------------
-- search_postings.ordinal — the unit a posting belongs to: -1 the whole
-- version, else the passage's ordinal. 0060 rebuilds the served index to
-- carry it, so a recall still reads its postings by an index-only scan.
------------------------------------------------------------------------------
ALTER TABLE search_postings ADD COLUMN ordinal integer NOT NULL DEFAULT -1;

COMMENT ON COLUMN search_postings.ordinal IS
    'The search unit: -1 the whole version, else the document passage''s ordinal (ADR-0081 d4). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- search_workspace_rows.units — the search units of the served versions a
-- row counts (ADR-0081 d4; the review's reading R6 amended): `rows` counts
-- versions, which BM25's N and every document frequency count, and `units`
-- the units whose lengths `total_len` sums, so the mean length stays per
-- unit. A version of any kind but `document` is one unit, so every row
-- written before this change has as many units as rows.
------------------------------------------------------------------------------
ALTER TABLE search_workspace_rows ADD COLUMN units bigint NOT NULL DEFAULT 0;
UPDATE search_workspace_rows SET units = rows;

COMMENT ON COLUMN search_workspace_rows.units IS
    'The search units of the counted versions, whose lengths total_len sums: a document '
    'version''s passages, one for any other version (ADR-0081 d4, R6 amended). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- oml_search_units — a version's search units: a document version's
-- passages, each with its own lexemes and length; a version of any other
-- kind is itself, ordinal -1, with the lexemes and length given. A document
-- version with no passage (not cut yet, or a text with no content line,
-- R1) has no unit, as the scans read none for it. Read by the fold, the
-- refill and the health check, so the three agree on what a unit is.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_units(t uuid, id uuid, kind text, sv tsvector, len integer)
    RETURNS TABLE (ordinal integer, search_vector tsvector, search_len integer)
    LANGUAGE sql STABLE SECURITY INVOKER SET search_path = public, pg_temp AS $$
    SELECT d.ordinal, d.search_vector, d.search_len FROM document_passages d
     WHERE kind = 'document' AND d.tenant_id = t AND d.memory_id = id
    UNION ALL
    SELECT -1, sv, len
     WHERE kind IS DISTINCT FROM 'document'
$$;

------------------------------------------------------------------------------
-- oml_search_count — 0021's count, which also keeps `units`: one version
-- of one unit, its lexemes `sv` and length `len`, added (`delta` 1) or
-- taken back (-1). Every caller but the fold below counts such a version.
------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION oml_search_count(t uuid, wa uuid, mk smallint, sv tsvector, len integer,
                                            delta integer) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    PERFORM oml_search_count_version(
        t, wa, mk, ARRAY(SELECT DISTINCT oml_token_id(l) FROM unnest(tsvector_to_array(sv)) l),
        len, 1, delta);
END
$$;

------------------------------------------------------------------------------
-- oml_search_count_version — one version's counts (ADR-0081 d4; the
-- review's reading R6 amended): its distinct tokens `ids` over all its
-- units each add or take back one row, so a document adds at most 1 to a
-- token's document frequency however many passages hold it; the version
-- adds one to `rows`, its `n_units` to `units` and their summed length `len`
-- to `total_len`.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_count_version(t uuid, wa uuid, mk smallint, ids bigint[], len bigint,
                                         n_units integer, delta integer) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF delta > 0 THEN
        INSERT INTO search_term_rows (tenant_id, workspace_id, kind, memory_kind, token_id, rows)
        SELECT t, wa, 1, mk, x.id, 1 FROM unnest(ids) x(id)
         ORDER BY x.id
        ON CONFLICT (tenant_id, workspace_id, kind, memory_kind, token_id)
        DO UPDATE SET rows = search_term_rows.rows + 1;
        INSERT INTO search_workspace_rows (tenant_id, workspace_id, kind, memory_kind, rows,
                                           total_len, units)
        VALUES (t, wa, 1, mk, 1, len, n_units)
        ON CONFLICT (tenant_id, workspace_id, kind, memory_kind)
        DO UPDATE SET rows = search_workspace_rows.rows + 1,
                      total_len = search_workspace_rows.total_len + EXCLUDED.total_len,
                      units = search_workspace_rows.units + EXCLUDED.units;
    ELSE
        PERFORM 1 FROM search_term_rows c
         WHERE c.tenant_id = t AND c.workspace_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.token_id = ANY (ids)
         ORDER BY c.token_id
           FOR UPDATE;
        UPDATE search_term_rows c SET rows = c.rows - 1
         WHERE c.tenant_id = t AND c.workspace_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.token_id = ANY (ids) AND c.rows > 0;
        DELETE FROM search_term_rows c
         WHERE c.tenant_id = t AND c.workspace_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.token_id = ANY (ids) AND c.rows = 0;
        UPDATE search_workspace_rows c
           SET rows = c.rows - 1, total_len = c.total_len - len, units = c.units - n_units
         WHERE c.tenant_id = t AND c.workspace_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.rows > 0;
        DELETE FROM search_workspace_rows c
         WHERE c.tenant_id = t AND c.workspace_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.rows = 0;
    END IF;
END
$$;
REVOKE EXECUTE ON FUNCTION oml_search_count_version(uuid, uuid, smallint, bigint[], bigint,
                                                    integer, integer) FROM PUBLIC;

------------------------------------------------------------------------------
-- oml_search_count_units — counts version `id` of kind `kind` through its
-- units: its tokens across them once, their count and their lengths.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_count_units(t uuid, wa uuid, id uuid, kind text, sv tsvector,
                                       len integer, delta integer) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    n integer;
    total bigint;
    ids bigint[];
BEGIN
    SELECT count(*), COALESCE(sum(u.search_len), 0) INTO n, total
      FROM oml_search_units(t, id, kind, sv, len) u;
    -- A version with no unit is counted nowhere, as no scan reads it.
    IF n = 0 THEN
        RETURN;
    END IF;
    ids := ARRAY(SELECT DISTINCT oml_token_id(l)
                   FROM oml_search_units(t, id, kind, sv, len) u
                  CROSS JOIN LATERAL unnest(tsvector_to_array(u.search_vector)) l);
    PERFORM oml_search_count_version(t, wa, oml_search_memory_kind(kind), ids, total, n, delta);
END
$$;
REVOKE EXECUTE ON FUNCTION oml_search_count_units(uuid, uuid, uuid, text, tsvector, integer,
                                                  integer) FROM PUBLIC;

------------------------------------------------------------------------------
-- oml_search_postings_fold — 0032's fold with every posting taken per unit
-- and every count per version: a document version's postings are its
-- passages', each carrying its ordinal, and its counts add or take back
-- one row, its tokens across its passages once, and its units. A
-- version's passages are written before its row (oml-pg insert_memory), so
-- the insert's fold finds them; they never change after. An erased or
-- deleted version's passages go after its counts are taken back, with the
-- body they were cut from (ADR-0081 d3). The trigger is 0032's, which
-- fires on status, so an erasure reaches the passages' delete.
------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION oml_search_postings_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    old_state smallint;
    new_state smallint;
    moved boolean := false;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        old_state := oml_search_state(OLD.status, OLD.is_tip, OLD.retirement, OLD.kind);
    END IF;
    IF TG_OP <> 'DELETE' THEN
        new_state := oml_search_state(NEW.status, NEW.is_tip, NEW.retirement, NEW.kind);
    END IF;
    IF TG_OP = 'UPDATE' THEN
        moved := OLD.search_vector IS DISTINCT FROM NEW.search_vector
              OR OLD.claim IS DISTINCT FROM NEW.claim
              OR OLD.search_text IS DISTINCT FROM NEW.search_text
              OR OLD.workspace_id <> NEW.workspace_id OR OLD.kind IS DISTINCT FROM NEW.kind;
        IF NOT moved AND old_state IS NOT DISTINCT FROM new_state
           AND NEW.status IS DISTINCT FROM 'erased' THEN
            RETURN NULL;
        END IF;
    END IF;
    IF old_state IS NOT NULL AND (new_state IS NULL OR moved) THEN
        IF old_state = 1 THEN
            PERFORM oml_search_count_units(OLD.tenant_id, OLD.workspace_id, OLD.id, OLD.kind,
                                           OLD.search_vector, OLD.search_len, -1);
        END IF;
        DELETE FROM search_postings p WHERE p.tenant_id = OLD.tenant_id AND p.memory_id = OLD.id;
        DELETE FROM search_sparse_queue q WHERE q.tenant_id = OLD.tenant_id AND q.memory_id = OLD.id;
        old_state := NULL;
    END IF;
    -- The passages leave with the body they were cut from.
    IF TG_OP = 'DELETE' OR NEW.status = 'erased' THEN
        DELETE FROM document_passages d
         WHERE d.tenant_id = OLD.tenant_id AND d.memory_id = OLD.id;
    END IF;
    IF new_state IS NULL THEN
        RETURN NULL;
    END IF;
    IF old_state IS NULL THEN
        INSERT INTO search_postings (tenant_id, memory_id, workspace_id, token_id, weight, doc_len,
                                     kind, memory_kind, state, ordinal)
        SELECT NEW.tenant_id, NEW.id, NEW.workspace_id, oml_token_id(x.lexeme),
               GREATEST(cardinality(x.positions), 1), u.search_len, 1,
               oml_search_memory_kind(NEW.kind), new_state, u.ordinal
          FROM oml_search_units(NEW.tenant_id, NEW.id, NEW.kind, NEW.search_vector,
                                NEW.search_len) u
         CROSS JOIN LATERAL unnest(u.search_vector) x;
        INSERT INTO search_sparse_queue (tenant_id, memory_id)
        SELECT NEW.tenant_id, NEW.id
         WHERE EXISTS (SELECT 1 FROM search_sparse_state s
                        WHERE s.tenant_id = NEW.tenant_id AND s.enabled)
        ON CONFLICT (tenant_id, memory_id)
        DO UPDATE SET enqueued_at = clock_timestamp(), attempts = 0, next_at = clock_timestamp(),
                      last_error = NULL, parked_at = NULL;
    ELSE
        UPDATE search_postings p SET state = new_state
         WHERE p.tenant_id = NEW.tenant_id AND p.memory_id = NEW.id;
        IF old_state = 1 THEN
            PERFORM oml_search_count_units(OLD.tenant_id, OLD.workspace_id, OLD.id, OLD.kind,
                                           OLD.search_vector, OLD.search_len, -1);
        END IF;
    END IF;
    IF new_state = 1 THEN
        PERFORM oml_search_count_units(NEW.tenant_id, NEW.workspace_id, NEW.id, NEW.kind,
                                       NEW.search_vector, NEW.search_len, 1);
    END IF;
    RETURN NULL;
END
$$;

------------------------------------------------------------------------------
-- oml_search_postings_refill — 0022's refill with the kind 1 postings taken
-- per unit and the counts per version: a token's row count is the versions
-- holding it in any unit, and a workspace's rows, units and length are its
-- served versions', their units' and those units' lengths. Kind 2 is
-- refilled as 0022 refills it; the sparse encoder writes a document's
-- passages' kind 2 postings itself.
------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION oml_search_postings_refill(t uuid) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF t IS DISTINCT FROM NULLIF(current_setting('oml.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'oml_search_postings_refill(%) runs with oml.tenant_id set to that tenant', t;
    END IF;
    PERFORM pg_advisory_xact_lock(
        ('x' || encode(substr(sha256(uuid_send(t)), 1, 8), 'hex'))::bit(64)::bigint);
    DELETE FROM search_postings p WHERE p.tenant_id = t AND p.kind = 1;
    DELETE FROM search_term_rows c WHERE c.tenant_id = t;
    DELETE FROM search_workspace_rows c WHERE c.tenant_id = t;
    INSERT INTO search_postings (tenant_id, memory_id, workspace_id, token_id, weight, doc_len,
                                 kind, memory_kind, state, ordinal)
    SELECT m.tenant_id, m.id, m.workspace_id, oml_token_id(x.lexeme),
           GREATEST(cardinality(x.positions), 1), u.search_len, 1,
           oml_search_memory_kind(m.kind),
           oml_search_state(m.status, m.is_tip, m.retirement, m.kind), u.ordinal
      FROM memories m
     CROSS JOIN LATERAL oml_search_units(m.tenant_id, m.id, m.kind, m.search_vector,
                                         m.search_len) u
     CROSS JOIN LATERAL unnest(u.search_vector) x
     WHERE m.tenant_id = t
       AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind) IS NOT NULL;
    DELETE FROM search_postings p
     WHERE p.tenant_id = t AND p.kind = 2
       AND NOT EXISTS (SELECT 1 FROM memories m
                        WHERE m.tenant_id = t AND m.id = p.memory_id
                          AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind)
                              IS NOT NULL);
    UPDATE search_postings p
       SET state = oml_search_state(m.status, m.is_tip, m.retirement, m.kind)
      FROM memories m
     WHERE p.tenant_id = t AND p.kind = 2 AND m.tenant_id = t AND m.id = p.memory_id
       AND p.state IS DISTINCT FROM oml_search_state(m.status, m.is_tip, m.retirement, m.kind);
    INSERT INTO search_sparse_queue (tenant_id, memory_id)
    SELECT m.tenant_id, m.id FROM memories m
     WHERE m.tenant_id = t
       AND EXISTS (SELECT 1 FROM search_sparse_state s WHERE s.tenant_id = t AND s.enabled)
       AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind) IS NOT NULL
       AND NOT EXISTS (SELECT 1 FROM search_postings p
                        WHERE p.tenant_id = t AND p.memory_id = m.id AND p.kind = 2)
    ON CONFLICT (tenant_id, memory_id) DO NOTHING;
    INSERT INTO search_term_rows (tenant_id, workspace_id, kind, memory_kind, token_id, rows)
    SELECT p.tenant_id, p.workspace_id, 1, p.memory_kind, p.token_id, count(DISTINCT p.memory_id)
      FROM search_postings p
     WHERE p.tenant_id = t AND p.kind = 1 AND p.state = 1
     GROUP BY p.tenant_id, p.workspace_id, p.memory_kind, p.token_id;
    INSERT INTO search_workspace_rows (tenant_id, workspace_id, kind, memory_kind, rows,
                                       total_len, units)
    SELECT m.tenant_id, m.workspace_id, 1, oml_search_memory_kind(m.kind),
           count(DISTINCT m.id), sum(u.search_len), count(*)
      FROM memories m
     CROSS JOIN LATERAL oml_search_units(m.tenant_id, m.id, m.kind, m.search_vector,
                                         m.search_len) u
     WHERE m.tenant_id = t AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind) = 1
     GROUP BY m.tenant_id, m.workspace_id, oml_search_memory_kind(m.kind);
END
$$;

------------------------------------------------------------------------------
-- oml_search_vocabulary_fold — 0032's fold with `document` among the kinds
-- whose promoted served tips give did_you_mean its words, as every hit kind
-- does (ADR-0049 d6; ADR-0092 d1; the build's reading R8).
------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION oml_search_vocabulary_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    was_live boolean := TG_OP <> 'INSERT'
        AND OLD.status = 'promoted' AND OLD.is_tip AND NOT OLD.retirement
        AND OLD.kind IN ('memory', 'derived_fact', 'document');
    is_live boolean := TG_OP <> 'DELETE'
        AND NEW.status = 'promoted' AND NEW.is_tip AND NOT NEW.retirement
        AND NEW.kind IN ('memory', 'derived_fact', 'document');
    moved boolean := TG_OP = 'UPDATE'
        AND (coalesce(OLD.search_text, OLD.claim) IS DISTINCT FROM coalesce(NEW.search_text, NEW.claim)
             OR OLD.workspace_id <> NEW.workspace_id OR OLD.kind <> NEW.kind);
BEGIN
    IF was_live AND (NOT is_live OR moved) THEN
        DELETE FROM search_vocabulary v
         WHERE v.tenant_id = OLD.tenant_id AND v.workspace_id = OLD.workspace_id
           AND v.word = ANY (oml_search_words(coalesce(OLD.search_text, OLD.claim))) AND v.rows = 1;
        UPDATE search_vocabulary v SET rows = v.rows - 1
         WHERE v.tenant_id = OLD.tenant_id AND v.workspace_id = OLD.workspace_id
           AND v.word = ANY (oml_search_words(coalesce(OLD.search_text, OLD.claim)));
    END IF;
    IF is_live AND (NOT was_live OR moved) THEN
        INSERT INTO search_vocabulary (tenant_id, workspace_id, word, rows)
        SELECT NEW.tenant_id, NEW.workspace_id, w, 1
          FROM unnest(oml_search_words(coalesce(NEW.search_text, NEW.claim))) w
        ON CONFLICT (tenant_id, workspace_id, word)
        DO UPDATE SET rows = search_vocabulary.rows + 1;
    END IF;
    RETURN NULL;
END
$$;

-- The serving role reads a version's units and counts them inside the
-- fold it runs.
DO $$
BEGIN
    EXECUTE format('GRANT EXECUTE ON FUNCTION '
                   'oml_search_units(uuid, uuid, text, tsvector, integer) TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT EXECUTE ON FUNCTION '
                   'oml_search_count_version(uuid, uuid, smallint, bigint[], bigint, integer, '
                   'integer) TO %I', current_setting('oml.serving_role'));
    EXECUTE format('GRANT EXECUTE ON FUNCTION '
                   'oml_search_count_units(uuid, uuid, uuid, text, tsvector, integer, integer) '
                   'TO %I', current_setting('oml.serving_role'));
END $$;
