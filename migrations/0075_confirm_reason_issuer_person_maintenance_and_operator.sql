-- 0075_confirm_reason_issuer_person_maintenance_and_operator — what the
-- operator's answers of 2026-11-27 (ADR-0097, third amendment) add to the
-- admin acts and the keys.
--
-- admin_acts.confirm_reason: the reason a confirmation requires (d26;
-- answer 18), folded from the admin.act that moves the act to confirmed.
-- Null on every act confirmed before it, and on every act not confirmed.
--
-- api_keys.issuer_person: the person the issuing key counted as at the
-- issuance (#508 N1; answer 17), folded from key.issued's issuer_person,
-- recorded the way memories.author_person is at a write (0061). A key
-- issued before it reads null, which counts as no recorded person: the
-- settlement then reads its issuer chain as before.
--
-- What it costs a live database: two nullable columns; no existing row is
-- read or rewritten.

SET LOCAL lock_timeout = '5s';

ALTER TABLE admin_acts ADD COLUMN confirm_reason text;
COMMENT ON COLUMN admin_acts.confirm_reason IS
    'The reason the confirmation gave, required since v0.8.6 (ADR-0097 d26).';

ALTER TABLE api_keys ADD COLUMN issuer_person text;
COMMENT ON COLUMN api_keys.issuer_person IS
    'The person the issuing key counted as at issuance, since v0.8.6 (#508 N1).';
