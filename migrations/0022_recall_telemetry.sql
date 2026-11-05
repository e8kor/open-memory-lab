-- 0022_recall_telemetry — each recall recorded for 30 days under row
-- security, the labels of what its reader did next, and the tenant's opt-in
-- to keeping the query text (ADR-0054 d2, d3, d4, d5, as accepted).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "recall
-- telemetry (ADR-0054)".
--
-- Telemetry, not claims: no memory's text, no version, no verdict and no
-- journal event is stored here, nothing is derived from it, and every row is
-- gone 30 days after its recall (ADR-0054 d2). Both tables are
-- list-partitioned by tenant_id alone, as memories is, each partition created
-- once with its tenant on the owner path ADR-0034 d11 already holds; no owner
-- DDL runs on a schedule (open question 2, settled on review: tenant-only).
-- The serving role deletes rows older than 30 days in bounded batches, each
-- its own transaction, through the (tenant_id, created_at) indexes, and every
-- read filters the same bound. No DEFAULT partition, so an event of a tenant
-- with no partition is refused and counted as dropped. Every partition has
-- ENABLE and FORCE ROW LEVEL SECURITY and the tenant_isolation policy
-- (ADR-0034 d11, d13).
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- tenants.telemetry_query_text_since — the instant the tenant's admin turned
-- the query text on, folded from telemetry.settings_set; NULL is off, the
-- default. An event keeps its query text only when it was created at or
-- after this instant. The rule is the database's: the trigger below nulls
-- any other text, whichever statement writes it (ADR-0054 d3).
------------------------------------------------------------------------------
ALTER TABLE tenants ADD COLUMN telemetry_query_text_since timestamptz(6);

COMMENT ON COLUMN tenants.telemetry_query_text_since IS
    'When telemetry.query_text was turned on, NULL when off: recall_events keeps the query '
    'text of an event created at or after it (ADR-0054 d3). '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- recall_events — one row per recall: who recalled, where, through which
-- pipeline, the digest of the whole request always and its NFC query text
-- only by the tenant's opt-in, and the versions served in rank order with
-- their scores, at most 20 (ADR-0054 d2).
------------------------------------------------------------------------------
CREATE TABLE recall_events (
    tenant_id          uuid           NOT NULL,
    recall_id          uuid           NOT NULL,
    created_at         timestamptz(6) NOT NULL,
    key_id             uuid           NOT NULL,
    principal          text           NOT NULL,
    scope_workarea_id  uuid           NOT NULL,
    source_workarea_id uuid,
    pipeline           text           NOT NULL,
    access             text           NOT NULL,
    query_digest       bytea          NOT NULL,
    query_text         text,
    hit_ids            uuid[]         NOT NULL,
    hit_scores         real[]         NOT NULL,
    CONSTRAINT recall_events_pkey PRIMARY KEY (tenant_id, recall_id, created_at),
    CONSTRAINT recall_events_pipeline CHECK (pipeline ~ '^[a-z0-9-]{1,40}$'),
    CONSTRAINT recall_events_access CHECK (access IN ('postings', 'scan', 'none')),
    CONSTRAINT recall_events_digest CHECK (octet_length(query_digest) = 32),
    CONSTRAINT recall_events_hits CHECK (
        cardinality(hit_ids) = cardinality(hit_scores) AND cardinality(hit_ids) <= 20)
) PARTITION BY LIST (tenant_id);

-- The labeller finds a key's most recent event inside a window by this
-- index; the feedback check and a label's attachment by the primary key.
CREATE INDEX recall_events_key ON recall_events (tenant_id, key_id, created_at DESC);
-- The purge and the labeller's no_use pass read a created_at range.
CREATE INDEX recall_events_created ON recall_events (tenant_id, created_at);

------------------------------------------------------------------------------
-- recall_outcomes — what the reader did next, one row per (recall, version,
-- label): inferred (get, derived, superseded, requery, no_use) or explicit
-- (feedback_useful, feedback_not_useful). created_at is the event's, so a
-- label is purged with its event; artifact_id is NULL for
-- a label of the whole recall, requery and no_use (ADR-0054 d5, d6).
------------------------------------------------------------------------------
CREATE TABLE recall_outcomes (
    tenant_id   uuid           NOT NULL,
    recall_id   uuid           NOT NULL,
    created_at  timestamptz(6) NOT NULL,
    artifact_id uuid,
    label       text           NOT NULL,
    source      text           NOT NULL,
    at          timestamptz(6) NOT NULL,
    CONSTRAINT recall_outcomes_once
        UNIQUE NULLS NOT DISTINCT (tenant_id, recall_id, created_at, artifact_id, label),
    CONSTRAINT recall_outcomes_label CHECK (label IN ('get', 'derived', 'superseded',
        'feedback_useful', 'feedback_not_useful', 'requery', 'no_use')),
    CONSTRAINT recall_outcomes_source CHECK (source IN ('inferred', 'explicit')),
    CONSTRAINT recall_outcomes_artifact CHECK (
        (artifact_id IS NULL) = (label IN ('requery', 'no_use')))
) PARTITION BY LIST (tenant_id);

CREATE INDEX recall_outcomes_created ON recall_outcomes (tenant_id, created_at);

-- No statistics on the parents, and none on the columns that hold a reader's
-- words or names in any partition: each tenant's partition is created with
-- the same, so pg_stats holds no tenant's query (FR-45's rule).
ALTER TABLE ONLY recall_events
    ALTER COLUMN query_text   SET STATISTICS 0,
    ALTER COLUMN query_digest SET STATISTICS 0,
    ALTER COLUMN principal    SET STATISTICS 0,
    ALTER COLUMN hit_ids      SET STATISTICS 0;
ALTER TABLE ONLY recall_outcomes
    ALTER COLUMN artifact_id SET STATISTICS 0;

ALTER TABLE recall_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE recall_events FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON recall_events
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
ALTER TABLE recall_outcomes ENABLE ROW LEVEL SECURITY;
ALTER TABLE recall_outcomes FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON recall_outcomes
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE recall_events IS
    'One row per recall, telemetry kept 30 days: digest always, query text by opt-in, the '
    'served versions and scores; partitioned by tenant (ADR-0054 d2). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON TABLE recall_outcomes IS
    'What a recall''s reader did next, inferred or sent as feedback '
    '(ADR-0054 d5, d6). Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- oml_telemetry_erase — the rule erase's build calls (ADR-0054 d4; ADR-0005
-- d4): every event that served an erased version loses its query text, its
-- query digest (zeroed: the CHECK wants 32 bytes) and the version's id and
-- score, and the version's labels go, under the
-- tenant's row security, which it checks. SECURITY INVOKER; granted to the
-- serving role, which erases as it writes.
------------------------------------------------------------------------------
CREATE FUNCTION oml_telemetry_erase(t uuid, versions uuid[]) RETURNS bigint
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    touched bigint;
BEGIN
    IF t IS DISTINCT FROM NULLIF(current_setting('oml.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'oml_telemetry_erase(%) runs with oml.tenant_id set to that tenant', t;
    END IF;
    UPDATE recall_events e
       SET query_text = NULL,
           -- A short query's digest inverts by word list (ADR-0005 d4's row).
           query_digest = decode(repeat('00', 32), 'hex'),
           hit_ids = ARRAY(SELECT u.id FROM unnest(e.hit_ids, e.hit_scores)
                                WITH ORDINALITY AS u(id, score, n)
                            WHERE u.id <> ALL (versions) ORDER BY u.n),
           hit_scores = ARRAY(SELECT u.score FROM unnest(e.hit_ids, e.hit_scores)
                                WITH ORDINALITY AS u(id, score, n)
                               WHERE u.id <> ALL (versions) ORDER BY u.n)
     WHERE e.tenant_id = t AND e.hit_ids && versions;
    GET DIAGNOSTICS touched = ROW_COUNT;
    DELETE FROM recall_outcomes o WHERE o.tenant_id = t AND o.artifact_id = ANY (versions);
    RETURN touched;
END
$$;

REVOKE EXECUTE ON FUNCTION oml_telemetry_erase(uuid, uuid[]) FROM PUBLIC;

------------------------------------------------------------------------------
-- oml_telemetry_query_text_rule — the opt-in, enforced by the database: an
-- event keeps its query text only when its tenant turned the text on at or
-- before the event's created_at; any other text, by INSERT or UPDATE, from any
-- statement of the serving role, is stored NULL (ADR-0054 d3). It reads the
-- tenant's row FOR SHARE, so an opt-out (an UPDATE of that row) committing
-- while a writer's batch is open waits for the batch and then nulls its text,
-- or the batch reads the opt-out once it committed. Owner-owned, SECURITY
-- INVOKER (ADR-0034 d5); the serving role holds no TRIGGER privilege, so it
-- cannot drop or disable it.
------------------------------------------------------------------------------
CREATE FUNCTION oml_telemetry_query_text_rule() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    since timestamptz;
BEGIN
    IF NEW.query_text IS NOT NULL THEN
        SELECT t.telemetry_query_text_since INTO since
          FROM tenants t WHERE t.id = NEW.tenant_id FOR SHARE;
        IF since IS NULL OR since > NEW.created_at THEN
            NEW.query_text := NULL;
        END IF;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER recall_events_query_text_rule
    BEFORE INSERT OR UPDATE OF query_text, created_at ON recall_events
    FOR EACH ROW EXECUTE FUNCTION oml_telemetry_query_text_rule();

------------------------------------------------------------------------------
-- One partition of each table per existing tenant, fenced before it is
-- attached, as 0021 builds the postings'; the lead builds a new tenant's
-- with its memories partition (ADR-0034 d11).
------------------------------------------------------------------------------
DO $$
DECLARE
    t         uuid;
    tbl       text;
    partition text;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        FOREACH tbl IN ARRAY ARRAY['recall_events', 'recall_outcomes'] LOOP
            partition := tbl || '_t_' || replace(t::text, '-', '');
            EXECUTE format('CREATE TABLE %I (LIKE %I INCLUDING ALL)', partition, tbl);
            IF tbl = 'recall_events' THEN
                EXECUTE format('ALTER TABLE %I ALTER COLUMN query_text SET STATISTICS 0, '
                               'ALTER COLUMN query_digest SET STATISTICS 0, '
                               'ALTER COLUMN principal SET STATISTICS 0, '
                               'ALTER COLUMN hit_ids SET STATISTICS 0', partition);
            ELSE
                EXECUTE format('ALTER TABLE %I ALTER COLUMN artifact_id SET STATISTICS 0',
                               partition);
            END IF;
            EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', partition);
            EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', partition);
            EXECUTE format(
                'CREATE POLICY tenant_isolation ON %I '
                'USING (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid) '
                'WITH CHECK (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid)',
                partition);
            EXECUTE format('ALTER TABLE %I ATTACH PARTITION %I FOR VALUES IN (%L)',
                           tbl, partition, t);
        END LOOP;
    END LOOP;
    PERFORM set_config('oml.startup_check', '', true);
END $$;

-- The serving role writes, labels, nulls (d3, d4) and purges (d2) the
-- events; it owns neither table (ADR-0034 d2, d3).
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON recall_events TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON recall_outcomes TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT EXECUTE ON FUNCTION oml_telemetry_erase(uuid, uuid[]) TO %I',
                   current_setting('oml.serving_role'));
END $$;
