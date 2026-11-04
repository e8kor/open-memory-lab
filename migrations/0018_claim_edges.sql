-- 0018_claim_edges — a labelled, ratifiable edge between two claims, keyed
-- inside its tenant (ADR-0008 d5, ADR-0021, ADR-0012 R2). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0009 are never edited,
-- a correction is the next numbered file. Numbered after v0.5.1's 0016 and
-- after 0017_workarea_transitions, its v0.5.2 sibling. Normative shape:
-- docs/data/postgres-schema.md "claim_edges", under "workareas and edges".
--
-- The columns are the schema page's: an edge is 'pending' until a ratifier
-- that is not its proposer ratifies it, and nothing deletes one. created_at
-- is its claim_edge.proposed event's at, the order key of the graph read,
-- which lists newest first as every list endpoint does (http-api.md
-- pagination). No
-- 'rejected' state and no term_curie column: both belong to ADR-0027, which
-- is not accepted, so the domain's EdgeState::Rejected is unused by the
-- store. The label is free text here; its admissibility (ADR-0021 d5) is
-- the domain's, decided before any write.
--
-- Every key into a tenant table carries tenant_id (ADR-0034 d12, 0008's
-- shape): a foreign-key check runs without row-level security, so a key on
-- id alone would accept another tenant's memory or key and answer whether
-- that id exists anywhere. Under the pair, a cross-tenant reference is
-- refused as one to no row is: 23503, from the one constraint. The keys
-- into api_keys are MATCH SIMPLE, and a NOT NULL tenant_id never matches a
-- provider-level key, so an operator key neither proposes nor ratifies.
--
-- The key-inequality CHECK is R2's cheap spelling on edges; the full rule,
-- ratifier principal distinct from proposer principal and from either
-- endpoint's lineage, is the ratification transaction's (ADR-0012).
--
-- Reading (not decided by ADR-0034): the table is not partitioned, and it
-- keeps no column statistics. A label is tenant text: the domain admits any
-- non-empty NFC label outside oml: (ADR-0021 d5), such as
-- 'acme:merger_target'. Over one unpartitioned table, a column's statistics
-- span every tenant, and EXPLAIN's estimate reads them under row security
-- (texteq and uuid_eq are leakproof), so a session set to tenant A could
-- learn whether another tenant uses a label or holds an endpoint id: the
-- FR-45 channel d11 closed for memories by partitions. Statistics target 0
-- on every column, set before any ANALYZE, closes it for the estimate:
-- ANALYZE collects nothing, so each predicate takes PostgreSQL's default
-- selectivity whatever any tenant stores (ADR-0034's alternatives table
-- lists target 0 as FR-45's candidate). The cost: the planner never learns
-- a label's or an endpoint's skew, so edge reads plan on defaults. What
-- stays open is FR-45's: pg_class's reltuples and relpages count every
-- tenant's edges together, readable by any role.
--
-- Run by the owner role. The serving role's grant is the block below, named
-- through oml.serving_role as 0007's are (postgres-schema.md "Privileges").

CREATE TABLE claim_edges (
    id              uuid NOT NULL,
    tenant_id       uuid NOT NULL,
    src_artifact_id uuid NOT NULL,
    dst_artifact_id uuid NOT NULL,
    label           text NOT NULL,
    state           text NOT NULL,
    proposer_key_id uuid NOT NULL,
    ratifier_key_id uuid,
    created_at      timestamptz(6) NOT NULL DEFAULT now(),
    CONSTRAINT claim_edges_pkey PRIMARY KEY (id),
    CONSTRAINT claim_edges_tenant_id_id UNIQUE (tenant_id, id),
    CONSTRAINT claim_edges_tenant_id_fkey
        FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT claim_edges_src_artifact_id_fkey
        FOREIGN KEY (tenant_id, src_artifact_id) REFERENCES memories (tenant_id, id),
    CONSTRAINT claim_edges_dst_artifact_id_fkey
        FOREIGN KEY (tenant_id, dst_artifact_id) REFERENCES memories (tenant_id, id),
    CONSTRAINT claim_edges_proposer_key_id_fkey
        FOREIGN KEY (tenant_id, proposer_key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT claim_edges_ratifier_key_id_fkey
        FOREIGN KEY (tenant_id, ratifier_key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT claim_edges_state
        CHECK (state IN ('pending', 'ratified')),
    CONSTRAINT claim_edges_ratifier_is_not_proposer
        CHECK (ratifier_key_id IS NULL OR ratifier_key_id <> proposer_key_id),
    CONSTRAINT claim_edges_ratified_has_ratifier
        CHECK ((state = 'ratified') = (ratifier_key_id IS NOT NULL))
);

-- No column statistics, before any ANALYZE can collect them: a target
-- lowered after one leaves the earlier statistics in place.
ALTER TABLE claim_edges
    ALTER COLUMN id              SET STATISTICS 0,
    ALTER COLUMN tenant_id       SET STATISTICS 0,
    ALTER COLUMN src_artifact_id SET STATISTICS 0,
    ALTER COLUMN dst_artifact_id SET STATISTICS 0,
    ALTER COLUMN label           SET STATISTICS 0,
    ALTER COLUMN state           SET STATISTICS 0,
    ALTER COLUMN proposer_key_id SET STATISTICS 0,
    ALTER COLUMN ratifier_key_id SET STATISTICS 0,
    ALTER COLUMN created_at      SET STATISTICS 0;

-- An endpoint's edges, either way; the ratified edges of one label, the
-- cycle refusal's read (ADR-0021).
CREATE INDEX claim_edges_src ON claim_edges (tenant_id, src_artifact_id);
CREATE INDEX claim_edges_dst ON claim_edges (tenant_id, dst_artifact_id);
CREATE INDEX claim_edges_ratified_label ON claim_edges (tenant_id, label)
    WHERE state = 'ratified';
-- The graph read's order, newest first (http-api.md pagination).
CREATE INDEX claim_edges_listing ON claim_edges (tenant_id, created_at DESC, id DESC);

ALTER TABLE claim_edges ENABLE ROW LEVEL SECURITY;
ALTER TABLE claim_edges FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON claim_edges
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE claim_edges IS
    'Labelled claim-to-claim edges, pending until ratified by a key that is not the proposer; '
    'never deleted. Owning page: docs/data/postgres-schema.md.';

-- claim_edges: the proposal's INSERT, the ratification's UPDATE and the
-- reads; no DELETE, because nothing deletes an edge.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON claim_edges TO %I',
                   current_setting('oml.serving_role'));
END $$;
