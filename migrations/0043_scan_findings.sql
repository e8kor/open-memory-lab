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
-- that admitted it, with the stored text kept in `migrated_from`. Every
-- finding the text names (`; ` between them, up to five and `and N more`)
-- becomes one row: `class: rule at /body line N` and an optional
-- ` at <view>` give the class, the rule, the field (`/body`, `/title`, or
-- `/body/` and the decoded view's path with its dots as slashes) and the
-- line, so a recording rescan that finds it again matches it by class, rule
-- and field and adds no second row. Admission's derived scan wrote the view
-- after the last part alone (the `and N more` tail included) and a rescan
-- after each finding, so a text of several findings with one view suffix,
-- on the last finding or on the tail, is ambiguous: none of its findings is
-- placed. A view is a document's own key path, and a key may hold `; `,
-- which splits one finding in two: a text of several parts where any part
-- fails the grammar is placed nowhere, and that part names no finding. A finding with no field, placed so or not
-- parsed, stands for one place of its class and rule: the recording rescan
-- takes its first find of them as that place and lists the others. Its view
-- and span were never stored, so they are NULL. Its verdict is `refuse` for
-- a refuse class (`secret.*`, `format.scan_bound`, `anchor.secret_path`),
-- which a recording rescan of v0.6.3 could store on a held version, and
-- `hold` otherwise.
--
-- No foreign key to memories: a version is never deleted (ADR-0001 R4), and
-- one would take a SHARE ROW EXCLUSIVE lock on every memories partition,
-- blocking the tenant's writes until this migration commits.
--
-- Numbered 0043, after the idempotency claims (0042).
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
        WITH split AS (
            -- Each part of the stored text, in order: a finding, or the
            -- trailing `and N more`, which names none but carries the view
            -- suffix when admission's derived scan held more than five.
            SELECT m.tenant_id, m.id, j.seq, m.scanned_under, m.scan_finding,
                   e.part, e.n::integer AS n,
                   e.part ~ '^and [0-9]+ more( at \S+)?$' AS tail,
                   e.part ~ '^and [0-9]+ more at \S+$' AS tail_suffixed,
                   regexp_match(e.part,
                       '^([a-z_]+\.[a-z_]+): (\S+) at (/[a-z_/]+) line ([0-9]+)(?: at (\S+))?$') AS r
              FROM memories m
              JOIN journal j ON j.tenant_id = m.tenant_id AND j.event_id = m.created_event_id
              CROSS JOIN LATERAL unnest(string_to_array(m.scan_finding, '; '))
                   WITH ORDINALITY AS e(part, n)
             WHERE m.tenant_id = t AND m.scan_finding IS NOT NULL
        ), shaped AS (
            -- Per text: how many findings it names, how many carry a view,
            -- whether the last finding or the tail carries one, and whether
            -- any part fails the grammar (a document key holding `; `
            -- splits a finding in two).
            SELECT split.*,
                   count(*) FILTER (WHERE NOT tail) OVER w AS k,
                   count(r[5]) FILTER (WHERE NOT tail) OVER w AS suffixed,
                   bool_or(NOT tail AND r[5] IS NOT NULL AND n = last_n) OVER w AS last_suffixed,
                   bool_or(tail_suffixed) OVER w AS tail_view,
                   bool_or(NOT tail AND r IS NULL) OVER w AS broken
              FROM (SELECT split.*,
                           max(n) FILTER (WHERE NOT tail) OVER (PARTITION BY id) AS last_n
                      FROM split) split
            WINDOW w AS (PARTITION BY id)
        ), placed AS (
            -- Admission's derived scan puts the view after the last part
            -- alone, the `and N more` tail included; a rescan puts it after
            -- each finding. One view suffix on the last of several findings,
            -- or on the tail, could be either, and a text with a part that
            -- fails the grammar may have split: no finding of such a text is
            -- placed, and a part that fails the grammar names no finding.
            SELECT shaped.*,
                   (k > 1 AND (broken OR (suffixed = 1 AND last_suffixed))) OR tail_view
                       AS unplaced
              FROM shaped
             WHERE NOT tail AND (r IS NOT NULL OR k = 1)
        )
        SELECT tenant_id, id, seq, n - 1,
               COALESCE(r[1], split_part(part, ': ', 1)),
               COALESCE(r[2], NULLIF(split_part(split_part(part, ': ', 2), ' at ', 1), ''),
                        split_part(part, ': ', 1)),
               CASE WHEN COALESCE(r[1], split_part(part, ': ', 1)) LIKE 'secret.%'
                         OR COALESCE(r[1], split_part(part, ': ', 1))
                            IN ('format.scan_bound', 'anchor.secret_path')
                    THEN 'refuse' ELSE 'hold' END,
               scanned_under,
               CASE WHEN r IS NULL OR unplaced THEN NULL
                    WHEN r[5] IS NULL THEN r[3]
                    WHEN r[5] = 'title' THEN '/title'
                    ELSE '/body/' || replace(r[5], '.', '/') END,
               CASE WHEN r IS NULL OR unplaced THEN NULL ELSE (r[4])::integer END,
               'admission', scan_finding
          FROM placed
        ON CONFLICT DO NOTHING;
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;
