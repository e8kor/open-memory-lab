-- A destroyed data key stays destroyed, a destruction is stamped with its own
-- transaction's time, and a live key is re-keyed only with its KEK (ADR-0005
-- d2, amended 2026-11-17; ADR-0060 d6; #455).
--
-- 0056 granted the serving role UPDATE on wrapped_dek and destroyed_at so an
-- erase destroys a version's key in the transaction that journals it. A
-- column grant cannot say which values: by itself it let the serving role
-- write a new wrapped key into a live row, write one back into a destroyed
-- row, and move a destruction's time. This trigger, owned by the owner and
-- run with the caller's rights, refuses each of those for every role:
--
--   (a) a destroyed row (OLD.destroyed_at set) keeps its wrapped_dek,
--       destroyed_at and kek_id, so it is never keyed again and its time
--       never moves; the lake writer's destroy_key upsert on a destroyed row
--       writes the same values and passes;
--   (b) a live row takes a new non-NULL wrapped_dek only together with a new
--       kek_id, the lake writer's re-wrap under the current KEK; the serving
--       role holds no grant on kek_id, so the one change it can make is the
--       destruction;
--   (c) a destruction (destroyed_at from NULL to a value) is stamped now(),
--       the time of the transaction that destroys it, as the erase and
--       destroy_key both write it.
--
-- The CHECK artifact_keys_destroyed (0031) already ties a NULL wrapped_dek to
-- a set destroyed_at.
--
-- What it costs a live database: catalog changes only; no table is scanned.
-- A lock on artifact_keys is waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

CREATE OR REPLACE FUNCTION oml_artifact_keys_guard() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF OLD.destroyed_at IS NOT NULL THEN
        IF NEW.wrapped_dek IS DISTINCT FROM OLD.wrapped_dek
           OR NEW.destroyed_at IS DISTINCT FROM OLD.destroyed_at
           OR NEW.kek_id IS DISTINCT FROM OLD.kek_id THEN
            RAISE EXCEPTION 'artifact_keys_guard: a destroyed data key stays destroyed'
                USING ERRCODE = 'check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.wrapped_dek IS NOT NULL
       AND NEW.wrapped_dek IS DISTINCT FROM OLD.wrapped_dek
       AND NEW.kek_id IS NOT DISTINCT FROM OLD.kek_id THEN
        RAISE EXCEPTION 'artifact_keys_guard: a live data key is re-keyed only with a new KEK'
            USING ERRCODE = 'check_violation';
    END IF;
    IF NEW.destroyed_at IS NOT NULL AND NEW.destroyed_at IS DISTINCT FROM now() THEN
        RAISE EXCEPTION 'artifact_keys_guard: a destruction is stamped with its own time'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER artifact_keys_guard
    BEFORE UPDATE ON artifact_keys
    FOR EACH ROW EXECUTE FUNCTION oml_artifact_keys_guard();

COMMENT ON FUNCTION oml_artifact_keys_guard() IS
    'Refuses an update of artifact_keys that re-keys a destroyed row, moves its destroyed_at or '
    'kek_id, writes a new wrapped key into a live row without a new kek_id, or stamps a destruction '
    'with a time other than its transaction''s (0057; ADR-0005 d2).';
