-- Behaviour of the saved-strategy SQL in queries/strategy_*.sql (M2a tasks 2 and 3).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/strategy_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

INSERT INTO users (id, email, password_hash) VALUES ('00000000-0000-7000-8000-0000000002a1', 'st@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000002c1', 'stc', 'C'),
    ('00000000-0000-7000-8000-0000000002d1', 'std', 'D');

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'strategy check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000002c1'
\set other '00000000-0000-7000-8000-0000000002d1'
\set user '00000000-0000-7000-8000-0000000002a1'
\set src1 '1111111111111111111111111111111111111111111111111111111111111111'
\set src2 '2222222222222222222222222222222222222222222222222222222222222222'
\set cfg1 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
\set cfg2 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'

-- Create: a new name gives a row with no versions; the same name again gives none.
CREATE TEMP TABLE s1 AS EXECUTE strategy_create(:'ws', 'Trend', 'SMA on SPY', :'user');
CREATE TEMP TABLE s2 AS EXECUTE strategy_create(:'ws', 'Trend', '', NULL);
CREATE TEMP TABLE s3 AS EXECUTE strategy_create(:'other', 'Trend', '', NULL);
SELECT id AS sid FROM s1 \gset
SELECT pg_temp.check((SELECT version_count FROM s1) = 0 AND (SELECT latest_version FROM s1) IS NULL, 'new strategy');
SELECT pg_temp.check((SELECT count(*) FROM s2) = 0, 'duplicate name in a workspace');
SELECT pg_temp.check((SELECT count(*) FROM s3) = 1, 'same name in another workspace');

-- Lock: only inside the workspace.
CREATE TEMP TABLE k1 AS EXECUTE strategy_lock(:'ws', :'sid');
CREATE TEMP TABLE k2 AS EXECUTE strategy_lock(:'other', :'sid');
SELECT pg_temp.check((SELECT count(*) FROM k1) = 1 AND (SELECT count(*) FROM k2) = 0, 'lock is workspace scoped');

-- Insert numbers versions 1, 2, ...; find returns a saved (source, config) pair.
CREATE TEMP TABLE v1 AS EXECUTE strategy_version_insert(:'ws', :'sid', 'sma_cross', :'src1', :'cfg1', '{"params":[]}', '{"fast":10,"slow":50}', :'user');
CREATE TEMP TABLE f1 AS EXECUTE strategy_version_find(:'ws', :'sid', :'src1', :'cfg1');
CREATE TEMP TABLE f2 AS EXECUTE strategy_version_find(:'ws', :'sid', :'src1', :'cfg2');
CREATE TEMP TABLE f3 AS EXECUTE strategy_version_find(:'other', :'sid', :'src1', :'cfg1');
SELECT id AS v1id FROM v1 \gset
SELECT pg_temp.check((SELECT version FROM v1) = 1 AND (SELECT strategy_name FROM v1) = 'Trend', 'first version');
SELECT pg_temp.check((SELECT parameters FROM v1) = '{"fast":10,"slow":50}'::jsonb, 'parameters stored');
SELECT pg_temp.check((SELECT id FROM f1) = :'v1id', 'find the saved version');
SELECT pg_temp.check((SELECT count(*) FROM f2) = 0, 'other parameters are not saved');
SELECT pg_temp.check((SELECT count(*) FROM f3) = 0, 'find is workspace scoped');

CREATE TEMP TABLE v2 AS EXECUTE strategy_version_insert(:'ws', :'sid', 'sma_cross', :'src1', :'cfg2', '{}', '{"fast":5,"slow":50}', NULL);
CREATE TEMP TABLE v3 AS EXECUTE strategy_version_insert(:'ws', :'sid', 'sma_cross', :'src2', :'cfg1', '{}', '{"fast":10,"slow":50}', NULL);
SELECT pg_temp.check((SELECT version FROM v2) = 2 AND (SELECT version FROM v3) = 3, 'versions count up');

-- The same code and parameters twice is refused by the unique index (the API finds it first).
DO $$
BEGIN
    INSERT INTO strategy_versions (workspace_id, strategy_id, version, builtin, source_hash, config_hash)
    SELECT workspace_id, strategy_id, 9, builtin, source_hash, config_hash FROM strategy_versions WHERE version = 1
       AND strategy_id = (SELECT id FROM strategies WHERE workspace_id = '00000000-0000-7000-8000-0000000002c1');
    RAISE EXCEPTION 'strategy check failed: duplicate version inserted';
EXCEPTION WHEN unique_violation THEN NULL;
END $$;

-- Versions are immutable.
DO $$
BEGIN
    UPDATE strategy_versions SET parameters = '{"fast":1}'::jsonb
     WHERE workspace_id = '00000000-0000-7000-8000-0000000002c1' AND version = 1;
    RAISE EXCEPTION 'strategy check failed: a version was updated';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'strategy versions are immutable' THEN RAISE; END IF;
END $$;

-- A built-in name must look like one, and parameters must be an object.
DO $$
BEGIN
    INSERT INTO strategy_versions (workspace_id, strategy_id, version, builtin, source_hash, config_hash)
    VALUES ('00000000-0000-7000-8000-0000000002c1', (SELECT id FROM strategies WHERE name = 'Trend'
            AND workspace_id = '00000000-0000-7000-8000-0000000002c1'), 20, 'Not A Name', repeat('c', 64), repeat('c', 64));
    RAISE EXCEPTION 'strategy check failed: bad builtin name accepted';
EXCEPTION WHEN check_violation THEN NULL;
END $$;
DO $$
BEGIN
    INSERT INTO strategy_versions (workspace_id, strategy_id, version, builtin, source_hash, config_hash, parameters)
    VALUES ('00000000-0000-7000-8000-0000000002c1', (SELECT id FROM strategies WHERE name = 'Trend'
            AND workspace_id = '00000000-0000-7000-8000-0000000002c1'), 21, 'sma_cross', repeat('d', 64), repeat('d', 64), '[]');
    RAISE EXCEPTION 'strategy check failed: array parameters accepted';
EXCEPTION WHEN check_violation THEN NULL;
END $$;

-- List and get show counts and the latest version; another workspace sees nothing.
CREATE TEMP TABLE l1 AS EXECUTE strategy_list(:'ws', 50);
CREATE TEMP TABLE g1 AS EXECUTE strategy_get(:'ws', :'sid');
CREATE TEMP TABLE g2 AS EXECUTE strategy_get(:'other', :'sid');
SELECT pg_temp.check((SELECT count(*) FROM l1) = 1, 'list');
SELECT pg_temp.check((SELECT version_count FROM g1) = 3 AND (SELECT latest_version FROM g1) = 3
                     AND (SELECT latest_version_id FROM g1) = (SELECT id FROM v3), 'latest version');
SELECT pg_temp.check((SELECT count(*) FROM g2) = 0, 'get is workspace scoped');

-- Versions of a strategy, newest first; one version by id, only in its workspace.
CREATE TEMP TABLE vs AS EXECUTE strategy_versions_of(:'ws', :'sid');
SELECT pg_temp.check((SELECT array_agg(version) FROM vs) = ARRAY[3, 2, 1], 'versions newest first');
CREATE TEMP TABLE vg1 AS EXECUTE strategy_version_get(:'ws', :'v1id');
CREATE TEMP TABLE vg2 AS EXECUTE strategy_version_get(:'other', :'v1id');
SELECT pg_temp.check((SELECT builtin FROM vg1) = 'sma_cross' AND (SELECT source_hash FROM vg1) = :'src1', 'version by id');
SELECT pg_temp.check((SELECT count(*) FROM vg2) = 0, 'version get is workspace scoped');

-- A backtest run records its version, strategy and code hash; a version of another workspace is refused.
CREATE TEMP TABLE dv AS EXECUTE dataset_version_register(:'ws', 'spy', :'src2', 10, 100, '{}', NULL, NULL, NULL);
SELECT id AS dvid FROM dv \gset
CREATE TEMP TABLE j AS EXECUTE job_enqueue(:'ws', 'backtest', '{}', 0, 3, NULL);
SELECT id AS job FROM j \gset
CREATE TEMP TABLE r AS EXECUTE run_insert_backtest('00000000-0000-7000-8000-0000000002e1', :'ws', :'job', '{}', '{}', NULL, '0.7.0', NULL, now(), :'dvid', :'v1id', 'sma_cross', :'src1');
SELECT pg_temp.check((SELECT strategy_version_id FROM runs WHERE id = '00000000-0000-7000-8000-0000000002e1') = :'v1id'
                     AND (SELECT strategy_sha256 FROM runs WHERE id = '00000000-0000-7000-8000-0000000002e1') = :'src1'
                     AND (SELECT strategy FROM runs WHERE id = '00000000-0000-7000-8000-0000000002e1') = 'sma_cross',
                     'run records the version');
DO $$
BEGIN
    INSERT INTO runs (workspace_id, kind, status, finished_at, strategy_version_id)
    SELECT '00000000-0000-7000-8000-0000000002d1', 'backtest', 'succeeded', now(), id
      FROM strategy_versions WHERE workspace_id = '00000000-0000-7000-8000-0000000002c1' AND version = 1;
    RAISE EXCEPTION 'strategy check failed: a run used another workspace''s version';
EXCEPTION WHEN foreign_key_violation THEN NULL;
END $$;

\echo strategy checks passed
ROLLBACK;
