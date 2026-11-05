-- 0025_similar_keys — the near-copy check's integer keys, which row security
-- lets an index serve, kept with each version's state by a trigger, and the
-- fill of the rows already written (ADR-0050 d1, d17).
-- Forward-only (docs/data/postgres-schema.md "Migrations"): no earlier file
-- is edited. Normative shapes: docs/data/postgres-schema.md "similar keys
-- (ADR-0050)".
--
-- A write is compared with the rows it may read by keys computed from words,
-- never by a text operator: under row security only leakproof operators
-- reach an index, and bigint and uuid equality are leakproof (ADR-0052
-- Context; ADR-0034 d13). Nothing here is SECURITY DEFINER and nothing is
-- marked LEAKPROOF.
--
-- Run by the owner role, with oml.serving_role set as 0007 requires.

------------------------------------------------------------------------------
-- oml_similar_words — a text's words as ADR-0050 d17 defines them: ASCII
-- letters lower-cased, split at every ASCII character that is not a letter
-- or a digit; a character outside ASCII belongs to its word. translate, not
-- lower, so no collation decides a case. oml_domain::similar::words computes
-- the same, held equal by a law.
------------------------------------------------------------------------------
CREATE FUNCTION oml_similar_words(t text) RETURNS text[]
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    RETURN array_remove(
        regexp_split_to_array(
            translate(t, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'),
            '[\x01-\x2f\x3a-\x40\x5b-\x60\x7b-\x7f]+'),
        '');

------------------------------------------------------------------------------
-- oml_similar_keys — a text's keys (d17): band 0, the same_text key, the id
-- of 'x:' and its words joined by single spaces; bands 1 to 10, the ids of
-- 'b<j>:' and the six minima of band j of a 60-permutation MinHash over the
-- distinct words' ids, permutation i mapping x to (a_i x + b_i) mod 2^31 - 1.
-- a_i and b_i are the ids of 'minhash:a:<i>' and 'minhash:b:<i>' reduced
-- into the field (a_i never 0), written out as constants: computing them per
-- call cost 6.5 ms a text, the constants 0.3 ms. No key for a text with no
-- word. oml_domain::similar computes the same,
-- held equal by a law.
------------------------------------------------------------------------------
CREATE FUNCTION oml_similar_keys(t text) RETURNS TABLE (band smallint, key bigint)
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    WITH w AS (SELECT oml_similar_words(t) AS words),
    ids AS (
        SELECT DISTINCT ((oml_token_id(x) % 2147483647) + 2147483647) % 2147483647 AS x
          FROM w, unnest(w.words) x),
    sig AS (
        SELECT p.i, min((p.a * ids.x + p.b) % 2147483647) AS m
          FROM unnest('{476861676,1515282218,1920958494,1357986051,1710795489,1427761765,
         71812234,913842131,1495627185,1414075433,190832847,1908531758,
         1359062085,1358768606,1625649955,301351999,1658226192,1128388107,
         1760657721,1354828801,1346167091,1958452912,534577678,1138851066,
         1838600336,1121893813,1118656884,1924391137,603356043,1793508964,
         1955920455,1926041472,102100381,1793693706,1055517955,1343195313,
         335171402,1377089306,1059877617,769226093,586163978,84094808,
         781013823,778656714,1160332682,414718673,302993545,684316145,
         2128700236,912109427,1332782912,1709194266,1518144297,1203815304,
         45200336,1352392827,318927021,352133606,1308267868,489097357}'::bigint[],
                      '{1784683121,1021439524,115854251,1425538005,951370155,1745344845,
         47701533,1690961993,819895383,662225352,2083788959,1505842054,
         849306189,1603201121,2090628866,1705660579,1662833718,1635850789,
         1737030563,1404547804,1449230118,1186422257,490623381,1808389197,
         964859242,406618249,1579806299,1425210676,592435309,655395402,
         1340155608,998133000,1033963231,512466687,378903634,65927014,
         48635188,1145917156,990859507,276958223,196783123,1756716089,
         2034670326,791218543,1407227686,1604708157,548791024,1795885952,
         2059854284,1296707158,974749547,310677980,1448893343,850568316,
         574208637,1239429189,1207805636,1311474768,55184435,843949105}'::bigint[])
               WITH ORDINALITY p(a, b, i)
         CROSS JOIN ids
         GROUP BY p.i)
    SELECT 0::smallint, oml_token_id('x:' || array_to_string(w.words, ' '))
      FROM w WHERE cardinality(w.words) > 0
    UNION ALL
    SELECT ((sig.i - 1) / 6 + 1)::smallint,
           oml_token_id('b' || ((sig.i - 1) / 6 + 1) || ':'
                        || string_agg(sig.m::text, ',' ORDER BY sig.i))
      FROM sig
     GROUP BY (sig.i - 1) / 6
$$;

COMMENT ON FUNCTION oml_similar_keys(text) IS
    'A text''s same_text key (band 0) and MinHash band keys (bands 1-10) (ADR-0050 d17). '
    'Owning page: docs/data/postgres-schema.md.';

-- A version's similar state: 0 held, 1 a served tip, or NULL for a version
-- the check never names: replaced, demoted, rejected or erased, a retirement
-- version, and a kind other than memory, derived_fact and steering (d1).
CREATE FUNCTION oml_similar_state(status text, is_tip boolean, retirement boolean, kind text)
    RETURNS smallint
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    RETURN CASE
        WHEN retirement OR kind IS NULL OR kind NOT IN ('memory', 'derived_fact', 'steering') THEN NULL
        WHEN status = 'held' THEN 0
        WHEN status = 'promoted' AND is_tip THEN 1
        ELSE NULL
    END;

------------------------------------------------------------------------------
-- similar_keys — one row per (version, key) of a held version or a served
-- tip, and none for any other (d17). List-partitioned by tenant_id, one
-- partition similar_keys_t_<tenant id, 32 hex> per tenant, each with ENABLE
-- and FORCE ROW LEVEL SECURITY and the tenant_isolation policy, as
-- search_postings' are; a key computed from one tenant's text meets only
-- that tenant's partition. similar_keys_key serves the check's one read by
-- key and workarea and carries every column it filters on, so the read is
-- an index-only scan; similar_keys_memory serves the trigger's flips and
-- the erase. No foreign key, as search_postings has none.
------------------------------------------------------------------------------
CREATE TABLE similar_keys (
    tenant_id     uuid     NOT NULL,
    memory_id     uuid     NOT NULL,
    workarea_id   uuid     NOT NULL,
    author_key_id uuid     NOT NULL,
    key           bigint   NOT NULL,
    band          smallint NOT NULL,
    state         smallint NOT NULL,
    CONSTRAINT similar_keys_band CHECK (band BETWEEN 0 AND 10),
    CONSTRAINT similar_keys_state CHECK (state IN (0, 1))
) PARTITION BY LIST (tenant_id);

CREATE INDEX similar_keys_key ON similar_keys (key, workarea_id)
    INCLUDE (state, author_key_id, memory_id, tenant_id);
CREATE INDEX similar_keys_memory ON similar_keys (memory_id);

-- No statistics on the parent, as search_postings (FR-45).
ALTER TABLE ONLY similar_keys
    ALTER COLUMN key           SET STATISTICS 0,
    ALTER COLUMN memory_id     SET STATISTICS 0,
    ALTER COLUMN workarea_id   SET STATISTICS 0,
    ALTER COLUMN author_key_id SET STATISTICS 0;

ALTER TABLE similar_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE similar_keys FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON similar_keys
    USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid);

COMMENT ON TABLE similar_keys IS
    'Per held version or served tip and key, the near-copy check''s same_text and MinHash band '
    'keys, read by index under row security, list-partitioned by tenant_id (ADR-0050 d17). '
    'Owning page: docs/data/postgres-schema.md.';

DO $$
BEGIN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON similar_keys TO %I',
                   current_setting('oml.serving_role'));
END $$;

------------------------------------------------------------------------------
-- oml_similar_keys_fold — keys follow the version (d17): written with a
-- held version, their state set at promotion, deleted when the version is
-- replaced, demoted, rejected, erased or deleted; a version whose claim,
-- workarea, author or kind changes gives its keys back and takes new ones.
------------------------------------------------------------------------------
CREATE FUNCTION oml_similar_keys_fold() RETURNS trigger
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
        moved := OLD.claim IS DISTINCT FROM NEW.claim OR OLD.workarea_id <> NEW.workarea_id
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
          FROM oml_similar_keys(NEW.claim) k;
    ELSE
        UPDATE similar_keys s SET state = new_state
         WHERE s.tenant_id = NEW.tenant_id AND s.memory_id = NEW.id;
    END IF;
    RETURN NULL;
END
$$;

------------------------------------------------------------------------------
-- One partition per existing tenant, fenced before it is attached, as 0021
-- builds search_postings' and the lead builds a new tenant's.
------------------------------------------------------------------------------
DO $$
DECLARE
    t         uuid;
    partition text;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        partition := 'similar_keys_t_' || replace(t::text, '-', '');
        EXECUTE format('CREATE TABLE %I (LIKE similar_keys INCLUDING ALL)', partition);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', partition);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', partition);
        EXECUTE format(
            'CREATE POLICY tenant_isolation ON %I '
            'USING (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid) '
            'WITH CHECK (tenant_id = NULLIF(current_setting(''oml.tenant_id'', true), '''')::uuid)',
            partition);
        EXECUTE format('ALTER TABLE similar_keys ATTACH PARTITION %I FOR VALUES IN (%L)',
                       partition, t);
    END LOOP;
    PERFORM set_config('oml.startup_check', '', true);
END $$;

-- The trigger first, so its lock on memories is held to the end of the
-- fill and no write lands between the fill's read and its commit.
CREATE TRIGGER memories_similar_keys
    AFTER INSERT OR UPDATE OF status, is_tip, retirement, claim, workarea_id, kind, author_key_id
          OR DELETE
    ON memories FOR EACH ROW EXECUTE FUNCTION oml_similar_keys_fold();

------------------------------------------------------------------------------
-- oml_similar_keys_refill — one tenant's keys rebuilt from its versions'
-- claims; 0025 runs it for every tenant, the owner for one when doctor names
-- a fault (d15), with oml.tenant_id set to that tenant, under the tenant's
-- chain lock, as oml_search_postings_refill. Owner only.
------------------------------------------------------------------------------
CREATE FUNCTION oml_similar_keys_refill(t uuid) RETURNS void
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
      FROM memories m CROSS JOIN LATERAL oml_similar_keys(m.claim) k
     WHERE m.tenant_id = t
       AND oml_similar_state(m.status, m.is_tip, m.retirement, m.kind) IS NOT NULL;
END
$$;

DO $$
DECLARE
    t uuid;
BEGIN
    PERFORM set_config('oml.startup_check', 'on', true);
    FOR t IN SELECT id FROM tenants ORDER BY id LOOP
        PERFORM set_config('oml.tenant_id', t::text, true);
        PERFORM oml_similar_keys_refill(t);
    END LOOP;
    PERFORM set_config('oml.tenant_id', '', true);
    PERFORM set_config('oml.startup_check', '', true);
END
$$;

REVOKE EXECUTE ON FUNCTION oml_similar_keys_refill(uuid) FROM PUBLIC;
