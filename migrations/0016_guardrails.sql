-- 0016_guardrails — a guardrail version stores its severity and its optional
-- rule, and a key stores its guardrail scope (proposed ADR-0037 d2, d4, d8,
-- d10, d14; build steps S1 and S2; planned for v0.5.1). Forward-only
-- (docs/data/postgres-schema.md "Migrations"): an applied file is never
-- edited, a correction is the next numbered file. 0014 and 0015 are v0.5.0's
-- (ADR-0044's user roots, ADR-0045's web tables), which ship first; this file
-- takes 0016, and on a branch without them the gap is left.
--
-- memories.kind holds no CHECK (0005), so kind 'guardrail' needs none. The
-- severity is clear, as it is on the submission events; the rule is stored
-- inline beside the body, as the PoC stores the body, and the submission
-- journals its digest only. Every other guardrail version carries a
-- severity; a row of any other kind, and a guardrail's retirement version,
-- carries neither, and a rule never stands without a severity. On a
-- partitioned table the columns and checks reach every partition, and a
-- partition made later (LIKE memories INCLUDING ALL) takes them too.
--
-- api_keys.guardrail_scope: every key issued before this migration holds the
-- issuance default, 'ancestors', as its key.issued without the field folds;
-- nothing in the journal is rewritten (d4, the maintainer's reading to
-- confirm).
--
-- Run by the owner role. Table privileges cover the new columns, so the
-- serving role's DML is 0007's, unchanged (postgres-schema.md "Privileges").

ALTER TABLE memories
    ADD COLUMN severity text
        CONSTRAINT memories_severity_check CHECK (severity IN ('never', 'must', 'should')),
    ADD COLUMN rule     jsonb,
    ADD CONSTRAINT memories_guardrail_fields_check
        CHECK ((severity IS NULL OR (kind = 'guardrail' AND NOT retirement))
               AND (severity IS NOT NULL OR kind <> 'guardrail' OR retirement)
               AND (rule IS NULL OR severity IS NOT NULL));

ALTER TABLE api_keys
    ADD COLUMN guardrail_scope text NOT NULL DEFAULT 'ancestors'
        CONSTRAINT api_keys_guardrail_scope_check
        CHECK (guardrail_scope IN ('workarea', 'ancestors'));
