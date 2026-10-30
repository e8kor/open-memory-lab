-- 0004_grants — the authorization state effective access is computed from:
-- grants, global per-key denials, each key's issuer and issued seq, and a
-- workarea's crew (ADR-0025 d1, d2, d14). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0003 are never edited,
-- a correction is the next numbered file.
-- Normative shapes: docs/data/postgres-schema.md "api_keys, grants,
-- key_denials, bans" and "workareas and edges"; the effective-access formula
-- and the permission epoch are docs/architecture/workareas.md's.

------------------------------------------------------------------------------
-- api_keys.issued_by, api_keys.issued_seq — the issuing key (NULL for the
-- bootstrap ceremony's first admin key, which no key issues) and the journal
-- seq of the key's key.issued, or of the key.rotated that minted it: the
-- floor of its permission epoch (ADR-0025 d14).
------------------------------------------------------------------------------
ALTER TABLE api_keys
    ADD COLUMN issued_by  uuid   REFERENCES api_keys (id),
                                 -- issuer ceiling checked at issuance
    ADD COLUMN issued_seq bigint;

-- Backfill from each key's own key.issued event: its seq, and the envelope's
-- api_key_id, the key that authored it. 0001's FORCE row-level security
-- binds this migration's role too, so the backfill runs one tenant at a time
-- under the ordinary tenant_isolation context, enumerating tenants through
-- 0003's startup policy; no policy is lifted.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        UPDATE api_keys k
           SET issued_seq = j.seq,
               issued_by  = j.api_key_id
          FROM journal j
         WHERE k.tenant_id = t
           AND j.tenant_id = t
           AND j.event_type = 'key.issued'
           AND j.payload ->> 'key_id' = k.id::text;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END $$;

-- A key row no key.issued accounts for fails here, before any epoch could be
-- computed without its floor.
ALTER TABLE api_keys ALTER COLUMN issued_seq SET NOT NULL;

------------------------------------------------------------------------------
-- workareas.crew — recorded at workarea.create, never operated: OML never
-- contacts the external manager (docs/data/event-catalog.md workarea.create).
------------------------------------------------------------------------------
ALTER TABLE workareas ADD COLUMN crew jsonb;   -- {name, charter_uri, external_manager}

------------------------------------------------------------------------------
-- grants — one role over the subtree of one workarea node, folded from
-- grant.issued. The node is an id, never a path; deny holds the subtrees
-- masked inside this grant only, as grant.issued spells them
-- ([{"subtree_node_id": …}]); epoch is the grant.issued's own journal seq.
------------------------------------------------------------------------------
CREATE TABLE grants (
    id               uuid   PRIMARY KEY,
    tenant_id        uuid   NOT NULL REFERENCES tenants (id),
    key_id           uuid   NOT NULL REFERENCES api_keys (id),
    role             text   NOT NULL
        CHECK (role IN ('reader', 'writer', 'dreamer')),  -- admissible per key role:
                                          -- roles-and-keys Grant admissibility (ADR-0025 d1)
    workarea_node_id uuid   NOT NULL REFERENCES workareas (id),  -- node id, never a path
    deny             jsonb  NOT NULL DEFAULT '[]',  -- subtree masks inside THIS grant only
    epoch            bigint NOT NULL                -- journal seq of this grant's grant.issued,
                                                    -- or of the key.rotated that reproduced it
);

-- Effective access reads a key's grants on every request (ADR-0025 d1).
CREATE INDEX grants_key ON grants (tenant_id, key_id);

COMMENT ON TABLE grants IS
    'Grants folded from grant.issued: one role over one node''s subtree, with per-grant deny masks. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- key_denials — a global per-key deny, folded from key.denied: deny-wins
-- across all of the key's grants; epoch is the key.denied's own journal seq.
------------------------------------------------------------------------------
CREATE TABLE key_denials (
    id               uuid           PRIMARY KEY,
    tenant_id        uuid           NOT NULL REFERENCES tenants (id),
    key_id           uuid           NOT NULL REFERENCES api_keys (id),
    workarea_node_id uuid           NOT NULL REFERENCES workareas (id),
    epoch            bigint         NOT NULL,   -- journal seq of this denial's key.denied
    created_by       uuid           NOT NULL,
    created_at       timestamptz(6) NOT NULL DEFAULT now()
);

CREATE INDEX key_denials_key ON key_denials (tenant_id, key_id);

COMMENT ON TABLE key_denials IS
    'Global per-key denials folded from key.denied: deny-wins across all grants. '
    'Owning page: docs/data/postgres-schema.md.';

------------------------------------------------------------------------------
-- Row-level security — as 0001: FORCE, so the owner is bound too, keyed by
-- the transaction-local oml.tenant_id the server sets from the API key.
------------------------------------------------------------------------------
ALTER TABLE grants ENABLE ROW LEVEL SECURITY;
ALTER TABLE grants FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON grants
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

ALTER TABLE key_denials ENABLE ROW LEVEL SECURITY;
ALTER TABLE key_denials FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON key_denials
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
