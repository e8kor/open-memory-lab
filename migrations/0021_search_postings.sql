-- 0021_search_postings — integer postings that row security lets an index
-- serve, the readable rows' counts kept per workarea, the trigger that keeps
-- both with each version's state, and the fill of the rows already written
-- (ADR-0052 d1, d2, d3, d5, d7, d8).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "search
-- postings (ADR-0052)".
--
-- Under row security no text operator reaches an index: `@@`, `%` and
-- `ts_rank` are not LEAKPROOF, so the planner keeps them above the
-- tenant_isolation policy (ADR-0034 d13). Integer and uuid equality are
-- leakproof, so `token_id = ANY($n)` stays an index condition for the
-- serving role (ADR-0052 Context). Nothing here is SECURITY DEFINER and
-- nothing is marked LEAKPROOF.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- oml_token_id — a lexeme's id: the first eight bytes of SHA-256 over its
-- UTF-8 bytes, read as a signed big-endian bigint (ADR-0052 d2). Computed,
-- never looked up, so no table maps a tenant's words to ids and a write
-- assigns nothing; oml-pg computes the same id (`token_id`), held equal by
-- a law.
------------------------------------------------------------------------------
CREATE FUNCTION oml_token_id(t text) RETURNS bigint
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN ('x' || encode(substr(sha256(convert_to(t, 'UTF8')), 1, 8), 'hex'))::bit(64)::bigint;

COMMENT ON FUNCTION oml_token_id(text) IS
    'A lexeme''s posting id: SHA-256''s first eight bytes as a signed bigint (ADR-0052 d2). '
    'Owning page: docs/data/postgres-schema.md.';

-- A version's posting state: 0 held, 1 served (a promoted tip that is no
-- retirement version), 2 off (replaced, demoted, rejected), or NULL for a
-- version that has no postings: a retirement version, a kind recall never
-- serves, and an erased version (ADR-0052 d1, d7).
CREATE FUNCTION oml_search_state(status text, is_tip boolean, retirement boolean, kind text)
    RETURNS smallint
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    RETURN CASE
        WHEN retirement OR kind IS NULL OR kind NOT IN ('memory', 'derived_fact') THEN NULL
        WHEN status = 'held' THEN 0
        WHEN status = 'promoted' AND is_tip THEN 1
        WHEN status IN ('promoted', 'demoted', 'rejected') THEN 2
        ELSE NULL
    END;

-- The memory kind as the postings and counts store it: 1 memory,
-- 2 derived_fact.
CREATE FUNCTION oml_search_memory_kind(kind text) RETURNS smallint
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN CASE kind WHEN 'memory' THEN 1 WHEN 'derived_fact' THEN 2 END;

------------------------------------------------------------------------------
-- search_postings — one row per (version, token): kind 1 holds a lexeme of
-- the version's stored search_vector with its term frequency and the
-- version's search_len, BM25's row length; kind 2 is kept for a learned
-- sparse encoder's weights and is not written yet (ADR-0052 d1, d3).
-- List-partitioned by tenant_id, one partition search_postings_t_<tenant id,
-- 32 hex> per tenant, each with ENABLE and FORCE ROW LEVEL SECURITY and the
-- tenant_isolation policy, as memories' are (0009). workarea_id and
-- memory_kind are copied at write (a version never changes either); state
-- flips with the version. Two indexes, and only leakproof equalities reach
-- them. search_postings_served holds the served postings alone, ordered by
-- token then workarea and carrying every column a score needs, so a recall
-- reads its readable workareas' postings of a token by an index-only scan
-- and a leaf scope reads its own workareas' alone (ADR-0052 open question 1,
-- measured on bench/scale); a state flip in or out of served therefore adds
-- or removes its entry, and is no heap-only update. search_postings_memory
-- serves the rescoring, the own held rows, the flips and the erase. No
-- foreign key: a partition exists only for a tenant that exists, and a row
-- naming no partition is refused (23514).
------------------------------------------------------------------------------
CREATE TABLE search_postings (
    tenant_id   uuid     NOT NULL,
    memory_id   uuid     NOT NULL,
    workarea_id uuid     NOT NULL,
    token_id    bigint   NOT NULL,
    weight      real     NOT NULL,
    doc_len     integer  NOT NULL,
    kind        smallint NOT NULL,
    memory_kind smallint NOT NULL,
    state       smallint NOT NULL,
    CONSTRAINT search_postings_kind CHECK (kind IN (1, 2)),
    CONSTRAINT search_postings_memory_kind CHECK (memory_kind IN (1, 2)),
    CONSTRAINT search_postings_state CHECK (state IN (0, 1, 2))
) PARTITION BY LIST (tenant_id);

CREATE INDEX search_postings_served ON search_postings (kind, token_id, workarea_id)
    INCLUDE (memory_kind, memory_id, weight, doc_len, tenant_id) WHERE state = 1;
CREATE INDEX search_postings_memory ON search_postings (memory_id);

ALTER TABLE search_postings ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_postings FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON search_postings
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE search_postings IS
    'Per version and token, the term frequency (kind 1) the bm25-postings retriever reads by '
    'index under row security, list-partitioned by tenant_id (ADR-0052 d1). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- search_term_rows and search_workarea_rows — the served versions' counts
-- per workarea and memory kind: how many hold each token, and how many there
-- are with their summed search_len (ADR-0052 d5). A recall sums them over
-- its readable nodes, so its row count, mean length and document
-- frequencies are the readable rows' and never the tenant's; a tenant-wide
-- count is never kept. A count reaching zero deletes its row. Fenced like
-- search_vocabulary, with no statistics on any column (FR-45).
------------------------------------------------------------------------------
CREATE TABLE search_term_rows (
    tenant_id   uuid     NOT NULL,
    workarea_id uuid     NOT NULL,
    kind        smallint NOT NULL,
    memory_kind smallint NOT NULL,
    token_id    bigint   NOT NULL,
    rows        integer  NOT NULL,
    CONSTRAINT search_term_rows_pkey
        PRIMARY KEY (tenant_id, workarea_id, kind, memory_kind, token_id),
    CONSTRAINT search_term_rows_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT search_term_rows_rows CHECK (rows > 0)
);

CREATE TABLE search_workarea_rows (
    tenant_id   uuid     NOT NULL,
    workarea_id uuid     NOT NULL,
    kind        smallint NOT NULL,
    memory_kind smallint NOT NULL,
    rows        integer  NOT NULL,
    total_len   bigint   NOT NULL,
    CONSTRAINT search_workarea_rows_pkey PRIMARY KEY (tenant_id, workarea_id, kind, memory_kind),
    CONSTRAINT search_workarea_rows_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT search_workarea_rows_rows CHECK (rows > 0)
);

ALTER TABLE search_term_rows
    ALTER COLUMN tenant_id   SET STATISTICS 0,
    ALTER COLUMN workarea_id SET STATISTICS 0,
    ALTER COLUMN kind        SET STATISTICS 0,
    ALTER COLUMN memory_kind SET STATISTICS 0,
    ALTER COLUMN token_id    SET STATISTICS 0,
    ALTER COLUMN rows        SET STATISTICS 0;
ALTER TABLE search_workarea_rows
    ALTER COLUMN tenant_id   SET STATISTICS 0,
    ALTER COLUMN workarea_id SET STATISTICS 0,
    ALTER COLUMN kind        SET STATISTICS 0,
    ALTER COLUMN memory_kind SET STATISTICS 0,
    ALTER COLUMN rows        SET STATISTICS 0,
    ALTER COLUMN total_len   SET STATISTICS 0;

ALTER TABLE search_term_rows ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_term_rows FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON search_term_rows
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
ALTER TABLE search_workarea_rows ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_workarea_rows FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON search_workarea_rows
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE search_term_rows IS
    'Per workarea, memory kind and token, how many served versions hold the token (ADR-0052 d5). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON TABLE search_workarea_rows IS
    'Per workarea and memory kind, the served versions and their summed search_len (ADR-0052 d5). '
    'Owning page: docs/data/postgres-schema.md.';

-- The trigger's writes and the retriever's reads.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON search_postings TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON search_term_rows TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON search_workarea_rows TO %I',
                   current_setting('oml.serving_role'));
END $$;

------------------------------------------------------------------------------
-- oml_search_count — adds (delta 1) or takes back (delta -1) one served
-- version's tokens and length in its workarea's counts. Tokens are taken in
-- id order, so two writes of one tenant lock count rows in the same order.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_count(t uuid, wa uuid, mk smallint, sv tsvector, len integer,
                                 delta integer) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF delta > 0 THEN
        INSERT INTO search_term_rows (tenant_id, workarea_id, kind, memory_kind, token_id, rows)
        SELECT t, wa, 1, mk, x.id, 1
          FROM (SELECT DISTINCT oml_token_id(l) AS id FROM unnest(tsvector_to_array(sv)) l) x
         ORDER BY x.id
        ON CONFLICT (tenant_id, workarea_id, kind, memory_kind, token_id)
        DO UPDATE SET rows = search_term_rows.rows + 1;
        INSERT INTO search_workarea_rows (tenant_id, workarea_id, kind, memory_kind, rows, total_len)
        VALUES (t, wa, 1, mk, 1, len)
        ON CONFLICT (tenant_id, workarea_id, kind, memory_kind)
        DO UPDATE SET rows = search_workarea_rows.rows + 1,
                      total_len = search_workarea_rows.total_len + EXCLUDED.total_len;
    ELSE
        DELETE FROM search_term_rows c
         WHERE c.tenant_id = t AND c.workarea_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.token_id = ANY (ARRAY(SELECT oml_token_id(l) FROM unnest(tsvector_to_array(sv)) l))
           AND c.rows = 1;
        UPDATE search_term_rows c SET rows = c.rows - 1
         WHERE c.tenant_id = t AND c.workarea_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.token_id = ANY (ARRAY(SELECT oml_token_id(l) FROM unnest(tsvector_to_array(sv)) l));
        DELETE FROM search_workarea_rows c
         WHERE c.tenant_id = t AND c.workarea_id = wa AND c.kind = 1 AND c.memory_kind = mk
           AND c.rows = 1;
        UPDATE search_workarea_rows c SET rows = c.rows - 1, total_len = c.total_len - len
         WHERE c.tenant_id = t AND c.workarea_id = wa AND c.kind = 1 AND c.memory_kind = mk;
    END IF;
END
$$;

------------------------------------------------------------------------------
-- oml_search_postings_fold — postings follow the version (ADR-0052 d7, d8):
-- written with it, one per lexeme of its stored search_vector; their state
-- flips with promotion, supersession, demotion, rejection and retirement,
-- and the counts follow each change into or out of served; only an erase
-- (or a row's deletion) deletes them. A version whose lexemes, workarea or
-- kind change gives its postings back and takes new ones, so no posting
-- holds a word the claim no longer says. No text is parsed: the lexemes
-- come from the stored column.
------------------------------------------------------------------------------
CREATE FUNCTION oml_search_postings_fold() RETURNS trigger
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
              OR OLD.workarea_id <> NEW.workarea_id OR OLD.kind IS DISTINCT FROM NEW.kind;
        IF NOT moved AND old_state IS NOT DISTINCT FROM new_state THEN
            RETURN NULL;
        END IF;
    END IF;
    -- The old postings leave: the row is gone, erased, no longer one with
    -- postings, or says other words.
    IF old_state IS NOT NULL AND (new_state IS NULL OR moved) THEN
        IF old_state = 1 THEN
            PERFORM oml_search_count(OLD.tenant_id, OLD.workarea_id,
                                     oml_search_memory_kind(OLD.kind), OLD.search_vector,
                                     OLD.search_len, -1);
        END IF;
        DELETE FROM search_postings p WHERE p.tenant_id = OLD.tenant_id AND p.memory_id = OLD.id;
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

------------------------------------------------------------------------------
-- One postings partition per existing tenant, fenced before it is
-- attached, as 0009 builds memories' and the lead builds a new tenant's
-- (ADR-0034 d11).
------------------------------------------------------------------------------
DO $$
DECLARE
    t         uuid;
    partition text;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        partition := 'search_postings_t_' || replace(t::text, '-', '');
        EXECUTE format('CREATE TABLE %I (LIKE search_postings INCLUDING ALL)', partition);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', partition);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', partition);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I '
            'USING (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid) '
            'WITH CHECK (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid)',
            partition);
        EXECUTE format('ALTER TABLE search_postings ATTACH PARTITION %I FOR VALUES IN (%L)',
                       partition, t);
    END LOOP;
    PERFORM set_config('oml.startup_check', '', true);
END $$;

-- The trigger first, so its lock on memories is held from here to the
-- end of the fill and no write lands between the fill's read and its
-- commit (ADR-0052 d8).
CREATE TRIGGER memories_search_postings
    AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, search_vector, workarea_id, kind
          OR DELETE
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_search_postings_fold();

------------------------------------------------------------------------------
-- The fill: every existing version's postings and the served versions'
-- counts, read from the stored lexemes and never from text, under each
-- tenant's row security, the tenants enumerated under 0003's startup policy
-- as 0020 enumerates them. Writes wait for it (FR-103).
------------------------------------------------------------------------------
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        INSERT INTO search_postings (tenant_id, memory_id, workarea_id, token_id, weight, doc_len,
                                     kind, memory_kind, state)
        SELECT m.tenant_id, m.id, m.workarea_id, oml_token_id(u.lexeme),
               GREATEST(cardinality(u.positions), 1), m.search_len, 1,
               oml_search_memory_kind(m.kind),
               oml_search_state(m.status, m.is_tip, m.retirement, m.kind)
          FROM memories m CROSS JOIN LATERAL unnest(m.search_vector) u
         WHERE m.tenant_id = t
           AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind) IS NOT NULL;
        INSERT INTO search_term_rows (tenant_id, workarea_id, kind, memory_kind, token_id, rows)
        SELECT p.tenant_id, p.workarea_id, 1, p.memory_kind, p.token_id,
               count(DISTINCT p.memory_id)
          FROM search_postings p
         WHERE p.tenant_id = t AND p.kind = 1 AND p.state = 1
         GROUP BY p.tenant_id, p.workarea_id, p.memory_kind, p.token_id;
        INSERT INTO search_workarea_rows (tenant_id, workarea_id, kind, memory_kind, rows,
                                          total_len)
        SELECT m.tenant_id, m.workarea_id, 1, oml_search_memory_kind(m.kind), count(*),
               sum(m.search_len)
          FROM memories m
         WHERE m.tenant_id = t AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind) = 1
         GROUP BY m.tenant_id, m.workarea_id, oml_search_memory_kind(m.kind);
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;

-- oml_search_postings_refill: not yet built.
CREATE FUNCTION oml_search_postings_refill(t uuid) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
END
$$;
