-- 0019_brief_edges — a claim joined to one workarea's brief, pending until a
-- key that is not its proposer ratifies it (ADR-0046 d7, d8). Forward-only
-- (docs/data/postgres-schema.md "Migrations"); numbered after
-- 0018_claim_edges, its v0.5.2 sibling. Normative shape:
-- docs/data/postgres-schema.md "brief_edges".
--
-- A brief edge is apart from claim_edges because it joins a claim to a
-- workarea, not two claims, and widens no lineage closure: membership in a
-- brief says where a claim is read first, not what it derives from.
-- ratified_seq is the seq of the review.ratify that ratified it, the
-- brief's order key; created_at is its brief_edge.proposed event's at, the
-- listing's order key. The label is fixed, 'brief', and is not stored.
--
-- Every key into a tenant table carries tenant_id (ADR-0034 d12), and the
-- table keeps no column statistics, as 0018 does for claim_edges and for
-- the same reason (FR-45): a workarea or claim id is a tenant's fact.

CREATE TABLE brief_edges (
    id              uuid NOT NULL,
    tenant_id       uuid NOT NULL,
    artifact_id     uuid NOT NULL,
    workarea_id     uuid NOT NULL,
    state           text NOT NULL,
    proposer_key_id uuid NOT NULL,
    ratifier_key_id uuid,
    ratified_seq    bigint,
    created_at      timestamptz(6) NOT NULL DEFAULT now(),
    CONSTRAINT brief_edges_pkey PRIMARY KEY (id),
    CONSTRAINT brief_edges_tenant_id_id UNIQUE (tenant_id, id),
    CONSTRAINT brief_edges_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT brief_edges_artifact_id_fkey
        FOREIGN KEY (tenant_id, artifact_id) REFERENCES memories (tenant_id, id),
    CONSTRAINT brief_edges_workarea_id_fkey
        FOREIGN KEY (tenant_id, workarea_id) REFERENCES workareas (tenant_id, id),
    CONSTRAINT brief_edges_proposer_key_id_fkey
        FOREIGN KEY (tenant_id, proposer_key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT brief_edges_ratifier_key_id_fkey
        FOREIGN KEY (tenant_id, ratifier_key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT brief_edges_state
        CHECK (state IN ('pending', 'ratified')),
    CONSTRAINT brief_edges_ratifier_is_not_proposer
        CHECK (ratifier_key_id IS NULL OR ratifier_key_id <> proposer_key_id),
    CONSTRAINT brief_edges_ratified_has_ratifier
        CHECK ((state = 'ratified') = (ratifier_key_id IS NOT NULL)),
    CONSTRAINT brief_edges_ratified_has_seq
        CHECK ((state = 'ratified') = (ratified_seq IS NOT NULL))
);

ALTER TABLE brief_edges
    ALTER COLUMN id              SET STATISTICS 0,
    ALTER COLUMN tenant_id       SET STATISTICS 0,
    ALTER COLUMN artifact_id     SET STATISTICS 0,
    ALTER COLUMN workarea_id     SET STATISTICS 0,
    ALTER COLUMN state           SET STATISTICS 0,
    ALTER COLUMN proposer_key_id SET STATISTICS 0,
    ALTER COLUMN ratifier_key_id SET STATISTICS 0,
    ALTER COLUMN ratified_seq    SET STATISTICS 0,
    ALTER COLUMN created_at      SET STATISTICS 0;

-- A workarea's brief edges, the brief's and the listing's one read.
CREATE INDEX brief_edges_workarea ON brief_edges (tenant_id, workarea_id, created_at DESC, id DESC);

ALTER TABLE brief_edges ENABLE ROW LEVEL SECURITY;
ALTER TABLE brief_edges FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON brief_edges
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE brief_edges IS
    'Claims joined to a workarea''s brief, pending until ratified by a key that is not the proposer; '
    'never deleted. Owning page: docs/data/postgres-schema.md.';

-- The proposal's INSERT, the ratification's UPDATE and the reads; no DELETE.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON brief_edges TO %I',
                   current_setting('oml.serving_role'));
END $$;
