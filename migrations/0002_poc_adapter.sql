-- 0002_poc_adapter — the columns the PoC Postgres adapter (crates/adapters/
-- oml-pg) needs beyond 0001's slice. Forward-only (docs/data/postgres-schema.md
-- "Migrations"): 0001 is never edited, a correction is the next numbered file.

------------------------------------------------------------------------------
-- journal.signed_bytes — the stored RFC 8785 canonical bytes verification
-- runs over ("the stored bytes are authoritative", docs/data/chain-format.md).
-- 0001 anticipated this column joining "with the chain verifier work in a
-- later migration"; the adapter's events_range/fork-check is that work.
-- The ADD/DROP DEFAULT pair keeps the migration valid even if a row ever
-- predates it; no deployment exists, so in practice the table is empty here.
------------------------------------------------------------------------------
ALTER TABLE journal ADD COLUMN signed_bytes bytea NOT NULL DEFAULT '\x';
ALTER TABLE journal ALTER COLUMN signed_bytes DROP DEFAULT;
COMMENT ON COLUMN journal.signed_bytes IS
    'RFC 8785 canonical signed bytes, stored verbatim; verification reads these, '
    'never a re-serialization. Owning page: docs/data/chain-format.md.';

------------------------------------------------------------------------------
-- memories — the PoC MemoryRecord fields the port round-trips that 0001's
-- M1 slice left out: authorship, the flattened lineage closure (the R2
-- exclusion input, docs/architecture/roles-and-keys.md), the declared claim
-- key, derivation edges and the scan finding (docs/architecture/admission.md).
------------------------------------------------------------------------------
ALTER TABLE memories
    ADD COLUMN claim_key          text,
    ADD COLUMN author_principal   text   NOT NULL DEFAULT '',
    ADD COLUMN author_key_id      uuid   NOT NULL
        DEFAULT '00000000-0000-0000-0000-000000000000',
    ADD COLUMN derives_from       uuid[] NOT NULL DEFAULT '{}',
    ADD COLUMN lineage_principals text[] NOT NULL DEFAULT '{}',
    ADD COLUMN lineage_key_ids    uuid[] NOT NULL DEFAULT '{}',
    ADD COLUMN scan_finding       text;
ALTER TABLE memories ALTER COLUMN author_principal DROP DEFAULT;
ALTER TABLE memories ALTER COLUMN author_key_id DROP DEFAULT;

------------------------------------------------------------------------------
-- memories.workarea_id loses its FK to workareas: the fold the commit applies
-- is mechanical — workarea resolution and existence are enforced by the
-- use-case layer, never by the adapter (ADR-0019 d1), and the port-contract
-- fixtures (oml-port-contracts) exercise MemoryHeld against workarea ids
-- with no materialized node. PoC decision, carried in the oml-pg phase report.
------------------------------------------------------------------------------
ALTER TABLE memories DROP CONSTRAINT memories_workarea_id_fkey;

------------------------------------------------------------------------------
-- review_queue.held_by_key loses its FK to api_keys: the claim records the
-- reviewing key id exactly as the port presents it, and the PoC port
-- (oml_app::ports::JournalStore::review_claim_next) claims by key id without
-- requiring an api_keys row — dreamer keys may also be provider-level rows
-- outside this tenant's RLS context. PoC decision, carried in the oml-pg
-- phase report.
------------------------------------------------------------------------------
ALTER TABLE review_queue DROP CONSTRAINT review_queue_held_by_key_fkey;
