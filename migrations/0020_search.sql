-- 0020_search — the search pipelines' text, stored lexemes, vocabulary and
-- indexes, and the tenant's default pipeline (ADR-0049 d4, d6, d9, d12;
-- ADR-0048 S2, S3).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. 0016 is left to v0.5.1's guardrails and 0017-0019 to v0.5.2.
-- Normative shapes: docs/data/postgres-schema.md "search (ADR-0049)".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- oml_search_split — a text's identifier parts: a lower-to-upper case change
-- splits a CamelCase run, and `.`, `_`, `/`, `:`, `#`, `@` and `-` become
-- spaces, so `useInvoiceFilters.ts` reads `use Invoice Filters ts`
-- (ADR-0048 d3). oml_search_text is the text and its parts, the one
-- normaliser of the query, the trigram index and the vocabulary. IMMUTABLE,
-- so an index and a generated column hold them; SECURITY INVOKER, so the
-- role split's definer check passes.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_split(t text) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN regexp_replace(
        regexp_replace(
            regexp_replace(t, '([[:lower:][:digit:]])([[:upper:]])', '\1 \2', 'g'),
            '([[:upper:]])([[:upper:]][[:lower:]])', '\1 \2', 'g'),
        '[._/:#@-]+', ' ', 'g');

CREATE FUNCTION oml_search_text(t text) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN t || ' ' || oml_search_split(t);

------------------------------------------------------------------------------
-- oml_search_vector — what the bm25 retriever scores: the text's `simple`
-- lexemes, then the identifier parts it does not already hold, then the
-- `english` stems neither holds, the stems taken from the text and from
-- the parts it lacks, never from the text twice. Each occurrence counts
-- once: a word the text says once has one position, whether it is matched
-- by its exact form or by its stem (`runs` holds `run` once, as `run`
-- does), where concatenating the text with its split copy, or `simple`
-- with `english`, would count it two to four times (ADR-0049 d12;
-- eval/bench/REPORT.md). An exact form always matches, a non-English word
-- included, and an English word also matches its stem.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_exact(t text) RETURNS tsvector
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN to_tsvector('simple'::regconfig, t)
        || ts_delete(to_tsvector('simple'::regconfig, oml_search_split(t)),
                     tsvector_to_array(to_tsvector('simple'::regconfig, t)));

CREATE FUNCTION oml_search_stems(t text) RETURNS tsvector
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN to_tsvector('english'::regconfig, t)
        || ts_delete(to_tsvector('english'::regconfig, oml_search_split(t)),
                     tsvector_to_array(to_tsvector('english'::regconfig, t)));

CREATE FUNCTION oml_search_vector(t text) RETURNS tsvector
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN oml_search_exact(t)
        || ts_delete(oml_search_stems(t), tsvector_to_array(oml_search_exact(t)));

COMMENT ON FUNCTION oml_search_vector(text) IS
    'The bm25 retriever''s lexemes, each occurrence once: simple, identifier parts, english stems '
    '(ADR-0049 d12). Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- memories.search_vector and search_len — each version's lexemes and their
-- count, computed once when the row is written, never rebuilt by a read
-- (ADR-0049 d12). Plain columns set by a BEFORE trigger rather than
-- generated ones, so the vector is computed once per write: a generated
-- search_len could not read search_vector and would compute it again. The
-- trigger fires on an insert and on an update naming claim or either
-- column, so a write cannot store lexemes that are not its claim's, and a
-- status change copies nothing. Every existing row is filled below, under
-- its tenant's row security; a partition created later takes the columns
-- through LIKE ... INCLUDING ALL and the trigger through ATTACH PARTITION.
-- No text index is built on them: under row security `@@` is not
-- leakproof, so the serving role's reads scan the scope and no index would
-- serve them (ADR-0034 d13). An erasure that rewrites a claim rewrites its
-- lexemes through this trigger; one that changed only the status would
-- leave the erased text in search_vector (ADR-0049 d12).
------------------------------------------------------------------------------
ALTER TABLE memories
    ADD COLUMN search_vector tsvector,
    ADD COLUMN search_len integer;

COMMENT ON COLUMN memories.search_vector IS
    'oml_search_vector(claim), set at write by memories_search_vector (ADR-0049 d12). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN memories.search_len IS
    'The count of distinct lexemes in search_vector, BM25''s row length (ADR-0049 d12). '
    'Owning page: docs/data/postgres-schema.md.';

CREATE FUNCTION oml_search_vector_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    NEW.search_vector := oml_search_vector(NEW.claim);
    NEW.search_len := length(NEW.search_vector);
    RETURN NEW;
END
$$;

-- The lexemes of the rows already written, under each tenant's row
-- security, the tenants enumerated under 0003's startup policy as 0005
-- enumerates them; each vector is computed once.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE memories m SET search_vector = v.sv, search_len = length(v.sv)
          FROM (SELECT id, oml_search_vector(claim) AS sv FROM memories WHERE tenant_id = t) v
         WHERE m.tenant_id = t AND m.id = v.id;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;

ALTER TABLE memories
    ALTER COLUMN search_vector SET NOT NULL,
    ALTER COLUMN search_len SET NOT NULL;

CREATE TRIGGER memories_search_vector
    BEFORE INSERT OR UPDATE OF claim, search_vector, search_len
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vector_fold();

------------------------------------------------------------------------------
-- search_vocabulary — the words did_you_mean may offer: for each workarea,
-- every `simple` word of oml_search_text over its promoted served tips that
-- are no retirement version, with the count of those rows that say it
-- (ADR-0049 d6, d12). A word leaves when the last such row stops being one
-- (replaced, retired, demoted), so a suggestion never names a word that only
-- a held row, a replaced version or another workarea says. Maintained by the
-- trigger below inside the write's own transaction; never journalled, since
-- it is derived from rows the journal already folds.
------------------------------------------------------------------------------
CREATE TABLE search_vocabulary (
    tenant_id   uuid    NOT NULL,
    workarea_id uuid    NOT NULL,
    word        text    NOT NULL,
    rows        integer NOT NULL,
    CONSTRAINT search_vocabulary_pkey PRIMARY KEY (tenant_id, workarea_id, word),
    CONSTRAINT search_vocabulary_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT search_vocabulary_rows CHECK (rows > 0)
);

-- A tenant's words are its facts, as claim_edges' columns are (FR-45).
ALTER TABLE search_vocabulary
    ALTER COLUMN tenant_id   SET STATISTICS 0,
    ALTER COLUMN workarea_id SET STATISTICS 0,
    ALTER COLUMN word        SET STATISTICS 0,
    ALTER COLUMN rows        SET STATISTICS 0;

ALTER TABLE search_vocabulary ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_vocabulary FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON search_vocabulary
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE search_vocabulary IS
    'Per workarea, the simple words of its promoted served tips and how many say each; '
    'did_you_mean''s only source (ADR-0049 d6, d12). Owning page: docs/data/postgres-schema.md.';

-- The trigger's upserts and decrements and the suggestion's read.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON search_vocabulary TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- The words a claim adds: its distinct `simple` words with a letter, in
-- byte order.
CREATE FUNCTION oml_search_words(t text) RETURNS text[]
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN ARRAY(SELECT w FROM unnest(tsvector_to_array(to_tsvector('simple'::regconfig,
                                                                  oml_search_text(t)))) w
                  WHERE w ~ '[[:alpha:]]' ORDER BY w COLLATE "C");

-- A row counts while it is a promoted served tip and no retirement version.
-- Words are taken and given back in byte order, so two writes of one tenant
-- lock vocabulary rows in the same order.
CREATE FUNCTION oml_search_vocabulary_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    was_live boolean := TG_OP <> 'INSERT'
        AND OLD.status = 'promoted' AND OLD.is_tip AND NOT OLD.retirement
        AND OLD.kind IN ('memory', 'derived_fact');
    is_live boolean := TG_OP <> 'DELETE'
        AND NEW.status = 'promoted' AND NEW.is_tip AND NOT NEW.retirement
        AND NEW.kind IN ('memory', 'derived_fact');
    moved boolean := TG_OP = 'UPDATE'
        AND (OLD.claim IS DISTINCT FROM NEW.claim OR OLD.workarea_id <> NEW.workarea_id
             OR OLD.kind <> NEW.kind);
BEGIN
    IF was_live AND (NOT is_live OR moved) THEN
        DELETE FROM search_vocabulary v
         WHERE v.tenant_id = OLD.tenant_id AND v.workarea_id = OLD.workarea_id
           AND v.word = ANY (oml_search_words(OLD.claim)) AND v.rows = 1;
        UPDATE search_vocabulary v SET rows = v.rows - 1
         WHERE v.tenant_id = OLD.tenant_id AND v.workarea_id = OLD.workarea_id
           AND v.word = ANY (oml_search_words(OLD.claim));
    END IF;
    IF is_live AND (NOT was_live OR moved) THEN
        INSERT INTO search_vocabulary (tenant_id, workarea_id, word, rows)
        SELECT NEW.tenant_id, NEW.workarea_id, w, 1 FROM unnest(oml_search_words(NEW.claim)) w
        ON CONFLICT (tenant_id, workarea_id, word)
        DO UPDATE SET rows = search_vocabulary.rows + 1;
    END IF;
    RETURN NULL;
END
$$;

CREATE TRIGGER memories_search_vocabulary
    AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, workarea_id, kind OR DELETE
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_vocabulary_fold();

-- The vocabulary of the rows already promoted, read and written under each
-- tenant's row security, the tenants enumerated under 0003's startup policy
-- as 0005 enumerates them.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        INSERT INTO search_vocabulary (tenant_id, workarea_id, word, rows)
        SELECT m.tenant_id, m.workarea_id, w, count(*)
          FROM memories m CROSS JOIN LATERAL unnest(oml_search_words(m.claim)) w
         WHERE m.tenant_id = t AND m.status = 'promoted' AND m.is_tip AND NOT m.retirement
           AND m.kind IN ('memory', 'derived_fact')
         GROUP BY 1, 2, 3;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;

------------------------------------------------------------------------------
-- pg_trgm, for the opt-in bm25-trigram pipeline and did_you_mean's
-- similarity (ADR-0048 d4; ADR-0049 d12). A trusted extension: the owner
-- role creates it when it holds CREATE on the database, as it does when it
-- owns the database (deployment.md). Where it does not, or the contrib
-- package is not installed, the migration goes on without it: the default pipelines need none, did_you_mean answers []
-- and a lead whose search.enabled names bm25-trigram refuses to start,
-- naming the extension (configuration.md search.enabled); doctor names its
-- absence. No trigram index is built: under row security `%` and `<%` are
-- not leakproof, so no index would serve the serving role's reads
-- (ADR-0034 d13).
------------------------------------------------------------------------------
DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pg_trgm;
EXCEPTION WHEN insufficient_privilege OR undefined_file OR feature_not_supported THEN
    RAISE NOTICE 'pg_trgm not created (%): bm25-trigram and did_you_mean are not served', SQLERRM;
END
$$;

------------------------------------------------------------------------------
-- tenants.search_default — the tenant's default pipeline, folded from
-- search.default_set; NULL follows the deployment's search.default
-- (ADR-0049 d4). The serving role already holds UPDATE on tenants (0007).
------------------------------------------------------------------------------
ALTER TABLE tenants ADD COLUMN search_default text
    CHECK (search_default IS NULL OR search_default ~ '^[a-z0-9-]{1,40}$');

COMMENT ON COLUMN tenants.search_default IS
    'The tenant''s default search pipeline by name, or NULL for the deployment''s (ADR-0049 d4). '
    'Owning page: docs/data/postgres-schema.md.';
