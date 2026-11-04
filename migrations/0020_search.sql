-- 0020_search — the search pipelines' text, vectors and indexes, and the
-- tenant's default pipeline (ADR-0049 d4, d9; ADR-0048 S2, S3).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. 0016 is left to v0.5.1's guardrails and 0017-0019 to v0.5.2.
-- Normative shapes: docs/data/postgres-schema.md "search (ADR-0049)".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- oml_search_text — one normaliser for the indexed text and the query: the
-- text itself, then its identifier parts, so `useInvoiceFilters.ts` keeps
-- its whole token and adds `use`, `invoice`, `filters` and `ts` (ADR-0048
-- d3). A lower-to-upper case change splits a CamelCase run; `.`, `_`, `/`,
-- `:`, `#`, `@` and `-` become spaces. IMMUTABLE, so an expression index
-- holds it; SECURITY INVOKER, so the role split's definer check passes.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_text(t text) RETURNS text
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN t || ' ' || regexp_replace(
        regexp_replace(
            regexp_replace(t, '([[:lower:][:digit:]])([[:upper:]])', '\1 \2', 'g'),
            '([[:upper:]])([[:upper:]][[:lower:]])', '\1 \2', 'g'),
        '[._/:#@-]+', ' ', 'g');

------------------------------------------------------------------------------
-- oml_search_vector — the normalised text under `simple` and under
-- `english` together, so an exact form always matches, a non-English word
-- included, and an English word also matches its stem (ADR-0048 d3).
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_vector(t text) RETURNS tsvector
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN to_tsvector('simple'::regconfig, oml_search_text(t))
        || to_tsvector('english'::regconfig, oml_search_text(t));

COMMENT ON FUNCTION oml_search_vector(text) IS
    'The bm25 retriever''s lexemes: simple and english over oml_search_text (ADR-0049). '
    'Owning page: docs/data/postgres-schema.md.';

-- On the partitioned parent, so every tenant's partition gets its own, and
-- a partition created later inherits it through LIKE ... INCLUDING ALL.
CREATE INDEX memories_search_vector ON memories
    USING gin (oml_search_vector(claim));

------------------------------------------------------------------------------
-- pg_trgm, for the trigram retriever (ADR-0048 d4). A trusted extension:
-- the owner role creates it when it holds CREATE on the database, as it does
-- when it owns the database (deployment.md). Where it does not, the
-- migration goes on without it, the trigram retriever is not served, and a
-- lead whose search.enabled names a trigram pipeline refuses to start,
-- naming the extension (configuration.md search.enabled); doctor names its
-- absence. The index is created here when the extension exists, and by the
-- lead's start once an operator creates the extension later.
------------------------------------------------------------------------------
DO $$
BEGIN
    BEGIN
        CREATE EXTENSION IF NOT EXISTS pg_trgm;
    EXCEPTION WHEN insufficient_privilege THEN
        RAISE NOTICE 'pg_trgm not created (%): the trigram retriever is not served', SQLERRM;
    END;
    IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm') THEN
        EXECUTE 'CREATE INDEX IF NOT EXISTS memories_search_trgm ON memories '
             || 'USING gin (oml_search_text(claim) gin_trgm_ops)';
    END IF;
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
