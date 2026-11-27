-- 0071_grant_create_flag — a grant may carry `create`, which lets its key
-- create child workspaces where the grant reaches (ADR-0097 d31).
--
-- `grant.issued` journals `create: true` for a flagged grant and nothing for
-- an unflagged one, so every grant stored before this file reads false, as
-- its event does. The column is written by that fold alone and never
-- changed: a grant is never edited in place.
--
-- What it costs a live database: one boolean column with a constant default,
-- which PostgreSQL adds without rewriting a row, under an ACCESS EXCLUSIVE
-- lock on grants waited for at most five seconds. Forward-only: no earlier
-- file is edited.

SET LOCAL lock_timeout = '5s';

ALTER TABLE grants
    ADD COLUMN may_create boolean NOT NULL DEFAULT false;
COMMENT ON COLUMN grants.may_create IS
    'The grant carries create: its key creates child workspaces where it reaches (ADR-0097 d31).';
