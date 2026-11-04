-- 0016_workarea_transitions — the status ladder's pending transitions
-- (ADR-0009 d1, d3). Forward-only (docs/data/postgres-schema.md
-- "Migrations"): 0001–0009 are never edited, a correction is the next
-- numbered file. Normative shape: docs/data/postgres-schema.md "workareas,
-- workarea_edges", the workarea_transitions sketch, and the build's readings
-- on docs/architecture/workareas.md.
--
-- A node's status is one column, workareas.status, which 0001 created with
-- its CHECK over the three rungs; the lead now writes it at create
-- ('development', or 'committed' on the universe root) and reads it back,
-- and a ratified transition is the only other write. This file adds the
-- proposals: one row per workarea.status_proposed, 'pending' until a second
-- identity ratifies it. Several pending rows may share a workarea; the
-- ratifying transaction, under the chain lock with the node and the row
-- locked, re-checks from_status against workareas.status (a stale row is
-- refused and stays 'pending') and, on a step to 'validated', that no direct
-- child stands on another rung.
--
-- The universe root is permanently 'committed' (ADR-0009 d1). Now that the
-- lead writes the status the record carries rather than one derived from
-- parent_id, the schema holds that rung itself: workareas_universe_committed
-- refuses a root stored on any other rung, so no create and no later
-- ratification can move it. Every root written before this file was stored
-- 'committed', so the constraint validates against the rows already held.
--
-- The references name the tenant with the id, because PostgreSQL runs a
-- foreign-key check without row-level security (0004_grants.sql): a
-- reference on id alone would accept another tenant's node or key. The
-- key-inequality CHECK is the cheap spelling of R2 (ADR-0010); the
-- principal half is checked by the store in the ratifying transaction, which
-- reads the ratifier's api_keys.principal and refuses proposer_principal,
-- since a principal can hold several keys. The proposal's fold writes
-- proposer_principal only when it is the proposing key's api_keys.principal.
--
-- Run by the owner role. The serving role's DML is granted below through
-- oml.serving_role, as 0007 grants every table's (postgres-schema.md
-- "Privileges"); a run without the setting fails at current_setting with
-- 42704 and grants nothing.

ALTER TABLE workareas ADD CONSTRAINT workareas_universe_committed
    CHECK (parent_id IS NOT NULL OR status = 'committed');

CREATE TABLE workarea_transitions (
    id                 uuid   PRIMARY KEY,           -- the events' transition_id
    tenant_id          uuid   NOT NULL,
    workarea_id        uuid   NOT NULL,
    from_status        text   NOT NULL
        CHECK (from_status IN ('development', 'committed', 'validated')),
    to_status          text   NOT NULL
        CHECK (to_status IN ('development', 'committed', 'validated')),
    state              text   NOT NULL CHECK (state IN ('pending', 'ratified')),
    proposer_key_id    uuid   NOT NULL,
    proposer_principal text   NOT NULL,              -- R2 is checked on the principal too
    ratifier_key_id    uuid,
    proposed_seq       bigint NOT NULL,              -- seq of workarea.status_proposed
    ratified_seq       bigint,                       -- seq of workarea.status_transition
    CONSTRAINT workarea_transitions_tenant_id_id UNIQUE (tenant_id, id),
    CONSTRAINT workarea_transitions_workarea
        FOREIGN KEY (tenant_id, workarea_id) REFERENCES workareas (tenant_id, id),
    CONSTRAINT workarea_transitions_proposer
        FOREIGN KEY (tenant_id, proposer_key_id) REFERENCES api_keys (tenant_id, id),
    CONSTRAINT workarea_transitions_ratifier
        FOREIGN KEY (tenant_id, ratifier_key_id) REFERENCES api_keys (tenant_id, id),
    -- One rung forward: the ladder has no other step (ADR-0009 d1).
    CONSTRAINT workarea_transitions_one_rung
        CHECK ((from_status, to_status) IN (('development', 'committed'),
                                            ('committed', 'validated'))),
    CONSTRAINT workarea_transitions_ratified_has_ratifier
        CHECK ((state = 'ratified') = (ratifier_key_id IS NOT NULL)),
    CONSTRAINT workarea_transitions_ratified_has_seq
        CHECK ((state = 'ratified') = (ratified_seq IS NOT NULL)),
    CONSTRAINT workarea_transitions_ratifier_not_proposer
        CHECK (ratifier_key_id IS NULL OR ratifier_key_id <> proposer_key_id)
);

-- The status read's pending list.
CREATE INDEX workarea_transitions_by_state
    ON workarea_transitions (tenant_id, workarea_id, state);

COMMENT ON TABLE workarea_transitions IS
    'Status-ladder proposals, pending until a second identity ratifies '
    '(ADR-0009 d3). Owning page: docs/data/postgres-schema.md.';

ALTER TABLE workarea_transitions ENABLE ROW LEVEL SECURITY;
ALTER TABLE workarea_transitions FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON workarea_transitions
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

-- workarea_transitions: the proposal's INSERT, the ratification's UPDATE
-- and the status read's pending list.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON workarea_transitions TO %I',
                   current_setting('oml.serving_role'));
END $$;
