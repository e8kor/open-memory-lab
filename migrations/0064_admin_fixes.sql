-- Admin fixes and the fix mark on memories (ADR-0089 d2, d3, d7, d8, d15,
-- d16, d17; #422).
--
-- admin_fixes: one row per fix an admin key used by a person took through
-- POST /v2/admin/fixes, folded from admin.fix in the commit that also
-- journals the lifecycle event the fix caused (d16). act is demote, edit,
-- retire or erase; release and import rollbacks wait for releases
-- (ADR-0085) and are refused by the schema until then. target_id is the
-- version the request named; served_id the version the fix made the
-- claim's served tip, where it made one (a demotion's restored version, an
-- edit's version, a retirement); replaced_id the version it took out of
-- service. The reason is stored in clear, as the signed event carries it
-- (ADR-0005 d3 as amended by ADR-0090 d7). state moves once, from
-- unconfirmed to confirmed or reverted, by admin.fix_confirmed or
-- admin.fix_reverted (d8); an erasure is never reverted.
--
-- memories.admin_fix_id names the fix a version is served because of, and
-- memories.admin_fix_confirmed and admin_fix_same_user carry what a reader
-- is shown of its confirmation (d7, d9): false and false until a person
-- confirms it. A version served by a verdict has all three NULL.
--
-- The rows are derived: the journal's fix, confirm and revert events
-- rebuild them (ADR-0090 d18).
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.
--
-- An admin key is linkable to its person (ADR-0085 d16, the operator's
-- decision of 2026-11-19): api_keys_linked_user drops its role <> 'admin'
-- clause. memories.admin_fix_reverted marks a version a reverted fix served
-- (ADR-0089 d17).
--
-- What it costs a live database: one new, empty table with one index, its
-- policy and its update guard, four nullable columns on memories with no
-- default, which rewrite no row, and the api_keys check replaced and
-- validated, one scan of api_keys under its SHARE UPDATE EXCLUSIVE lock. ALTER TABLE memories takes ACCESS EXCLUSIVE on memories
-- and its partitions for the catalog change only; the foreign key to
-- tenants takes SHARE ROW EXCLUSIVE on tenants until the commit. A lock is
-- waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

CREATE TABLE admin_fixes (
    tenant_id          uuid        NOT NULL REFERENCES tenants (id),
    id                 uuid        NOT NULL,
    act                text        NOT NULL CHECK (act IN ('demote', 'edit', 'retire', 'erase')),
    target_id          uuid        NOT NULL,
    workspace_id       uuid        NOT NULL,
    claim_key          text        NOT NULL,
    replaced_id        uuid,
    served_id          uuid,
    reason             text        NOT NULL CHECK (reason <> ''),
    loosens_guardrail  boolean     NOT NULL,
    admin_key_id       uuid        NOT NULL,
    lifecycle_event_id uuid        NOT NULL,
    state              text        NOT NULL DEFAULT 'unconfirmed'
                                   CHECK (state IN ('unconfirmed', 'confirmed', 'reverted')),
    fixed_at           timestamptz NOT NULL,
    fixed_seq          bigint      NOT NULL,
    settled_by_key_id  uuid,
    settled_same_user  boolean,
    settled_at         timestamptz,
    settled_seq        bigint,
    restored_id        uuid,
    PRIMARY KEY (tenant_id, id),
    CHECK ((state = 'unconfirmed') = (settled_at IS NULL)),
    CHECK ((settled_at IS NULL) = (settled_seq IS NULL)),
    CHECK ((settled_at IS NULL) = (settled_by_key_id IS NULL)),
    CHECK (act <> 'erase' OR state <> 'reverted')
);
COMMENT ON TABLE admin_fixes IS
    'An admin''s marked break-glass fix and its confirmation or revert (ADR-0089 d3, d8, d16). '
    'Owning page: docs/data/postgres-schema.md.';
CREATE INDEX admin_fixes_unconfirmed ON admin_fixes (tenant_id, fixed_at)
    WHERE state = 'unconfirmed';

ALTER TABLE admin_fixes ENABLE ROW LEVEL SECURITY;
ALTER TABLE admin_fixes FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON admin_fixes
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE memories
    ADD COLUMN admin_fix_id        uuid,
    ADD COLUMN admin_fix_confirmed boolean,
    ADD COLUMN admin_fix_same_user boolean;
COMMENT ON COLUMN memories.admin_fix_id IS
    'The admin fix this version is served because of; NULL for a version a verdict promoted (ADR-0089 d7).';
COMMENT ON COLUMN memories.admin_fix_confirmed IS
    'Whether a person confirmed the fix that serves this version: approver.confirmed (ADR-0089 d7).';
COMMENT ON COLUMN memories.admin_fix_same_user IS
    'Whether that confirmation was by the admin''s own person, in a development scope: approver.same_user (ADR-0089 d7, d8).';

ALTER TABLE memories ADD COLUMN admin_fix_reverted boolean;
COMMENT ON COLUMN memories.admin_fix_reverted IS
    'Whether a person reverted the fix that served this version: approver.reverted (ADR-0089 d7, d17; the decision of 2026-11-19).';

-- The serving role inserts a fix and settles it once; it never rewrites what
-- the fix recorded (ADR-0089 d3, d8). Column grants keep the recorded
-- columns out of its reach, and the guard below keeps the state moving once,
-- from unconfirmed to confirmed or reverted, owner-owned and SECURITY
-- INVOKER as 0057's guard is.
CREATE OR REPLACE FUNCTION oml_admin_fixes_guard() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF OLD.state <> 'unconfirmed' OR NEW.state NOT IN ('confirmed', 'reverted') THEN
        RAISE EXCEPTION 'admin_fixes_guard: a fix moves once, from unconfirmed to confirmed or reverted'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER admin_fixes_guard
    BEFORE UPDATE ON admin_fixes
    FOR EACH ROW EXECUTE FUNCTION oml_admin_fixes_guard();

COMMENT ON FUNCTION oml_admin_fixes_guard() IS
    'Refuses an update of admin_fixes that does not move state once, from unconfirmed to '
    'confirmed or reverted (0064; ADR-0089 d8).';

-- An admin key is linked to its person (ADR-0085 d16, the operator's decision
-- of 2026-11-19): the check keeps the link off session keys and user keys
-- only. The new check is looser than 0050's, so every stored row meets it;
-- it is added NOT VALID and validated in this transaction, a scan of
-- api_keys, which holds one row per key.
ALTER TABLE api_keys DROP CONSTRAINT api_keys_linked_user;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_linked_user CHECK (
        linked_user_id IS NULL
        OR (user_root_id IS NULL AND credential <> 'session')
    ) NOT VALID;
ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_linked_user;

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT ON admin_fixes TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT UPDATE (state, settled_by_key_id, settled_same_user, settled_at, '
                   'settled_seq, restored_id) ON admin_fixes TO %I',
                   current_setting('oml.serving_role'));
END $$;
