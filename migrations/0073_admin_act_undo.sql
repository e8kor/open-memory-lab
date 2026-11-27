-- 0073_admin_act_undo — a declined admin act is undone, and a person's admin
-- keys are suspended while an act against them is open or while they stand
-- unlinked after the upgrade (ADR-0097 d8, d10, d21; the operator's
-- decisions of 2026-11-27).
--
-- grants.revoked_epoch is the seq of the grant.revoked that ended a grant:
-- the decline of the sole_admin act that added it (d10). A revoked grant
-- confers nothing and its row stays as history, as a lifted denial's does;
-- its revocation still moves the key's permission epoch. NULL for every
-- grant in force. Both reads of grants filter on it.
--
-- api_keys.suspended_cause is why a key authenticates nothing for now:
-- 'against_admin' while an act against its person is open (d8, d21),
-- 'unlinked' for an admin key that stood with no accepted link at the
-- first start of v0.8.6, which still reaches /v2/whoami and
-- POST /v2/keys/self/link (the production migration; d22).
-- suspended_act_id names the act against an admin that suspended it, NULL
-- for 'unlinked'; suspended_at is the event's instant, from which the
-- upgrade window's thirty days run. All three are NULL for a key that is
-- not suspended; the key.suspended and key.suspension_lifted events set and
-- clear them, so the journal rebuilds them (ADR-0090 d18). Authentication
-- reads them from the row the lookup already reads, so it adds no store
-- call (the bottleneck register's "Authenticate" row).
--
-- admin_acts gains a second policy, SELECT only: before any tenant is
-- known, the contest-only sign-in reads the open remove_admin act whose
-- target names the signing-in address's digest, matched on
-- oml.api_key_hash as the web_emails lookup is (0015), so a removed admin,
-- whose entry is gone, can still contest (d8; the operator's decision of
-- 2026-11-27). The partial index serves that one lookup.
--
-- What it costs a live database: four nullable columns with no default,
-- catalog changes that rewrite no row, and a policy and a partial index on
-- admin_acts, a table 0070 creates in the same release, so the index builds
-- over no live row. ALTER TABLE takes ACCESS EXCLUSIVE on grants and
-- api_keys until the commit, so key lookups wait for it.

SET LOCAL lock_timeout = '5s';
-- lint:allow column-check api_keys holds one row per key, tens to hundreds a tenant, and every new column is NULL on each, so validating the inline check scans a small table once under the lock
-- lint:allow constraint api_keys_suspension holds on every row the moment it is added, each new column being NULL, and api_keys is small, so one scan under the held lock is cheaper than a second, no-transaction migration

ALTER TABLE grants ADD COLUMN revoked_epoch bigint;
COMMENT ON COLUMN grants.revoked_epoch IS
    'The seq of the grant.revoked that ended the grant; a revoked grant confers nothing (ADR-0097 d10).';

ALTER TABLE api_keys
    ADD COLUMN suspended_cause text CHECK (suspended_cause IN ('against_admin', 'unlinked')),
    ADD COLUMN suspended_act_id uuid,
    ADD COLUMN suspended_at timestamptz,
    ADD CONSTRAINT api_keys_suspension CHECK (
        (suspended_cause IS NULL AND suspended_act_id IS NULL AND suspended_at IS NULL)
        OR (suspended_cause = 'unlinked' AND suspended_act_id IS NULL AND suspended_at IS NOT NULL)
        OR (suspended_cause = 'against_admin' AND suspended_act_id IS NOT NULL
            AND suspended_at IS NOT NULL));
COMMENT ON COLUMN api_keys.suspended_cause IS
    'Why the key authenticates nothing for now: an open act against its person, or unlinked at the upgrade (ADR-0097 d8, d21).';

CREATE POLICY contest_lookup ON admin_acts FOR SELECT
    USING (target->>'email_digest'
           = 'sha256:' || NULLIF(current_setting('oml.api_key_hash', true), ''));
CREATE INDEX admin_acts_removed_admin_email ON admin_acts ((target->>'email_digest'))
    WHERE act = 'remove_admin';
