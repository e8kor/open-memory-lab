-- The approving verdict that ratified a memory version, kept on its row
-- (ADR-0077 d3, d6).
--
-- A served memory says when it was approved and by what kind of reviewer.
-- The journal holds that in the version's review.approve event, which no
-- read of a memory may scan for: a recall would read the tenant's whole
-- chain. So the fold of lifecycle.promote and lifecycle.supersede now
-- writes, beside promoted_at, what a read serves of the verdict the event's
-- ratification_event_id names:
--
--   approver_held_by    the reviewing key's attestation at the verdict:
--                       agent, human or service, as api_keys.held_by spells
--                       it.
--   approver_same_user  the promotion event's same_user mark (ADR-0055 d7);
--                       NULL where the event predates the mark.
--   approved_at         the verdict's at.
--
-- All three are NULL on a version never promoted. The row names no
-- reviewing key and no principal: who reviewed stays in the journal's
-- review events (ADR-0025 d15), so this migration adds no attribution for
-- an erasure to cover (#146).
--
-- Rows promoted before this migration are filled from the journal, the
-- record: for each tenant, each version's latest promotion event, the
-- review.approve event it names, and that event's key's held_by. The event
-- names its verdict as ratification_event_id; v0.1.0 named it
-- verdict_event_id, and both are read. A promotion whose event names no
-- verdict the journal holds leaves its row NULL, and the row is served with
-- no approver.
--
-- Numbered 0044, after the scan findings (0043).
--
-- What it costs a live database: three nullable columns, catalog changes
-- only; then one UPDATE per tenant, which reads that tenant's promotion and
-- approval events once and rewrites each promoted row once. It fires no
-- search trigger, since each names its columns and none names these. The
-- server runs it at start, before it serves. A lock is waited for at most
-- 5 s.

SET LOCAL lock_timeout = '5s';

ALTER TABLE memories
    ADD COLUMN approver_held_by   text,
    ADD COLUMN approver_same_user boolean,
    ADD COLUMN approved_at        timestamptz(6);

-- Each promoted version's approval, under each tenant's row security, the
-- tenants enumerated under 0003's startup policy as 0020 enumerates them.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE memories m
           SET approver_held_by   = v.held_by,
               approver_same_user = v.same_user,
               approved_at        = v.at
          FROM (SELECT DISTINCT ON (p.subject)
                       p.subject, k.held_by, p.same_user, a.at
                  FROM (SELECT r.seq,
                               COALESCE(r.payload ->> 'artifact_id',
                                        r.payload ->> 'new_artifact_id')::uuid AS subject,
                               COALESCE(r.payload ->> 'ratification_event_id',
                                        r.payload ->> 'verdict_event_id')::uuid AS verdict,
                               (r.payload ->> 'same_user')::boolean AS same_user
                          FROM journal r
                         WHERE r.tenant_id = t
                           AND r.event_type IN ('lifecycle.promote', 'lifecycle.supersede')) p
                  JOIN journal a ON a.tenant_id = t AND a.event_id = p.verdict
                                AND a.event_type = 'review.approve'
                  JOIN api_keys k ON k.id = a.api_key_id
                 ORDER BY p.subject, p.seq DESC) v
         WHERE m.tenant_id = t AND m.id = v.subject;
    END LOOP;
END $$;
