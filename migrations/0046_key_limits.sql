-- A key's end date and its request rate (ADR-0079 d1, d4, d5).
--
-- Two nullable columns on api_keys, folded from key.issued and replaced by
-- key.limits_set:
--
--   expires_at           the instant from which the key authenticates
--                        nothing. NULL: the key does not expire. The row
--                        stays past it, as a revoked key's does (R4;
--                        ADR-0026 d10); every lookup compares it with the
--                        lead's clock, so nothing sweeps it.
--   requests_per_minute  the requests the key may make in one UTC minute.
--                        NULL: the key has no rate of its own, and only its
--                        account's quota, if any, limits it (ADR-0068 d5).
--
-- Every existing key gets NULL in both, so no key expires or is limited by
-- this migration.
--
-- An admin key carries neither (api_keys_admin_key_has_no_limits): the keys
-- that change limits hold none, so none sheds its own through a second
-- admin key, and no admin key ends another by a date (ADR-0079 d1, d5).
--
-- Numbered 0046, after the usage metering (0045).
--
-- What it costs a live database: two nullable columns with no default, a
-- catalog change only, and two CHECKs validated in one scan of api_keys,
-- which holds one row per key ever issued. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

-- lint:allow column-check api_keys holds one row per key ever issued and every row reads NULL, so validating the inline check scans a small table once
-- lint:allow constraint the same scan of the same small table validates it, and every row reads NULL in both columns it names
ALTER TABLE api_keys
    ADD COLUMN expires_at          timestamptz(6),
    ADD COLUMN requests_per_minute integer
        CHECK (requests_per_minute BETWEEN 1 AND 1000000),
    ADD CONSTRAINT api_keys_admin_key_has_no_limits
        CHECK (role <> 'admin' OR (expires_at IS NULL AND requests_per_minute IS NULL));

COMMENT ON COLUMN api_keys.expires_at IS
    'The instant from which the key authenticates nothing; NULL never (ADR-0079 d1, d2).';
COMMENT ON COLUMN api_keys.requests_per_minute IS
    'The key''s own requests per UTC minute; NULL none of its own (ADR-0079 d4).';
