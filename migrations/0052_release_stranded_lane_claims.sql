-- Releases the review claims a key that cannot approve them took on the
-- person lane before it existed (ADR-0074 d19, accepted for the instruction
-- class by ADR-0080 d16; #437).
--
-- A claim never expires and only its holder sees it. Before the lane, review
-- next served a steering, guardrail or skill version, or a successor of
-- another kind replacing one, to any reviewer key, a crew's agent key
-- included. Such a key can no longer approve it, and no person is served it
-- while the claim stands. This clears held_by_key on every such claim whose
-- holder is not a person's reviewer key: role reviewer, used_by person, no
-- crew and no profile (until #214 brings ADR-0074 d2's definition), so the
-- row returns to the lane for a person. Nothing is journalled: a claim is
-- queue state, not a record, and its holder made no verdict.
--
-- The instruction kinds are spelled as the domain's class table spells them
-- today (steering, guardrail, skill; ADR-0080 d16).
--
-- Numbered 0052, after the linked tenant keys (0050, 0051).
--
-- What it costs a live database: one UPDATE of review_queue joined to its
-- memories and the holders' key rows, each tenant's rows under its row
-- security as 0043 enumerates the tenants. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE review_queue q
           SET held_by_key = NULL
          FROM memories m, api_keys k
         WHERE q.tenant_id = t AND m.tenant_id = t AND m.id = q.memory_id
           AND k.tenant_id = t AND k.id = q.held_by_key
           AND NOT (k.role = 'reviewer' AND k.used_by = 'person'
                    AND k.crew_id IS NULL AND k.profile IS NULL)
           AND (m.kind IN ('steering', 'guardrail', 'skill')
                OR (m.replaces_artifact_id IS NOT NULL AND EXISTS (
                    SELECT 1 FROM memories r
                     WHERE r.tenant_id = t AND r.id = m.replaces_artifact_id
                       AND r.kind IN ('steering', 'guardrail', 'skill'))));
    END LOOP;
    PERFORM set_config('oml.startup_check', '', true);
    PERFORM set_config('oml.tenant_id', '', true);
END $$;
