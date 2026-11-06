-- A code parent (ADR-0064 d3).
--
-- NUMBERING: 0036 is reserved for feat/implicit-capture in docs/NUMBERS.md.
--
-- workareas.children_agent is false unless a person's key marked the node a
-- code parent, journalled as workarea.mode_set with `children: agent` and
-- folded here alone. Under a code parent, and nowhere else, an agent writer
-- key creates in-progress children. The default is today's behaviour, so no
-- row changes meaning.
--
-- Safe on a live database: a lock is waited for at most 5 s, then the
-- migration fails and is run again. A column with a constant default is a
-- catalog change since PostgreSQL 11. The check is added NOT VALID and
-- validated in the same transaction; on workareas, small and unpartitioned,
-- the scan is brief.

SET LOCAL lock_timeout = '5s';

ALTER TABLE workareas
    ADD COLUMN children_agent boolean NOT NULL DEFAULT false;
-- A code parent is itself standard, so its children may each be in progress
-- (ADR-0030 d1 refuses nesting; ADR-0064 d3); `universe` is never one.
ALTER TABLE workareas
    ADD CONSTRAINT workareas_code_parent_standard
        CHECK (NOT children_agent OR (mode = 'standard' AND parent_id IS NOT NULL)) NOT VALID;
ALTER TABLE workareas VALIDATE CONSTRAINT workareas_code_parent_standard;
