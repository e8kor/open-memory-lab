-- 0022_search_sparse_queue — the outbox the lead's learned sparse encoder
-- drains: a version that gets postings is enqueued in its own write's
-- transaction, and the encoder writes its kind 2 postings later, off the
-- commit path (ADR-0052 d15). Replaces 0021's trigger function, so the
-- enqueue rides the same trigger, and 0021's refill, so a refill rebuilds
-- kind 1 alone and keeps the encoder's kind 2 rows. Nothing is queued for a
-- tenant until a lead serving a sparse pipeline turns its sparse state on
-- (search_sparse_state), so a deployment on the default pipelines keeps no
-- queue; that lead backfills the tenant's existing versions in bounded,
-- resumable steps (amended on review, 2026-11-05).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "search
-- postings (ADR-0052)".
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- search_sparse_queue — one row per version waiting for the encoder.
-- enqueued_at is the version's turn and its write's guard: the encoder
-- deletes the row only when it is the one it read, so a version enqueued
-- again meanwhile is encoded again. attempts, next_at and last_error carry
-- a failed encode's retry and its reason, which doctor names. Fenced like
-- the count tables, with no statistics (FR-45).
------------------------------------------------------------------------------
CREATE TABLE search_sparse_queue (
    tenant_id   uuid        NOT NULL,
    memory_id   uuid        NOT NULL,
    enqueued_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    attempts    integer     NOT NULL DEFAULT 0,
    next_at     timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_error  text,
    parked_at   timestamptz,
    CONSTRAINT search_sparse_queue_pkey PRIMARY KEY (tenant_id, memory_id),
    CONSTRAINT search_sparse_queue_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT search_sparse_queue_attempts CHECK (attempts >= 0)
);

-- Oldest first, the order the drain takes; a parked entry (a dead letter,
-- kept for doctor) is never taken.
CREATE INDEX search_sparse_queue_order ON search_sparse_queue (tenant_id, enqueued_at, memory_id)
    WHERE parked_at IS NULL;

ALTER TABLE search_sparse_queue
    ALTER COLUMN tenant_id   SET STATISTICS 0,
    ALTER COLUMN memory_id   SET STATISTICS 0,
    ALTER COLUMN enqueued_at SET STATISTICS 0,
    ALTER COLUMN next_at     SET STATISTICS 0;

ALTER TABLE search_sparse_queue ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_sparse_queue FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON search_sparse_queue
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

-- The lead's encoder finds the tenants with due entries in one statement,
-- under the transaction-local marker 0003's tenants policy reads: the
-- serving role is the only role connecting, and it reads ids and times
-- (ADR-0052 d15, MEDIUM-3 on review).
CREATE POLICY lead_sparse_enumeration ON search_sparse_queue FOR SELECT
    USING (current_setting('oml.startup_check', true) = 'on');

COMMENT ON TABLE search_sparse_queue IS
    'Versions waiting for the learned sparse encoder, which writes their kind 2 postings off the '
    'commit path (ADR-0052 d15). Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- search_sparse_state — per tenant, whether a lead serving a sparse
-- pipeline has turned the queue on, and how far its backfill of the
-- versions written before then has gone: the last version id it read
-- (backfill_after) and when it finished (backfilled_at). The trigger
-- enqueues only for a tenant whose row is enabled; a lead serving no
-- sparse pipeline turns every row off and empties the queue.
------------------------------------------------------------------------------
CREATE TABLE search_sparse_state (
    tenant_id      uuid        NOT NULL,
    enabled        boolean     NOT NULL,
    backfill_after uuid,
    backfilled_at  timestamptz,
    CONSTRAINT search_sparse_state_pkey PRIMARY KEY (tenant_id),
    CONSTRAINT search_sparse_state_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants (id)
);
ALTER TABLE search_sparse_state ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_sparse_state FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON search_sparse_state
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY lead_sparse_enumeration ON search_sparse_state FOR SELECT
    USING (current_setting('oml.startup_check', true) = 'on');
COMMENT ON TABLE search_sparse_state IS
    'Per tenant, whether the learned sparse queue is on and how far its backfill went (ADR-0052 d15). '
    'Owning page: docs/data/postgres-schema.md.';

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON search_sparse_queue TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON search_sparse_state TO %I',
                   current_setting('oml.serving_role'));
END $$;

------------------------------------------------------------------------------
-- oml_search_postings_fold — 0021's, with the queue: a version whose
-- postings are written is enqueued (again, with a new enqueued_at, when its
-- words changed), and a version whose postings leave leaves the queue. A
-- state flip moves every kind's postings, the encoder's included.
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

------------------------------------------------------------------------------
-- oml_search_postings_refill — 0021's, for kind 1 alone: kind 1 is a
-- function of the stored lexemes and is rebuilt from them; kind 2 is the
-- encoder's, so its rows keep their weights, take their versions' current
-- state, leave with a version that has no postings any more, and a version
-- with none is enqueued where the tenant's sparse state is on (ADR-0052 d15). Same checks and lock as 0021's.
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
    DELETE FROM search_workarea_rows c WHERE c.tenant_id = t;
    INSERT INTO search_postings (tenant_id, memory_id, workarea_id, token_id, weight, doc_len,
                                 kind, memory_kind, state)
    SELECT m.tenant_id, m.id, m.workarea_id, oml_token_id(u.lexeme),
           GREATEST(cardinality(u.positions), 1), m.search_len, 1,
           oml_search_memory_kind(m.kind),
           oml_search_state(m.status, m.is_tip, m.retirement, m.kind)
      FROM memories m CROSS JOIN LATERAL unnest(m.search_vector) u
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
    INSERT INTO search_term_rows (tenant_id, workarea_id, kind, memory_kind, token_id, rows)
    SELECT p.tenant_id, p.workarea_id, 1, p.memory_kind, p.token_id, count(DISTINCT p.memory_id)
      FROM search_postings p
     WHERE p.tenant_id = t AND p.kind = 1 AND p.state = 1
     GROUP BY p.tenant_id, p.workarea_id, p.memory_kind, p.token_id;
    INSERT INTO search_workarea_rows (tenant_id, workarea_id, kind, memory_kind, rows, total_len)
    SELECT m.tenant_id, m.workarea_id, 1, oml_search_memory_kind(m.kind), count(*),
           sum(m.search_len)
      FROM memories m
     WHERE m.tenant_id = t AND oml_search_state(m.status, m.is_tip, m.retirement, m.kind) = 1
     GROUP BY m.tenant_id, m.workarea_id, oml_search_memory_kind(m.kind);
END
$$;
REVOKE EXECUTE ON FUNCTION oml_search_postings_refill(uuid) FROM PUBLIC;

-- No fill here: nothing is queued until a lead serving a sparse pipeline
-- turns a tenant's state on, and that lead backfills the tenant's existing
-- versions itself, in bounded, resumable steps (ADR-0052 d15).
