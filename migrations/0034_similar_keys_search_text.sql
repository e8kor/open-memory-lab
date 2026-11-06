-- The near-copy check reads the text search reads (ADR-0056 d4; ADR-0050
-- d17; review of #90, L4): a version's keys are those of
-- coalesce(search_text, claim), so two diagrams that say one thing share
-- keys whatever their arrows look like, and a prose row's keys are those of
-- its claim, as before. The fold also fires on an update that names
-- search_text. The check's writer side and candidate comparison read the same
-- text in the application.
--
-- No row is refilled: 0032 is unreleased, so every row a deployment holds
-- has search_text NULL and keys of its claim, which are the keys of
-- coalesce(search_text, claim). A database that wrote a format claim between
-- 0032 and this file is refilled with oml_similar_keys_refill, as doctor
-- directs. Replacing a function and a trigger waits on no row; a lock is
-- waited for at most 5 s.

SET LOCAL lock_timeout = '5s';

CREATE OR REPLACE FUNCTION oml_similar_keys_fold() RETURNS trigger
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
    old_state smallint;
    new_state smallint;
    moved boolean := false;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        old_state := oml_similar_state(OLD.status, OLD.is_tip, OLD.retirement, OLD.kind);
    END IF;
    IF TG_OP <> 'DELETE' THEN
        new_state := oml_similar_state(NEW.status, NEW.is_tip, NEW.retirement, NEW.kind);
    END IF;
    IF TG_OP = 'UPDATE' THEN
        moved := OLD.claim IS DISTINCT FROM NEW.claim
              OR OLD.search_text IS DISTINCT FROM NEW.search_text
              OR OLD.workarea_id <> NEW.workarea_id
              OR OLD.kind IS DISTINCT FROM NEW.kind
              OR OLD.author_key_id IS DISTINCT FROM NEW.author_key_id;
        IF NOT moved AND old_state IS NOT DISTINCT FROM new_state THEN
            RETURN NULL;
        END IF;
    END IF;
    IF old_state IS NOT NULL AND (new_state IS NULL OR moved) THEN
        DELETE FROM similar_keys s WHERE s.tenant_id = OLD.tenant_id AND s.memory_id = OLD.id;
        old_state := NULL;
    END IF;
    IF new_state IS NULL THEN
        RETURN NULL;
    END IF;
    IF old_state IS NULL THEN
        INSERT INTO similar_keys (tenant_id, memory_id, workarea_id, author_key_id, key, band, state)
        SELECT NEW.tenant_id, NEW.id, NEW.workarea_id, NEW.author_key_id, k.key, k.band, new_state
          FROM oml_similar_keys(coalesce(NEW.search_text, NEW.claim)) k;
    ELSE
        UPDATE similar_keys s SET state = new_state
         WHERE s.tenant_id = NEW.tenant_id AND s.memory_id = NEW.id;
    END IF;
    RETURN NULL;
END
$$;

DROP TRIGGER memories_similar_keys ON memories;
CREATE TRIGGER memories_similar_keys
    AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, search_text, workarea_id, kind,
                              author_key_id
          OR DELETE
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_similar_keys_fold();

CREATE OR REPLACE FUNCTION oml_similar_keys_refill(t uuid) RETURNS void
    LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
    IF t IS DISTINCT FROM NULLIF(current_setting('oml.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'oml_similar_keys_refill(%) runs with oml.tenant_id set to that tenant', t;
    END IF;
    PERFORM pg_advisory_xact_lock(
        ('x' || encode(substr(sha256(uuid_send(t)), 1, 8), 'hex'))::bit(64)::bigint);
    DELETE FROM similar_keys s WHERE s.tenant_id = t;
    INSERT INTO similar_keys (tenant_id, memory_id, workarea_id, author_key_id, key, band, state)
    SELECT m.tenant_id, m.id, m.workarea_id, m.author_key_id, k.key, k.band,
           oml_similar_state(m.status, m.is_tip, m.retirement, m.kind)
      FROM memories m CROSS JOIN LATERAL oml_similar_keys(coalesce(m.search_text, m.claim)) k
     WHERE m.tenant_id = t
       AND oml_similar_state(m.status, m.is_tip, m.retirement, m.kind) IS NOT NULL;
END
$$;
