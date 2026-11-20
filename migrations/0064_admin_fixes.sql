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
-- (ADR-0005 d3 as amended by ADR-0090 d7). admin_person is the person the
-- admin key acted for at the fix, which the settlement compares, never the
-- key's link as it stands then (the operator's decision of 2026-11-19).
-- state moves once, from unconfirmed to confirmed or reverted, by
-- admin.fix_confirmed or admin.fix_reverted (d8), or to superseded when a
-- later fix takes over the mark on the version it served, superseded_by
-- naming that fix (d17, the reading of 2026-11-19); an erasure is never
-- reverted.
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
-- clause. A console admin's link waits on its person's acceptance (the
-- operator's decision of 2026-11-19 on key-to-person links):
-- api_keys.link_pending_user_id names the user root until the person
-- accepts it from their own console session, key.link_accepted, which
-- moves it to linked_user_id. memories.admin_fix_reverted marks a version a
-- reverted fix served (ADR-0089 d17).
--
-- Links made before this migration wait on their person too: every linked
-- key but the bootstrap ceremony's first admin key (issued_by NULL: no key
-- issues it, and its person runs the ceremony) moves its linked_user_id to
-- link_pending_user_id, so its person accepts it before it acts for them.
-- api_keys_link_guard, owner-owned and SECURITY INVOKER as 0057's guard is,
-- admits a change of linked_user_id only to NULL (an unlink), from the
-- link pending to the same root (an acceptance), or on the ceremony's own
-- key while it holds no link and is its tenant's only key (the bootstrap's),
-- and refuses any change of issued_by, which marks that key. On insert it
-- refuses a link or a pending link (the operator's decision of 2026-11-20).
--
-- An admin can no longer quietly become a person who signs in already (the
-- operator's decision of 2026-11-19, ADR-0045 d2's note):
-- web_emails.pending_since marks an entry an admin added, or changed to
-- name them, for such a person; it signs in to nothing until that person
-- accepts it from a session they already have, web.email_accepted, which
-- clears it. A session opens only on an entry with pending_since NULL.
--
-- A database that applied an earlier draft of 0063 or 0064 (a stage or
-- development database built from the #498 branch before 2026-11-20's fourth
-- round) is reset and migrated again: sqlx records each file's checksum, and
-- an edited file no longer matches it. No released database holds either:
-- v0.8.3 ships 0065 without them, and they run after it on upgrade.
--
-- What it costs a live database: one new, empty table with one index, its
-- policy and its update guard, four nullable columns on memories, one on
-- api_keys and one on web_emails with no default, which rewrite no row, an
-- insert and update guard on api_keys, the linked keys' rows moved per
-- tenant, and the api_keys checks replaced and added NOT VALID, then
-- validated. Every lock below is held until the migration commits, as any
-- lock a transaction takes is. ALTER TABLE memories takes ACCESS EXCLUSIVE
-- on memories and its partitions; its change is to the catalog alone, so it
-- scans no row, but reads and writes of memories wait to the commit. The
-- DROP CONSTRAINT on api_keys takes ACCESS EXCLUSIVE on api_keys, so the
-- move of the linked keys' rows and both VALIDATEs scan api_keys under it:
-- the fourth review measured about 400 ms at 1,000,000 keys of which 10,000
-- are linked, the move's scan included, so a key lookup waits about that
-- long; a tenant holds hundreds of keys. ALTER TABLE web_emails takes
-- ACCESS EXCLUSIVE on web_emails, a catalog change, and sign-ins wait to the
-- commit. The foreign key to tenants takes SHARE ROW EXCLUSIVE on tenants.
-- A lock is waited for at most 5 s.

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
    admin_person       text        NOT NULL CHECK (admin_person <> ''),
    lifecycle_event_id uuid        NOT NULL,
    state              text        NOT NULL DEFAULT 'unconfirmed'
                                   CHECK (state IN ('unconfirmed', 'confirmed', 'reverted',
                                                    'superseded')),
    fixed_at           timestamptz NOT NULL,
    fixed_seq          bigint      NOT NULL,
    settled_by_key_id  uuid,
    settled_same_user  boolean,
    settled_at         timestamptz,
    settled_seq        bigint,
    restored_id        uuid,
    superseded_by      uuid,
    PRIMARY KEY (tenant_id, id),
    CHECK ((state = 'unconfirmed') = (settled_at IS NULL)),
    CHECK ((settled_at IS NULL) = (settled_seq IS NULL)),
    CHECK ((state IN ('unconfirmed', 'superseded')) = (settled_by_key_id IS NULL)),
    CHECK ((state = 'superseded') = (superseded_by IS NOT NULL)),
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
-- from unconfirmed to confirmed, reverted or superseded, owner-owned and
-- SECURITY INVOKER as 0057's guard is.
CREATE OR REPLACE FUNCTION oml_admin_fixes_guard() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF OLD.state <> 'unconfirmed'
       OR NEW.state NOT IN ('confirmed', 'reverted', 'superseded') THEN
        RAISE EXCEPTION 'admin_fixes_guard: a fix moves once, from unconfirmed to confirmed, reverted or superseded'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER admin_fixes_guard
    BEFORE UPDATE ON admin_fixes
    FOR EACH ROW EXECUTE FUNCTION oml_admin_fixes_guard();

COMMENT ON FUNCTION oml_admin_fixes_guard() IS
    'Refuses an update of admin_fixes that does not move state once, from unconfirmed to '
    'confirmed, reverted or superseded (0064; ADR-0089 d8, d17).';

-- An admin key is linked to its person (ADR-0085 d16, the operator's decision
-- of 2026-11-19): the check keeps the link off session keys and user keys
-- only. The new check is looser than 0050's, so every stored row meets it;
-- it is added NOT VALID and validated in this transaction. The DROP
-- CONSTRAINT holds ACCESS EXCLUSIVE on api_keys to the commit, so the
-- VALIDATE scans api_keys, which holds one row per key, under that lock.
ALTER TABLE api_keys DROP CONSTRAINT api_keys_linked_user;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_linked_user CHECK (
        linked_user_id IS NULL
        OR (user_root_id IS NULL AND credential <> 'session')
    ) NOT VALID;
-- lint:allow validate the new check is looser than the one it replaces, so no stored row fails it, and api_keys holds one row per key
ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_linked_user;

-- A console admin's link waits on its person (the operator's decision of
-- 2026-11-19 on key-to-person links): the user root it names, until that
-- person accepts it. A key holds a link or a pending one, never both, and
-- neither on a session key or a user key. The check is validated after the
-- move below, under the same lock.
ALTER TABLE api_keys ADD COLUMN link_pending_user_id uuid;
COMMENT ON COLUMN api_keys.link_pending_user_id IS
    'The user root a console admin linked this key to, until that person accepts the link; the '
    'key acts for no one meanwhile (ADR-0085 d16, the operator''s decision of 2026-11-19).';

-- Links made before this migration wait on their person as a console
-- admin's link does (the operator's decision of 2026-11-19): every linked key
-- but the bootstrap ceremony's first admin key, which no key issues, moves its
-- link to pending, one tenant at a time under that tenant's row security.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE api_keys
           SET link_pending_user_id = linked_user_id,
               linked_user_id = NULL
         WHERE tenant_id = t
           AND linked_user_id IS NOT NULL
           AND issued_by IS NOT NULL;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END $$;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_link_pending CHECK (
        link_pending_user_id IS NULL
        OR (linked_user_id IS NULL AND user_root_id IS NULL AND credential <> 'session')
    ) NOT VALID;
-- lint:allow validate a row the move set was a linked key, so its user_root_id is NULL and it is no session key as api_keys_linked_user held, every other row has the column NULL, and api_keys holds one row per key
ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_link_pending;

-- A key comes to act for a person only through the person (the operator's
-- decision of 2026-11-19): an update that sets linked_user_id is an unlink to
-- NULL, an acceptance that moves the link pending to the same root, or the
-- bootstrap ceremony's link of its own first admin key, which no key issues,
-- while that key is its tenant's only key, as it is at the ceremony; issued_by,
-- which marks that key, never changes. An insert carries no link and no
-- pending link, which only an update sets (the operator's decision of
-- 2026-11-20); an admin key no key issues and inserted beside other keys is
-- never its tenant's only key, so the ceremony's exception never reaches it. What stays open: the serving role sets
-- link_pending_user_id, hash and web_emails.pending_since without a guard of
-- the database (ADR-0045's limits). Owner-owned and SECURITY INVOKER, as
-- 0057's guard is; the tenant's other keys are read under its row security.
CREATE OR REPLACE FUNCTION oml_api_keys_link_guard() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.linked_user_id IS NOT NULL OR NEW.link_pending_user_id IS NOT NULL THEN
            RAISE EXCEPTION 'api_keys_link_guard: a key is inserted with no link; its person accepts one later'
                USING ERRCODE = 'check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.issued_by IS DISTINCT FROM OLD.issued_by THEN
        RAISE EXCEPTION 'api_keys_link_guard: a key''s issuer never changes'
            USING ERRCODE = 'check_violation';
    END IF;
    IF NEW.linked_user_id IS NOT DISTINCT FROM OLD.linked_user_id
       OR NEW.linked_user_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF OLD.linked_user_id IS NULL
       AND OLD.link_pending_user_id = NEW.linked_user_id
       AND NEW.link_pending_user_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF OLD.linked_user_id IS NULL AND OLD.link_pending_user_id IS NULL
       AND OLD.issued_by IS NULL AND OLD.role = 'admin'
       AND NOT EXISTS (SELECT 1 FROM api_keys k
                        WHERE k.tenant_id = OLD.tenant_id AND k.id <> OLD.id) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'api_keys_link_guard: a key acts for a person only once that person accepts its link'
        USING ERRCODE = 'check_violation';
END $$;

CREATE TRIGGER api_keys_link_guard
    BEFORE INSERT OR UPDATE OF linked_user_id, issued_by ON api_keys
    FOR EACH ROW EXECUTE FUNCTION oml_api_keys_link_guard();

COMMENT ON FUNCTION oml_api_keys_link_guard() IS
    'Refuses an insert of api_keys with a link or a pending link; an update that sets linked_user_id other than to '
    'NULL, from the link pending to the same root, or on the bootstrap ceremony''s first admin '
    'key while it holds no link and is its tenant''s only key; and any change of issued_by '
    '(0064; ADR-0085 d16, the operator''s decisions of 2026-11-19 and 2026-11-20).';

-- A sign-in email for a person who signs in already waits on that person
-- (the operator's decision of 2026-11-19, ADR-0045 d2's note): the time it
-- began to wait, NULL once accepted or for an entry that never waited.
ALTER TABLE web_emails ADD COLUMN pending_since timestamptz;
COMMENT ON COLUMN web_emails.pending_since IS
    'When an admin added this entry, or changed it to name its user, for a person who signs in '
    'already; it signs in to nothing until that person accepts it, and is NULL after '
    '(ADR-0045 d2, the operator''s decision of 2026-11-19).';

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT ON admin_fixes TO %I',
                   current_setting('oml.serving_role'));
    EXECUTE format('GRANT UPDATE (state, settled_by_key_id, settled_same_user, settled_at, '
                   'settled_seq, restored_id, superseded_by) ON admin_fixes TO %I',
                   current_setting('oml.serving_role'));
END $$;
