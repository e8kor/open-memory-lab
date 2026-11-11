-- scan_findings — every finding on a memory version, one row each
-- (ADR-0074 d23).
--
-- A version stored one finding, `memories.scan_finding`, the first hold
-- admission met. This table holds every finding the scan made on it: its
-- class, rule and verdict, the rule set it was found under, where it fired
-- (the field, the line, the view the rule read and the byte range in that
-- view), who recorded it (`admission`, a recording `rescan`; the classifier
-- and the contradiction check of #217 and #218 later) and the seq of the
-- event that recorded it. Never the matched text (ADR-0065 d8). Admission
-- writes a version's findings in the commit that stores it; the recording
-- rescan adds a finding the list does not hold by class, rule and field.
-- The policy version and the dismissal columns d23 also names are added by
-- the phase that first writes them (#214).
--
-- `memories.scan_finding` stays, written as before, for one release: the
-- in-progress served-set predicates read it (ADR-0074 d20, d28); a later
-- migration drops it. An older binary refuses to start once this migration
-- has run (sqlx finds a migration it does not know), and the way back after
-- a migration is a restore (docs/operations/upgrades.md), so no older binary
-- writes after it. The one window is a held version an older server wrote
-- while this migration ran: it has no row here, doctor's
-- `scan_findings_unlisted` counts it, and `oml admin rescan --record-held`
-- lists its findings.
--
-- Each stored `scan_finding` becomes one row (d23), found by `admission`
-- under the rule set the version was scanned under, at the seq of the event
-- that admitted it, with the stored text kept in `migrated_from`. Its first
-- finding, `class: rule at /body line N` and an optional ` at <view>`,
-- gives the class, the rule, the field (`/body`, `/title`, or `/body/` and
-- the decoded view's path with its dots as slashes) and the line, so a
-- recording rescan that finds it again matches it by class, rule and field
-- and adds no second row. A text that does not parse keeps its field NULL,
-- and the recording rescan then matches by class and rule alone. Its view
-- and span were never stored, so they are NULL. Its verdict is `refuse` for
-- a refuse class (`secret.*`, `format.scan_bound`, `anchor.secret_path`),
-- which a recording rescan of v0.6.3 could store on a held version, and
-- `hold` otherwise.
--
-- No foreign key to memories: a version is never deleted (ADR-0001 R4), and
-- one would take a SHARE ROW EXCLUSIVE lock on every memories partition,
-- blocking the tenant's writes until this migration commits.
--
-- Numbered 0043, after the as-of tips (0041); 0042 is another branch's.
--
-- What it costs a live database: a new table, its policies and its two
-- indexes, built while it is empty; then one INSERT … SELECT per tenant,
-- which reads that tenant's memories and journal rows under ACCESS SHARE,
-- so writes and reads go on. It scans each tenant's partition once, since
-- no index covers `scan_finding`. A lock is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

CREATE TABLE scan_findings (
    tenant_id     uuid    NOT NULL,
    memory_id     uuid    NOT NULL,
    event_seq     bigint  NOT NULL CHECK (event_seq >= 1),
    ord           integer NOT NULL CHECK (ord >= 0),
    class         text    NOT NULL,
    rule          text    NOT NULL,
    verdict       text    NOT NULL CHECK (verdict IN ('refuse', 'hold', 'annotate')),
    rule_set      text,
    field         text,
    line          integer CHECK (line >= 1),
    via           text,
    span_start    bigint  CHECK (span_start >= 0),
    span_end      bigint  CHECK (span_end >= span_start),
    found_by      text    NOT NULL
        CHECK (found_by IN ('admission', 'rescan', 'classifier', 'contradiction')),
    migrated_from text,
    CONSTRAINT scan_findings_pkey PRIMARY KEY (tenant_id, memory_id, event_seq, ord),
    -- One row per finding: a rescan that finds what the list holds adds none.
    CONSTRAINT scan_findings_one UNIQUE NULLS NOT DISTINCT (tenant_id, memory_id, class, rule, field)
);

COMMENT ON TABLE scan_findings IS
    'Every scan finding on a memory version, never the matched text (ADR-0074 d23; ADR-0065 d8). '
    'Owning page: docs/architecture/admission.md.';
COMMENT ON COLUMN scan_findings.via IS
    'The view the rule read: raw, clean, spaced or letters (ADR-0074 d6).';
COMMENT ON COLUMN scan_findings.migrated_from IS
    'On a row migrated from memories.scan_finding by 0043, the text that column held.';

ALTER TABLE scan_findings ENABLE ROW LEVEL SECURITY;
ALTER TABLE scan_findings FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON scan_findings
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

-- The serving role writes a version's findings and reads them; a row is
-- never updated or deleted.
DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT ON scan_findings TO %I',
                   current_setting('oml.serving_role'));
END $$;

-- Each stored finding as one row, under each tenant's row security, the
-- tenants enumerated under 0003's startup policy as 0020 enumerates them.
DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        INSERT INTO scan_findings (tenant_id, memory_id, event_seq, ord, class, rule, verdict,
                                   rule_set, field, line, found_by, migrated_from)
        SELECT p.tenant_id, p.id, p.seq, 0, p.class, p.rule,
               CASE WHEN p.class LIKE 'secret.%'
                         OR p.class IN ('format.scan_bound', 'anchor.secret_path')
                    THEN 'refuse' ELSE 'hold' END,
               p.scanned_under, p.field, p.line, 'admission', p.scan_finding
          FROM (
            SELECT m.tenant_id, m.id, j.seq, m.scanned_under, m.scan_finding,
                   COALESCE(r[1], split_part(m.scan_finding, ': ', 1)) AS class,
                   COALESCE(r[2],
                            NULLIF(split_part(split_part(m.scan_finding, ': ', 2), ' at ', 1), ''),
                            split_part(m.scan_finding, ': ', 1)) AS rule,
                   CASE WHEN r IS NULL THEN NULL
                        WHEN r[5] IS NULL THEN r[3]
                        WHEN r[5] = 'title' THEN '/title'
                        ELSE '/body/' || replace(r[5], '.', '/') END AS field,
                   (r[4])::integer AS line
              FROM memories m
              JOIN journal j ON j.tenant_id = m.tenant_id AND j.event_id = m.created_event_id
              LEFT JOIN LATERAL regexp_match(
                    split_part(m.scan_finding, '; ', 1),
                    '^([a-z_]+\.[a-z_]+): (\S+) at (/[a-z_/]+) line ([0-9]+)(?: at (\S+))?$') AS r
                ON true
             WHERE m.tenant_id = t AND m.scan_finding IS NOT NULL
          ) p
        ON CONFLICT DO NOTHING;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;
