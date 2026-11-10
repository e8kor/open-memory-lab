-- federated_recall — the policies and the record of federated recall
-- (ADR-0072 phase 1, #205).
--
-- tenants: the tenant's `federation` policy, folded from
-- `federation.policy_set`: `enabled` (off until a tenant admin turns it
-- on), `default_sources` (the mode an absent `sources` reads, never a list)
-- and `max_sources` (the most candidates a `wide` recall searches beside
-- its scope, and the most names a list carries).
--
-- workareas: `accept_federated`, folded from `workarea.federation_set`:
-- whether recalls scoped elsewhere may search the node. It grants nothing;
-- off, a key that reads the node still recalls there by naming it as the
-- scope.
--
-- recall_events: what a federated tenant's recall searched, so the replay
-- computes the same reference set (d9): `sources`, the mode or the list's
-- workarea ids; the federated nodes searched; the reference set's nodes.
-- NULL on a tenant with federation off. Workarea ids only, under the
-- table's retention and erasure (ADR-0054 d4).
--
-- Numbered 0040, after the web session credentials (0039). Each column is
-- added with a constant default or none, so no table is rewritten; the
-- inline checks hold for the default every existing row takes.

SET LOCAL lock_timeout = '5s';

ALTER TABLE tenants
    ADD COLUMN federation_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN federation_default_sources text NOT NULL DEFAULT 'scope'
        CHECK (federation_default_sources IN ('scope', 'wide')),
    ADD COLUMN federation_max_sources integer NOT NULL DEFAULT 8
        CHECK (federation_max_sources BETWEEN 0 AND 64);

COMMENT ON COLUMN tenants.federation_enabled IS
    'Whether recall may search beyond its scope, folded from federation.policy_set (ADR-0072 d5). '
    'Owning page: docs/data/postgres-schema.md.';
COMMENT ON COLUMN tenants.federation_default_sources IS
    'The sources mode an absent recall sources reads: scope or wide, never a list (ADR-0072 d5).';
COMMENT ON COLUMN tenants.federation_max_sources IS
    'The most candidates a wide recall searches beside its scope, and the most names a list carries (ADR-0072 d5).';

ALTER TABLE workareas
    ADD COLUMN accept_federated boolean NOT NULL DEFAULT true;

COMMENT ON COLUMN workareas.accept_federated IS
    'Whether recalls scoped elsewhere may search this node, folded from workarea.federation_set; '
    'grants nothing (ADR-0072 d5). Owning page: docs/data/postgres-schema.md.';

ALTER TABLE recall_events
    ADD COLUMN sources jsonb,
    ADD COLUMN federated_workarea_ids uuid[],
    ADD COLUMN reference_workarea_ids uuid[];

COMMENT ON COLUMN recall_events.sources IS
    'The recall''s sources on a federated tenant: "scope", "wide" or the list''s workarea ids; '
    'NULL with federation off (ADR-0072 d9).';
COMMENT ON COLUMN recall_events.reference_workarea_ids IS
    'The reference set''s nodes, whose statistics scored every hit, for the replay (ADR-0072 d9).';
