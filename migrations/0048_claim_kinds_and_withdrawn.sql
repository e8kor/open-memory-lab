-- The claim kinds skill, document and test, the withdrawn state and the
-- document passages table (ADR-0080 d1; ADR-0081 d1, d2, d3; ADR-0086 d1;
-- ADR-0083 d13).
--
-- memories.kind holds no CHECK (0005, 0016), so the three kinds need none.
-- Each brings its own fields, signed in the submission and stored beside the
-- version: a skill's name and description (ADR-0080 d1), a document's title,
-- sent or derived (ADR-0081 d1), and a test case's shape (ADR-0086 d1). Each
-- column is NULL on every other kind and on a retirement, which the check
-- below holds, so no row written before this migration needs a value.
--
-- The state `withdrawn` is a held version its author retracted: kept with
-- its text, never served and in no review queue (ADR-0083 d13). The live
-- claim index (0005, 0009) covers held, promoted and demoted versions only,
-- so a withdrawn version releases its claim key as a rejected one does. Its
-- search postings turn off as a rejected version's do; its similar keys go,
-- since oml_similar_state (0027) keeps keys for held versions and served
-- tips alone.
--
-- document_passages holds the passages ADR-0081 d3 cuts a document into at
-- capture. They are derived from the signed body and never signed
-- themselves, so they are erased with it. Nothing writes the table yet: the
-- passage cutter lands with recall's use of it (#405). One table, not
-- partitioned: a passage row is small, and the passages of one version are
-- read together by its key.
--
-- Numbered 0048, after the rename (0047). 0049 validates the checks.
--
-- What it costs a live database: catalog changes only. The columns are
-- nullable with no default and the checks are added NOT VALID, so nothing
-- scans memories here; the status check is swapped in the same transaction.
-- The function redefinition takes effect for the next trigger call. A lock
-- is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories
    ADD COLUMN skill_name        text,
    ADD COLUMN skill_description text,
    ADD COLUMN document_title    text,
    ADD COLUMN test_shape        text;

ALTER TABLE memories
    ADD CONSTRAINT memories_kind_fields CHECK (
        (skill_name IS NULL) = (skill_description IS NULL)
        AND (skill_name IS NULL OR (kind = 'skill' AND NOT retirement))
        AND (document_title IS NULL OR (kind = 'document' AND NOT retirement))
        AND (test_shape IS NULL OR (kind = 'test' AND NOT retirement
                                    AND test_shape IN ('recall', 'guardrail', 'skill', 'document')))
    ) NOT VALID;

ALTER TABLE memories DROP CONSTRAINT memories_status_check;
ALTER TABLE memories ADD CONSTRAINT memories_status_check
    CHECK (status IN ('held', 'promoted', 'demoted', 'rejected', 'erased', 'withdrawn')) NOT VALID;

-- A withdrawn version's postings are off, as a rejected version's are
-- (ADR-0052 d1, d7; ADR-0083 d13).
CREATE OR REPLACE FUNCTION oml_search_state(status text, is_tip boolean, retirement boolean, kind text)
    RETURNS smallint
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    RETURN CASE
        WHEN retirement OR kind IS NULL OR kind NOT IN ('memory', 'derived_fact') THEN NULL
        WHEN status = 'held' THEN 0
        WHEN status = 'promoted' AND is_tip THEN 1
        WHEN status IN ('promoted', 'demoted', 'rejected', 'withdrawn') THEN 2
        ELSE NULL
    END;

CREATE TABLE document_passages (
    tenant_id    uuid    NOT NULL,
    memory_id    uuid    NOT NULL,
    ordinal      integer NOT NULL CHECK (ordinal >= 0 AND ordinal < 4096),
    start_line   integer NOT NULL CHECK (start_line >= 1),
    end_line     integer NOT NULL CHECK (end_line >= start_line),
    heading_path text[]  NOT NULL DEFAULT '{}',
    fragment     text    NOT NULL,
    CONSTRAINT document_passages_pkey PRIMARY KEY (tenant_id, memory_id, ordinal)
);

COMMENT ON TABLE document_passages IS
    'The passages of a document version, cut at capture and never signed; erased with the body '
    '(ADR-0081 d3). Owning page: docs/data/postgres-schema.md.';

ALTER TABLE document_passages ENABLE ROW LEVEL SECURITY;
ALTER TABLE document_passages FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON document_passages
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

-- The serving role writes a version's passages at capture, reads them, and
-- deletes them when the body is erased.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, DELETE ON document_passages TO %I',
                   current_setting('oml.serving_role'));
END $$;
