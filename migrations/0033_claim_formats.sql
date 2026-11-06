-- Claim formats (ADR-0056 d1, d2, d4, d10).
--
-- A version names its format, a media type from a closed list, and may carry
-- a one-line title. Both are signed in the submission event, so the row holds
-- them as written. A non-prose version also stores the search text its
-- adapter derived, with the adapter and version that derived it; the
-- lexemes, the postings, the vocabulary and the sparse encoder read
-- coalesce(search_text, claim), so a prose row's lexemes are the lexemes of
-- its claim, exactly as before (ADR-0049 d12; ADR-0052 d8). Every existing
-- row is text/plain with no title and no derived text, so nothing is
-- rewritten.
--
-- Safe on a live database, as 0025 and 0026 are: a lock is waited for at most
-- 5 s, then the migration fails and is run again in a quieter moment. Adding
-- a column with a constant default is a catalog change. The checks are added
-- NOT VALID, which takes no scan under the exclusive lock this transaction
-- holds until it commits; 0034 validates them, outside any transaction, so
-- the scans run under SHARE UPDATE EXCLUSIVE, which lets reads and writes
-- run (review of #90, H2). The triggers are replaced under the lock their
-- creation takes on each partition, which waits on no row.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories
    ADD COLUMN format text NOT NULL DEFAULT 'text/plain',
    ADD COLUMN title text,
    ADD COLUMN search_text text,
    ADD COLUMN search_text_by text;

ALTER TABLE memories
    ADD CONSTRAINT memories_format_shape CHECK (
        -- \x3b is the semicolon of `text/vnd.oml.code; lang=rust`
        format ~ '^[a-z]+/[a-z0-9.+-]+(\x3b lang=[a-z0-9]+)?$') NOT VALID;
ALTER TABLE memories
    ADD CONSTRAINT memories_title_line CHECK (
        title IS NULL OR (char_length(title) BETWEEN 1 AND 100
                          AND title !~ '[\n\r]')) NOT VALID;
ALTER TABLE memories
    ADD CONSTRAINT memories_search_text_by CHECK (
        (search_text IS NULL) = (search_text_by IS NULL)) NOT VALID;

COMMENT ON COLUMN memories.format IS
    'The media type signed with the version, fixed for it (ADR-0056 d1, d2). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN memories.title IS
    'The one-line title signed with the version; required of a non-prose claim (ADR-0056 d10). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN memories.search_text IS
    'The text search reads in place of claim, derived by the format''s adapter; NULL when it equals claim (ADR-0056 d4). '
    'Plaintext derived from the body, erased with it (ADR-0005 d4). Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN memories.search_text_by IS
    'The adapter and version that derived search_text, such as mermaid/1 (ADR-0056 d3, d4). '
    'Owning page: docs/data/postgres-schema.md.';

-- The lexemes of a row are those of its search text, or of its claim when
-- it has none: a write cannot store lexemes that are not its search text's.
CREATE OR REPLACE FUNCTION oml_search_vector_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    NEW.search_vector := oml_search_vector(coalesce(NEW.search_text, NEW.claim));
    NEW.search_len := length(NEW.search_vector);
    RETURN NEW;
END
$$;

DROP TRIGGER memories_search_vector ON memories;
CREATE TRIGGER memories_search_vector
    BEFORE INSERT OR UPDATE OF claim, search_text, search_vector, search_len
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vector_fold();

-- The postings follow the stored lexemes; a change of search text moves them
-- as a change of claim does.
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
              OR OLD.workarea_id <> NEW.workarea_id OR OLD.kind IS DISTINCT FROM NEW.kind;
        IF NOT moved AND old_state IS NOT DISTINCT FROM new_state THEN
            RETURN NULL;
        END IF;
    END IF;
    IF old_state IS NOT NULL AND (new_state IS NULL OR moved) THEN
        IF old_state = 1 THEN
            PERFORM oml_search_count(OLD.tenant_id, OLD.workarea_id,
                                     oml_search_memory_kind(OLD.kind), OLD.search_vector,
                                     OLD.search_len, -1);
        END IF;
        DELETE FROM search_postings p WHERE p.tenant_id = OLD.tenant_id AND p.memory_id = OLD.id;
        DELETE FROM search_sparse_queue q WHERE q.tenant_id = OLD.tenant_id AND q.memory_id = OLD.id;
        old_state := NULL;
    END IF;
    IF new_state IS NULL THEN
        RETURN NULL;
    END IF;
    IF old_state IS NULL THEN
        INSERT INTO search_postings (tenant_id, memory_id, workarea_id, token_id, weight, doc_len,
                                     kind, memory_kind, state)
        SELECT NEW.tenant_id, NEW.id, NEW.workarea_id, oml_token_id(u.lexeme),
               GREATEST(cardinality(u.positions), 1), NEW.search_len, 1,
               oml_search_memory_kind(NEW.kind), new_state
          FROM unnest(NEW.search_vector) u;
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
            PERFORM oml_search_count(OLD.tenant_id, OLD.workarea_id,
                                     oml_search_memory_kind(OLD.kind), OLD.search_vector,
                                     OLD.search_len, -1);
        END IF;
    END IF;
    IF new_state = 1 THEN
        PERFORM oml_search_count(NEW.tenant_id, NEW.workarea_id, oml_search_memory_kind(NEW.kind),
                                 NEW.search_vector, NEW.search_len, 1);
    END IF;
    RETURN NULL;
END
$$;

DROP TRIGGER memories_search_postings ON memories;
CREATE TRIGGER memories_search_postings
    AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, search_text, search_vector,
                              workarea_id, kind
          OR DELETE
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_postings_fold();

-- The did_you_mean vocabulary is the words of the search text too.
CREATE OR REPLACE FUNCTION oml_search_vocabulary_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    was_live boolean := TG_OP <> 'INSERT'
        AND OLD.status = 'promoted' AND OLD.is_tip AND NOT OLD.retirement
        AND OLD.kind IN ('memory', 'derived_fact');
    is_live boolean := TG_OP <> 'DELETE'
        AND NEW.status = 'promoted' AND NEW.is_tip AND NOT NEW.retirement
        AND NEW.kind IN ('memory', 'derived_fact');
    moved boolean := TG_OP = 'UPDATE'
        AND (coalesce(OLD.search_text, OLD.claim) IS DISTINCT FROM coalesce(NEW.search_text, NEW.claim)
             OR OLD.workarea_id <> NEW.workarea_id OR OLD.kind <> NEW.kind);
BEGIN
    IF was_live AND (NOT is_live OR moved) THEN
        DELETE FROM search_vocabulary v
         WHERE v.tenant_id = OLD.tenant_id AND v.workarea_id = OLD.workarea_id
           AND v.word = ANY (oml_search_words(coalesce(OLD.search_text, OLD.claim))) AND v.rows = 1;
        UPDATE search_vocabulary v SET rows = v.rows - 1
         WHERE v.tenant_id = OLD.tenant_id AND v.workarea_id = OLD.workarea_id
           AND v.word = ANY (oml_search_words(coalesce(OLD.search_text, OLD.claim)));
    END IF;
    IF is_live AND (NOT was_live OR moved) THEN
        INSERT INTO search_vocabulary (tenant_id, workarea_id, word, rows)
        SELECT NEW.tenant_id, NEW.workarea_id, w, 1
          FROM unnest(oml_search_words(coalesce(NEW.search_text, NEW.claim))) w
        ON CONFLICT (tenant_id, workarea_id, word)
        DO UPDATE SET rows = search_vocabulary.rows + 1;
    END IF;
    RETURN NULL;
END
$$;

DROP TRIGGER memories_search_vocabulary ON memories;
CREATE TRIGGER memories_search_vocabulary
    AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, search_text, workarea_id, kind
          OR DELETE
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vocabulary_fold();
