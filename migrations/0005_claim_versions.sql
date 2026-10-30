-- 0005_claim_versions — the claim fields the claim item needs: a version
-- ordinal per claim_key, the kind, the served tip, promoted_at, the NFC byte
-- count, the retirement mark and the replaced version; the PoC's
-- 'superseded' state migrated away (ADR-0025 d6, d8, d10, d11, d18).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): 0001–0004 are
-- never edited, a correction is the next numbered file.
-- Normative shapes: docs/data/postgres-schema.md "memories"; the chain and
-- tip semantics are docs/architecture/audit-and-lineage.md's. The PoC table
-- keeps its id, status and inline claim columns; it has no provenance or
-- simulation columns yet, so the served-tip index carries no taint clause.

------------------------------------------------------------------------------
-- The new columns. version, kind and body_bytes are backfilled below and
-- then made NOT NULL.
------------------------------------------------------------------------------
ALTER TABLE memories
    ADD COLUMN version              integer,
    ADD COLUMN kind                 text,     -- memory | derived_fact | steering | term |
                                              -- term_mapping | vocab_release
    ADD COLUMN is_tip               boolean        NOT NULL DEFAULT false,
    ADD COLUMN promoted_at          timestamptz(6),
    ADD COLUMN body_bytes           integer,  -- UTF-8 octets of the NFC body; budget tokens
                                              -- derive from it (retrieval Token budgets)
    ADD COLUMN retirement           boolean        NOT NULL DEFAULT false,
                                              -- structural, like term_status: survives shredding
    ADD COLUMN replaces_artifact_id uuid;     -- a supersession's replaced version; NULL for
                                              -- version 1

-- Backfill, one tenant at a time under the ordinary tenant_isolation context
-- (0001's FORCE row-level security binds this migration's role too),
-- enumerating tenants through 0003's startup policy, as 0004 does; no policy
-- is lifted. For each tenant:
--
-- * kind: the PoC validated a submission's kind and discarded it, and served
--   every row as 'memory'; that is what each row was served as, so that is
--   what it keeps.
-- * body_bytes: the octets of the claim's NFC form, which is what capture
--   counts now (ADR-0025 d6). The stored text is not rewritten: its digest
--   is journalled.
-- * replaces_artifact_id: a replaced row's superseded_by named its successor.
-- * version: what capture assigns now, per (workarea, claim_key) in capture
--   order. A version that replaces nothing is 1, so a claim captured after
--   a never-promoted rejection is a fresh version 1 (ADR-0025 d18). A
--   successor is 1 + the greatest version before it, whatever its state
--   (audit-and-lineage.md supersession chains): every PoC supersession
--   replaced a promoted version and kept its claim_key, so that is 1 + the
--   successors captured before it.
-- * is_tip: a promoted version, or one the first PoC wrote 'superseded', is
--   the served tip unless its successor reached promotion. The PoC moved the
--   tip when a supersession was proposed; a version whose successor is still
--   held, or was rejected, is the tip again, since only ratification moves it
--   (ADR-0025 d10).
-- * promoted_at: the at of the lifecycle.promote that promoted the row.
-- * status: 'superseded' becomes 'promoted'; superseded adds no state.
-- * idempotency: a stored PoC submit receipt names no version; every PoC
--   submit captured a version 1.
--
-- The PoC never refused a second claim under a claim_key in use, and d18
-- never lets two held, promoted or demoted versions that replace nothing
-- share one claim_key in one workarea: two promoted ones leave no single
-- served tip to choose, and a held one beside a live one would meet the
-- served tip as a constraint error when it was approved. The migration
-- refuses them by name rather than choose which one keeps the claim_key,
-- hide one from recall, or leave one that no verdict can promote.
DO $$
DECLARE
    t        uuid;
    rivalled record;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);

        UPDATE memories
           SET kind = 'memory',
               body_bytes = octet_length(normalize(claim, NFC))
         WHERE tenant_id = t;

        UPDATE memories s
           SET replaces_artifact_id = r.id
          FROM memories r
         WHERE s.tenant_id = t AND r.tenant_id = t
           AND r.superseded_by = s.id;

        UPDATE memories m
           SET version = n.version
          FROM (SELECT id,
                       CASE WHEN replaces_artifact_id IS NULL THEN 1
                            ELSE 1 + count(replaces_artifact_id)
                                     OVER (PARTITION BY workarea_id, claim_key
                                           ORDER BY created_at, id)
                       END AS version
                  FROM memories
                 WHERE tenant_id = t) n
         WHERE m.tenant_id = t AND m.id = n.id;

        UPDATE memories m
           SET is_tip = NOT EXISTS (
                   SELECT 1 FROM memories s
                    WHERE s.tenant_id = t
                      AND s.id = m.superseded_by
                      AND s.status IN ('promoted', 'superseded'))
         WHERE m.tenant_id = t
           AND m.status IN ('promoted', 'superseded');

        UPDATE memories m
           SET promoted_at = j.at
          FROM journal j
         WHERE m.tenant_id = t AND j.tenant_id = t
           AND j.event_type = 'lifecycle.promote'
           AND j.payload ->> 'artifact_id' = m.id::text;

        UPDATE memories SET status = 'promoted'
         WHERE tenant_id = t AND status = 'superseded';

        UPDATE idempotency
           SET response = jsonb_set(response, '{version}', '1')
         WHERE tenant_id = t
           AND jsonb_typeof(response) = 'object'
           AND NOT response ? 'version';

        SELECT workarea_id, claim_key, count(*) AS live INTO rivalled
          FROM memories
         WHERE tenant_id = t AND replaces_artifact_id IS NULL
           AND status IN ('held', 'promoted', 'demoted')
         GROUP BY workarea_id, claim_key
        HAVING count(*) > 1
         LIMIT 1;
        IF FOUND THEN
            RAISE EXCEPTION '0005: tenant % has % live first versions of claim_key % in workarea %; a claim_key names one live claim, and one claim serves one tip (ADR-0025 d10, d18)',
                t, rivalled.live, rivalled.claim_key, rivalled.workarea_id;
        END IF;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END $$;

-- A row the backfill could not number fails here. claim_key was nullable
-- in 0002; the ring always required one, so only a row the server never
-- wrote fails its NOT NULL.
ALTER TABLE memories
    ALTER COLUMN claim_key  SET NOT NULL,
    ALTER COLUMN version    SET NOT NULL,
    ALTER COLUMN kind       SET NOT NULL,
    ALTER COLUMN body_bytes SET NOT NULL;

-- superseded_by is now the successor's replaces_artifact_id.
ALTER TABLE memories DROP COLUMN superseded_by;

-- The design state set; 'superseded' is not a state (ADR-0025 d10).
ALTER TABLE memories DROP CONSTRAINT memories_status_check;
ALTER TABLE memories ADD CONSTRAINT memories_status_check
    CHECK (status IN ('held', 'promoted', 'demoted', 'rejected', 'erased'));

------------------------------------------------------------------------------
-- Indexes (docs/data/postgres-schema.md "memories").
------------------------------------------------------------------------------

-- A version ordinal is unique among the held, promoted and demoted versions
-- of a claim_key in a workarea, so a rejected or erased claim releases its
-- key for a fresh version 1 (ADR-0025 d18).
CREATE UNIQUE INDEX memories_claim_version
    ON memories (tenant_id, workarea_id, claim_key, version)
    WHERE status IN ('held', 'promoted', 'demoted');

-- The served tip: at most one promoted tip per claim_key in a workarea.
CREATE UNIQUE INDEX memories_served_tip
    ON memories (tenant_id, workarea_id, claim_key)
    WHERE is_tip AND status = 'promoted';

-- The own-work listing: every row a key authored, newest first (ADR-0025 d13).
CREATE INDEX memories_author ON memories (tenant_id, author_key_id, created_at DESC);
