-- 0069_key_previous_hashes — a rotated key's previous value during its grace
-- (ADR-0096 d13, the operator's decision of 2026-11-25).
--
-- `key.rotated` gives a bearer key a new hash on api_keys.hash. With a grace,
-- up to 24 hours, the previous hash keeps authenticating until valid_until:
-- one row per key, replaced by the key's next rotation and deleted by a
-- rotation without grace. The server compares valid_until with its clock,
-- so a row past its end resolves nothing; the next rotation of that key
-- clears it. Key resolution precedes the tenant, as 0001's key_resolution
-- policy on api_keys has it, so the row is found by an exact match on
-- oml.api_key_hash, SELECT only.
--
-- What it costs a live database: one new, empty table and its unique index
-- on the hash. The foreign keys take a share lock on tenants and api_keys
-- to validate an empty table, waited for at most five seconds.

SET LOCAL lock_timeout = '5s';

CREATE TABLE key_previous_hashes (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    key_id uuid NOT NULL,
    hash bytea NOT NULL,
    valid_until timestamptz NOT NULL,
    PRIMARY KEY (tenant_id, key_id),
    FOREIGN KEY (tenant_id, key_id) REFERENCES api_keys (tenant_id, id)
);
CREATE UNIQUE INDEX key_previous_hashes_hash ON key_previous_hashes (hash);
COMMENT ON TABLE key_previous_hashes IS
    'A rotated key''s previous value hash and the end of its grace (ADR-0096 d13).';

ALTER TABLE key_previous_hashes ENABLE ROW LEVEL SECURITY;
ALTER TABLE key_previous_hashes FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON key_previous_hashes
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);
CREATE POLICY key_resolution ON key_previous_hashes FOR SELECT
    USING (hash = decode(NULLIF(current_setting('oml.api_key_hash', true), ''), 'hex'));

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, DELETE ON key_previous_hashes TO %I',
                   current_setting('oml.serving_role'));
END $$;
