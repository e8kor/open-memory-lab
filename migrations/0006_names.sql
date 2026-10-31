-- 0006_names — names on workareas, crews, keys and tenants, and crews as
-- records of their own (ADR-0026 d1, d2, d8). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): 0001–0005 are never edited,
-- a correction is the next numbered file.
-- Normative shapes: docs/data/postgres-schema.md "names and tenants",
-- "workareas and edges" and "api_keys, grants, key_denials, bans"; the name
-- rules are docs/api/http-api.md "Names and ids". The steps are the ones
-- "The migration that adds names" lists there.
--
-- This migration names nothing. A migration cannot sign a journal event,
-- and every name a row takes is journalled: the rows that predate names are
-- named under ADR-0026 d11's rules (http-api.md "Rows created before
-- names") by the lead at its first start on this schema, before it serves
-- a request, in one system.names_assigned per chain whose fold fills the
-- name, crew_id and path columns added here. That naming lives in the
-- lead's startup path, after the startup fork check (crates/bins/
-- oml-server-lead), not in SQL: until it has run, every row this migration
-- finds keeps NULL names, and a lead whose start leaves one unnamed serves
-- no request — /readyz reports names_assigned not ready and every other
-- route answers system.unavailable. prior_name is history no reference
-- reads; the naming reads each prior name from the journal, never from
-- these columns.
--
-- Renaming tenants.name and workareas.name breaks every earlier binary,
-- which reads them, so this upgrade stops them before it migrates
-- (docs/operations/upgrades.md "The upgrade order").

-- ltree holds the derived workarea path; PostgreSQL 17 admits
-- "alphanumeric characters, underscores, and hyphens" in a label, so every
-- name is one (postgres-schema.md "workareas and edges").
CREATE EXTENSION IF NOT EXISTS ltree;

------------------------------------------------------------------------------
-- oml_name — the reference grammar of ADR-0026 d2, one domain every named
-- column checks: a lowercase RFC 1035 label, 1–63 characters, that never
-- reads as an id — not the RFC 9562 hex-and-dash form, and not 32 hex
-- digits. The derived form <prefix>-<32 hex> passes, because the fold of
-- system.names_assigned writes it; every creation refuses that form before
-- it journals anything.
------------------------------------------------------------------------------
CREATE DOMAIN oml_name AS text CHECK (
    VALUE ~ '^[a-z]([a-z0-9-]{0,61}[a-z0-9])?$'
    AND VALUE !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    AND VALUE !~ '^[0-9a-f]{32}$'
);

------------------------------------------------------------------------------
-- Step 1: the PoC's creation-time names become prior_name. They hold
-- duplicates and ungrammatical values, the root's 'root' among them
-- (0001_init.sql), so neither the domain nor a unique index can be laid on
-- them; they stay unconstrained text, NULL on every row created with a name.
------------------------------------------------------------------------------
ALTER TABLE tenants RENAME COLUMN name TO prior_name;
ALTER TABLE tenants ALTER COLUMN prior_name DROP NOT NULL;
ALTER TABLE workareas RENAME COLUMN name TO prior_name;
ALTER TABLE workareas ALTER COLUMN prior_name DROP NOT NULL;
COMMENT ON COLUMN tenants.prior_name IS
    'The PoC creation-time name, kept as history; no reference reads it (ADR-0026 d11).';
COMMENT ON COLUMN workareas.prior_name IS
    'The PoC creation-time name, kept as history; no reference reads it (ADR-0026 d11).';

------------------------------------------------------------------------------
-- Step 2: crews, and the new nullable columns. A crew record is recorded,
-- not operated (docs/architecture/workareas.md "Crews: recorded, not
-- operated"); every field but the name is optional, so a later record can
-- add fields without reshaping this one. The references onto crews name the
-- tenant with the id, because PostgreSQL runs a foreign-key check without
-- row-level security (0004_grants.sql): a reference on id alone would
-- accept another tenant's crew.
------------------------------------------------------------------------------
CREATE TABLE crews (
    id               uuid           PRIMARY KEY,
    tenant_id        uuid           NOT NULL REFERENCES tenants (id),
    name             oml_name,                   -- immutable; OML's key for the crew;
                                                 -- NOT NULL with the other names, below
    charter_uri      text,
    external_manager text,
    created_at       timestamptz(6) NOT NULL,    -- the at of the event that created it
    CONSTRAINT crews_tenant_id_name UNIQUE (tenant_id, name),
    CONSTRAINT crews_tenant_id_id   UNIQUE (tenant_id, id)  -- target of crew references
);
COMMENT ON TABLE crews IS
    'Named crew records, recorded not operated (ADR-0026 d8). '
    'Owning page: docs/data/postgres-schema.md.';

ALTER TABLE tenants ADD COLUMN name oml_name;          -- unique per deployment

ALTER TABLE workareas
    ADD COLUMN name    oml_name,   -- unique in the tenant; never changed
    ADD COLUMN path    ltree,      -- DERIVED from names
    ADD COLUMN crew_id uuid,       -- the crew record this workarea names, or NULL
    ADD CONSTRAINT workareas_crew
        FOREIGN KEY (tenant_id, crew_id) REFERENCES crews (tenant_id, id);

ALTER TABLE api_keys
    ADD COLUMN name    oml_name,   -- the key's name; never changed, never released
    ADD COLUMN crew_id uuid,       -- the crew record named at issuance; fixed for life
    ADD CONSTRAINT api_keys_crew
        FOREIGN KEY (tenant_id, crew_id) REFERENCES crews (tenant_id, id);

-- A lifted denial masks nothing and its row stays as history: lifted_epoch
-- is the journal seq of the key.denial_lifted that lifted it, NULL while the
-- denial is in force. The lead's naming lifts every denial on an admin key
-- in the commit that names the chain (ADR-0026 d11, rule 5; event-catalog.md
-- key.denial_lifted).
ALTER TABLE key_denials
    ADD COLUMN lifted_epoch bigint;

------------------------------------------------------------------------------
-- Step 3: the unique constraints, on the new name columns only; each
-- admits any number of NULLs. They are the authority name.taken reports
-- (errors.md): a creation that loses a race to one commits nothing.
-- api_keys' constraint treats the operator keys' NULL tenant as one value,
-- so they share the provider's one namespace; as a plain NULLS NOT DISTINCT
-- constraint it would also make two unnamed keys of one tenant collide, so
-- it is a unique index over the named rows, the same rule once every name
-- is set.
------------------------------------------------------------------------------
ALTER TABLE tenants
    ADD CONSTRAINT tenants_name UNIQUE (name);
ALTER TABLE workareas
    ADD CONSTRAINT workareas_tenant_id_name UNIQUE (tenant_id, name);  -- the whole tree
CREATE UNIQUE INDEX api_keys_tenant_id_name ON api_keys (tenant_id, name) NULLS NOT DISTINCT
    WHERE name IS NOT NULL;

------------------------------------------------------------------------------
-- Step 4: the CHECK that the root, and only the root, is named universe.
-- A check constraint "is satisfied if the check expression evaluates to
-- true or the null value" (PostgreSQL 17 documentation, 5.5.1 "Check
-- Constraints"), and the comparison is NULL on a row whose name is NULL,
-- so the CHECK passes every row this migration finds and refuses a
-- non-root universe under a root that has no name yet as under one that
-- holds universe. The store answers that refusal as name.taken (errors.md).
------------------------------------------------------------------------------
ALTER TABLE workareas
    ADD CONSTRAINT workareas_universe_is_root
        CHECK ((parent_id IS NULL) = (name = 'universe'));

-- The NOT NULL on name and path is a later migration's: it cannot run
-- before the lead's first start has named every row, and it fails, holding
-- that upgrade, if a NULL remains (postgres-schema.md "names and tenants").

------------------------------------------------------------------------------
-- Row-level security — as 0001: FORCE, so the owner is bound too, keyed by
-- the transaction-local oml.tenant_id the server sets from the API key.
------------------------------------------------------------------------------
ALTER TABLE crews ENABLE ROW LEVEL SECURITY;
ALTER TABLE crews FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON crews
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
