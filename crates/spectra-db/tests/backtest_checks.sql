-- Behaviour of the dataset-version and backtest-run SQL in queries/*.sql (M3 task 5).
--   node crates/spectra-db/scripts/prepare-queries.mjs > /tmp/prep.sql
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f /tmp/prep.sql -f crates/spectra-db/tests/backtest_checks.sql
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

INSERT INTO users (id, email, password_hash) VALUES ('00000000-0000-7000-8000-000000000011', 'bt@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000000c1', 'btc', 'C'),
    ('00000000-0000-7000-8000-0000000000d1', 'btd', 'D');

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'backtest check failed: %', what; END IF; END $$;

\set sum1 '1111111111111111111111111111111111111111111111111111111111111111'
\set sum2 '2222222222222222222222222222222222222222222222222222222222222222'

-- Workspaces resolve by id or slug.
CREATE TEMP TABLE w1 AS EXECUTE workspace_resolve('btc');
CREATE TEMP TABLE w2 AS EXECUTE workspace_resolve('00000000-0000-7000-8000-0000000000C1');
CREATE TEMP TABLE w3 AS EXECUTE workspace_resolve('nope');
SELECT pg_temp.check((SELECT id FROM w1) = '00000000-0000-7000-8000-0000000000c1', 'resolve by slug');
SELECT pg_temp.check((SELECT id FROM w2) = '00000000-0000-7000-8000-0000000000c1', 'resolve by id, any case');
SELECT pg_temp.check((SELECT count(*) FROM w3) = 0, 'unknown workspace');

-- Register: first version is 1; the same checksum again returns it; a new checksum is version 2.
CREATE TEMP TABLE r1 AS EXECUTE dataset_version_register('00000000-0000-7000-8000-0000000000c1', ' spy daily ', :'sum1', 2400, 9000, '{}', '00000000-0000-7000-8000-000000000011');
CREATE TEMP TABLE r2 AS EXECUTE dataset_version_register('00000000-0000-7000-8000-0000000000c1', 'spy daily', :'sum1', 2400, 9000, '{}', NULL);
CREATE TEMP TABLE r3 AS EXECUTE dataset_version_register('00000000-0000-7000-8000-0000000000c1', 'spy daily', :'sum2', 2500, 9100, '{"issues":0}', NULL);
SELECT pg_temp.check((SELECT version FROM r1) = 1 AND NOT (SELECT existed FROM r1), 'first version');
SELECT pg_temp.check((SELECT id FROM r2) = (SELECT id FROM r1) AND (SELECT existed FROM r2), 'same checksum reuses the version');
SELECT pg_temp.check((SELECT version FROM r3) = 2 AND NOT (SELECT existed FROM r3), 'new checksum is the next version');
SELECT pg_temp.check((SELECT count(*) FROM datasets WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1') = 1,
                     'one dataset (name trimmed)');

-- The same checksum in another workspace is a separate dataset and version.
CREATE TEMP TABLE r4 AS EXECUTE dataset_version_register('00000000-0000-7000-8000-0000000000d1', 'spy daily', :'sum1', 2400, 9000, '{}', NULL);
SELECT pg_temp.check((SELECT version FROM r4) = 1 AND (SELECT id FROM r4) <> (SELECT id FROM r1), 'per-workspace');

-- Resolve only inside the owning workspace.
SELECT id AS v1 FROM r1 \gset
CREATE TEMP TABLE s1 AS EXECUTE dataset_version_resolve('00000000-0000-7000-8000-0000000000c1', :'v1');
CREATE TEMP TABLE s2 AS EXECUTE dataset_version_resolve('00000000-0000-7000-8000-0000000000d1', :'v1');
SELECT pg_temp.check((SELECT checksum FROM s1) = :'sum1' AND (SELECT row_count FROM s1) = 2400
                     AND (SELECT dataset_name FROM s1) = 'spy daily' AND (SELECT version FROM s1) = 1, 'resolve');
SELECT pg_temp.check((SELECT count(*) FROM s2) = 0, 'another workspace cannot resolve it');

-- A backtest run links its job and dataset version.
CREATE TEMP TABLE j AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000c1', 'backtest', '{}', 0, 3, NULL);
SELECT id AS job FROM j \gset
CREATE TEMP TABLE run AS EXECUTE run_insert_backtest('00000000-0000-7000-8000-0000000000e1', '00000000-0000-7000-8000-0000000000c1', :'job', '{"strategy":"sma_cross"}', '{"schema":"spectra.backtest.summary/v1"}', 7, '0.7.0', '00000000-0000-7000-8000-000000000011', now(), :'v1');
SELECT pg_temp.check((SELECT kind FROM runs WHERE id = '00000000-0000-7000-8000-0000000000e1') = 'backtest', 'run kind');
SELECT pg_temp.check((SELECT dataset_version_id FROM runs WHERE id = '00000000-0000-7000-8000-0000000000e1') = :'v1', 'run dataset version');
SELECT pg_temp.check((SELECT status FROM runs WHERE id = '00000000-0000-7000-8000-0000000000e1') = 'succeeded', 'run status');

-- Artifacts are found by run and kind, only inside the run's workspace.
INSERT INTO run_artifacts (workspace_id, run_id, kind, sha256, path, size_bytes, content_type)
VALUES ('00000000-0000-7000-8000-0000000000c1', '00000000-0000-7000-8000-0000000000e1', 'equity_series',
        :'sum2', 'sha256/22/22/x', 42, 'application/json');
CREATE TEMP TABLE a1 AS EXECUTE run_artifact_get('00000000-0000-7000-8000-0000000000c1', '00000000-0000-7000-8000-0000000000e1', 'equity_series');
CREATE TEMP TABLE a2 AS EXECUTE run_artifact_get('00000000-0000-7000-8000-0000000000d1', '00000000-0000-7000-8000-0000000000e1', 'equity_series');
CREATE TEMP TABLE a3 AS EXECUTE run_artifact_get('00000000-0000-7000-8000-0000000000c1', '00000000-0000-7000-8000-0000000000e1', 'drawdown_series');
SELECT pg_temp.check((SELECT sha256 FROM a1) = :'sum2' AND (SELECT size_bytes FROM a1) = 42
                     AND (SELECT content_type FROM a1) = 'application/json', 'artifact by kind');
SELECT pg_temp.check((SELECT count(*) FROM a2) = 0, 'another workspace cannot read the artifact');
SELECT pg_temp.check((SELECT count(*) FROM a3) = 0, 'missing kind');

-- A run cannot point at another workspace's dataset version (composite foreign key).
DO $$
BEGIN
    INSERT INTO runs (workspace_id, kind, dataset_version_id)
    SELECT '00000000-0000-7000-8000-0000000000d1', 'backtest', id FROM dataset_versions
     WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1' LIMIT 1;
    RAISE EXCEPTION 'backtest check failed: cross-workspace dataset version accepted';
EXCEPTION WHEN foreign_key_violation THEN NULL;
END $$;

\echo backtest checks passed
ROLLBACK;
