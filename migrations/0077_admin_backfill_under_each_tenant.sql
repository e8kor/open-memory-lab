-- 0070's two backfills, run again under each tenant's row security
-- (ADR-0097 d10, d11; review finding store-1). 0070 ran its UPDATEs with
-- no `oml.tenant_id` set, so under the owner role the forced row security
-- on web_emails, tenants and journal hid every row and both changed
-- nothing: every tenant kept `sole_admin` true and every admin entry
-- accepted before the upgrade kept `admin_since` NULL. The tenants are
-- enumerated under 0003's startup policy, as 0020 enumerates them. Each
-- UPDATE only fills what 0070 left: an `admin_since` the fold has written
-- since stays, and `sole_admin` only turns false, so the migration is
-- idempotent over a database where 0070's backfill did apply.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE web_emails w
           SET admin_since = coalesce(
               (SELECT max(j.at) FROM journal j
                 WHERE j.tenant_id = t
                   AND j.event_type IN ('web.email_allowed', 'web.email_changed',
                                        'web.email_accepted')
                   AND j.payload ->> 'email_digest' = 'sha256:' || encode(w.email_hash, 'hex')
                   AND (j.payload ->> 'admin')::boolean),
               w.created_at)
         WHERE w.tenant_id = t AND w.admin AND w.pending_since IS NULL
           AND w.admin_since IS NULL;
        UPDATE tenants
           SET sole_admin = false
         WHERE id = t AND sole_admin
           AND (SELECT count(DISTINCT j.payload ->> 'user') FROM journal j
                 WHERE j.tenant_id = t
                   AND j.event_type IN ('web.email_allowed', 'web.email_changed',
                                        'web.email_accepted')
                   AND (j.payload ->> 'admin')::boolean) >= 2;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;
