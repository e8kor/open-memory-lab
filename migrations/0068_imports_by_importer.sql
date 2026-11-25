-- 0068_imports_by_importer — the imports a person's keys ran, newest first
-- (ADR-0096 d6).
--
-- `GET /v2/imports` lists, for a person's web session, the imports of every
-- key that names the person as its user, and for any other key that key's
-- own, newest first by created_at then id. The listing reads the imports
-- table by tenant and importing key in that order; this index serves it,
-- one range per key, so a page costs the keys' ranges and not the tenant's
-- imports. docs/reference/limits.md carries the bottleneck row.
--
-- What it costs a live database: one CREATE INDEX on imports, which holds a
-- row per import ever run; a deployment with few imports builds it at once.
CREATE INDEX imports_importer_created ON imports
    (tenant_id, importer_key_id, created_at DESC, id DESC);
