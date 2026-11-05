-- In-progress workareas and the curator key profile (ADR-0030 r2 d1, d8,
-- d14).
--
-- NUMBERING: 0026 is taken by the open lanes (lake writer, valid time,
-- similar keys); this file renumbers to the next free number at merge.
--
-- workareas.mode is `standard` unless a person's key put the node in
-- progress, journalled as workarea.mode_set and folded here alone.
-- api_keys.profile is null unless issuance named `curator`, journalled in
-- key.issued. Both default to today's behaviour, so no row changes meaning.
--
-- Safe on a live database: a lock is waited for at most 5 s, then the
-- migration fails and is run again. A column with a constant default is a
-- catalog change since PostgreSQL 11; the checks are added NOT VALID, which
-- takes no scan under the exclusive lock, then validated under SHARE UPDATE
-- EXCLUSIVE, which lets reads and writes run.

SET LOCAL lock_timeout = '5s';

ALTER TABLE workareas
    ADD COLUMN mode text NOT NULL DEFAULT 'standard';
ALTER TABLE workareas
    ADD CONSTRAINT workareas_mode CHECK (mode IN ('standard', 'in_progress')) NOT VALID;
-- `universe` is never in progress (ADR-0030 d1).
ALTER TABLE workareas
    ADD CONSTRAINT workareas_universe_standard
        CHECK (parent_id IS NOT NULL OR mode = 'standard') NOT VALID;

ALTER TABLE api_keys
    ADD COLUMN profile text;
ALTER TABLE api_keys
    ADD CONSTRAINT api_keys_profile
        CHECK (profile IS NULL OR (profile = 'curator' AND role = 'dreamer'
                                   AND held_by = 'agent')) NOT VALID;

ALTER TABLE workareas VALIDATE CONSTRAINT workareas_mode;
ALTER TABLE workareas VALIDATE CONSTRAINT workareas_universe_standard;
ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_profile;
