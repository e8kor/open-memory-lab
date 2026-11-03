-- 0011_claim_edges — a labelled, ratifiable edge between two claims, keyed
-- inside its tenant (ADR-0008 d5, ADR-0021, ADR-0012 R2). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0009 are never edited,
-- a correction is the next numbered file. 0010 is reserved for the workarea
-- transitions lane and is left unused here. Normative shape:
-- docs/data/postgres-schema.md "claim_edges", under "workareas and edges".
--
-- The columns are the schema page's: an edge is 'pending' until a ratifier
-- that is not its proposer ratifies it, and nothing deletes one. No
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
-- Reading (not decided by ADR-0034): the table is not partitioned. d11
-- partitions memories alone, for its FTS index's statistics; claim_edges
-- holds no indexed text, and its label index's statistics name labels, not
-- claims.
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

-- An endpoint's edges, either way; the ratified edges of one label, the
-- cycle refusal's read (ADR-0021).
CREATE INDEX claim_edges_src ON claim_edges (tenant_id, src_artifact_id);
CREATE INDEX claim_edges_dst ON claim_edges (tenant_id, dst_artifact_id);
CREATE INDEX claim_edges_ratified_label ON claim_edges (tenant_id, label)
    WHERE state = 'ratified';

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
