-- 0070_admin_acts — every admin act is one journalled record; a widening
-- act's key waits on a second admin person; a tenant knows whether it ever
-- had two live admin persons, and how old each admin person's flag is
-- (ADR-0097 d1, d2, d5, d10, d11, d23).
--
-- admin_acts: one row per admin act, folded from admin.act. The first
-- envelope inserts the row in the commit of the act's effect; each later
-- one, with the same act_id, moves its state: pending_confirmation to
-- confirmed, declined or (by its life) expired, effective to declined for a
-- sole_admin act (d10). class is fixed per act in code and stored so the
-- doctor and To do read it without the code's table. reason is stored in
-- clear, as the signed event carries it (d26). actor_person is the person
-- the acting key counted as at the act, NULL where it counted as no one
-- (until d21 leaves no such admin key). effect holds what a confirmation
-- applies for an act still pending, a pending key's grants among it: the
-- table of their own d23 asks for, which no recall, review-queue, count or
-- row-security query reads, so no pending grant is in force before a
-- second admin person confirms it. The rows are derived: the journal's
-- admin.act events rebuild them (ADR-0090 d18).
--
-- api_keys.pending_act_id names the widening act a key waits on, from its
-- key.issued to the act's confirmation, which clears it. A key holding one
-- authenticates nothing on any path (d23); the check reads the row the
-- lookup already reads, so authentication adds no store call (the
-- bottleneck register's "Authenticate" row).
--
-- web_emails.admin_since is when the entry's admin flag was accepted: the
-- add of an unpending admin entry, a change that flags an unpending entry,
-- or the person's acceptance of a pending one. An admin person counts as a
-- second admin person only for an act at least seven days after it (d11).
-- It is NULL for an entry with no admin flag and for a pending one. Live
-- entries are backfilled from the journal's last web.email_allowed,
-- web.email_changed or web.email_accepted naming the entry's digest with
-- the flag, and from created_at where the journal names none.
--
-- tenants.sole_admin is true for a tenant that has never had two live admin
-- persons (d10), folded from the admin-entry events, never from the state
-- alone: the backfill reads the journal, and sets it false for a tenant
-- whose journal ever flagged two different users admin
-- (web.email_allowed, web.email_changed or web.email_accepted with admin
-- true). That counts a flag a person never accepted as live, which only
-- ever takes a tenant out of sole_admin: it fails closed.
--
-- What it costs a live database: one new, empty table with two indexes and
-- its policy; one nullable column on api_keys and one on web_emails with no
-- default, catalog changes that rewrite no row; one column on tenants with a
-- constant default, which PostgreSQL 11 and later add without a rewrite.
-- The backfills read the journal's web.email_* events once, a scan of the
-- journal by event_type: tenants hold tens of such events. ALTER TABLE
-- takes ACCESS EXCLUSIVE on each table until the commit, so key lookups and
-- sign-ins wait for the migration.

SET LOCAL lock_timeout = '5s';

CREATE TABLE admin_acts (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    id uuid NOT NULL,
    act text NOT NULL,
    class text NOT NULL CHECK (class IN ('narrowing', 'erase', 'widening', 'consent',
                                         'against_admin', 'neutral')),
    target jsonb NOT NULL,
    reason text,
    actor_key_id uuid,
    actor_person oml_name,
    state text NOT NULL CHECK (state IN ('effective', 'pending_confirmation', 'confirmed',
                                         'declined', 'expired', 'contested', 'settled')),
    confirmed_by_person oml_name,
    settled_by_key_id uuid,
    sole_admin boolean NOT NULL,
    cause text CHECK (cause IN ('upgrade', 'deactivation', 'settlement', 'recovery')),
    created_at timestamptz NOT NULL,
    settled_at timestamptz,
    effect jsonb NOT NULL DEFAULT '{}'::jsonb,
    seq bigint NOT NULL,
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT admin_acts_actor CHECK (actor_key_id IS NOT NULL OR cause IS NOT NULL)
);
CREATE INDEX admin_acts_state ON admin_acts (tenant_id, state, created_at);
CREATE INDEX admin_acts_actor_person ON admin_acts (tenant_id, actor_person, created_at);
COMMENT ON TABLE admin_acts IS
    'One row per admin act, folded from admin.act: its class, state, reason and actor person (ADR-0097 d2).';
COMMENT ON COLUMN admin_acts.effect IS
    'What a confirmation applies for an act still pending, a pending key''s grants among it; read by no recall, review or count query (ADR-0097 d23).';

ALTER TABLE admin_acts ENABLE ROW LEVEL SECURITY;
ALTER TABLE admin_acts FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON admin_acts
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE api_keys ADD COLUMN pending_act_id uuid;
COMMENT ON COLUMN api_keys.pending_act_id IS
    'The widening admin act this key waits on; it authenticates nothing until a second admin person confirms it (ADR-0097 d23).';

ALTER TABLE web_emails ADD COLUMN admin_since timestamptz;
COMMENT ON COLUMN web_emails.admin_since IS
    'When the entry''s admin flag was accepted; an admin person counts as a second one only for an act seven days later (ADR-0097 d11).';

ALTER TABLE tenants ADD COLUMN sole_admin boolean NOT NULL DEFAULT true;
COMMENT ON COLUMN tenants.sole_admin IS
    'True while the tenant has never had two live admin persons, folded from the admin-entry events (ADR-0097 d10).';

UPDATE web_emails w
   SET admin_since = coalesce(
       (SELECT max(j.at) FROM journal j
         WHERE j.tenant_id = w.tenant_id
           AND j.event_type IN ('web.email_allowed', 'web.email_changed', 'web.email_accepted')
           AND j.payload ->> 'email_digest' = 'sha256:' || encode(w.email_hash, 'hex')
           AND (j.payload ->> 'admin')::boolean),
       w.created_at)
 WHERE w.admin AND w.pending_since IS NULL;

UPDATE tenants t
   SET sole_admin = false
 WHERE (SELECT count(DISTINCT j.payload ->> 'user') FROM journal j
         WHERE j.tenant_id = t.id
           AND j.event_type IN ('web.email_allowed', 'web.email_changed', 'web.email_accepted')
           AND (j.payload ->> 'admin')::boolean) >= 2;

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON admin_acts TO %I',
                   current_setting('oml.serving_role'));
END $$;
