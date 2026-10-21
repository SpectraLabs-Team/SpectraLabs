-- Behaviour of the dataset-browser SQL in queries/*.sql (M1 task 6).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/dataset_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'dataset check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000000f1'
\set other '00000000-0000-7000-8000-0000000000f2'
\set c1 '1111111111111111111111111111111111111111111111111111111111111111'
\set c2 '2222222222222222222222222222222222222222222222222222222222222222'
\set c3 '3333333333333333333333333333333333333333333333333333333333333333'
\set b1 'b111111111111111111111111111111111111111111111111111111111111111'
\set b2 'b222222222222222222222222222222222222222222222222222222222222222'
\set b3 'b333333333333333333333333333333333333333333333333333333333333333'
\set file 'f000000000000000000000000000000000000000000000000000000000000000'

INSERT INTO workspaces (id, slug, name) VALUES (:'ws', 'dsc', 'Datasets'), (:'other', 'dsc-other', 'Other');
INSERT INTO uploads (id, workspace_id, sha256, size_bytes, filename, expires_at, ingested_at) VALUES
    ('00000000-0000-7000-8000-00000000f0a1', :'ws', :'file', 100, 'spy.csv', now() + interval '1 day', now());

-- spy: two versions (v2 from an upload); qqq: one version.
CREATE TEMP TABLE r1 AS EXECUTE dataset_version_register(:'ws', 'spy', :'c1', 100, 1000,
    '{"symbols":["SPY"],"first_ts_ns":1,"last_ts_ns":2,"rows_rejected":0,"warnings":[]}', NULL, :'b1', NULL);
CREATE TEMP TABLE r2 AS EXECUTE dataset_version_register(:'ws', 'spy', :'c2', 120, 1200,
    '{"symbols":["SPY"],"first_ts_ns":1,"last_ts_ns":3,"rows_rejected":2,"warnings":["a","b"]}', NULL, :'b2',
    '{"upload_id":"00000000-0000-7000-8000-00000000f0a1","filename":"spy.csv"}');
CREATE TEMP TABLE r3 AS EXECUTE dataset_version_register(:'ws', 'qqq', :'c3', 50, 500,
    '{"symbols":["QQQ"]}', NULL, :'b3', NULL);
SELECT id AS v1 FROM r1 \gset
SELECT id AS v2 FROM r2 \gset
SELECT id AS v3 FROM r3 \gset
SELECT dataset_id AS spy FROM dataset_versions WHERE id = :'v1' \gset
SELECT dataset_id AS qqq FROM dataset_versions WHERE id = :'v3' \gset

-- A finished backtest run on v1, and a queued backtest on v2.
INSERT INTO jobs (id, workspace_id, kind, status, payload, attempts, started_at, finished_at) VALUES
    ('00000000-0000-7000-8000-00000000f0b1', :'ws', 'backtest', 'succeeded', '{}', 1, now(), now());
INSERT INTO runs (id, workspace_id, job_id, dataset_version_id, status, metrics) VALUES
    ('00000000-0000-7000-8000-00000000f0c1', :'ws', '00000000-0000-7000-8000-00000000f0b1', :'v1', 'succeeded',
     '{"metrics":{"total_return":0.125}}');
INSERT INTO jobs (id, workspace_id, kind, status, payload) VALUES
    ('00000000-0000-7000-8000-00000000f0b2', :'ws', 'backtest', 'queued', jsonb_build_object('dataset_version_id', :'v2'));

-- The grouped list: newest first, with the latest version.
CREATE TEMP TABLE l1 AS EXECUTE dataset_list_grouped(:'ws', 50);
SELECT pg_temp.check((SELECT count(*) FROM l1) = 2, 'two datasets');
SELECT pg_temp.check((SELECT name FROM l1 ORDER BY updated_at DESC LIMIT 1) = 'qqq', 'newest first');
SELECT pg_temp.check((SELECT version_count FROM l1 WHERE name = 'spy') = 2, 'version count');
SELECT pg_temp.check((SELECT latest_version FROM l1 WHERE name = 'spy') = 2 AND (SELECT row_count FROM l1 WHERE name = 'spy') = 120,
                     'latest version');
SELECT pg_temp.check((SELECT symbols FROM l1 WHERE name = 'spy') = '["SPY"]'::jsonb AND (SELECT last_ts_ns FROM l1 WHERE name = 'spy') = 3,
                     'symbols and range from the report');
CREATE TEMP TABLE l2 AS EXECUTE dataset_list_grouped(:'other', 50);
SELECT pg_temp.check((SELECT count(*) FROM l2) = 0, 'per workspace');

-- One dataset, its versions, one version, its runs.
CREATE TEMP TABLE g1 AS EXECUTE dataset_get(:'ws', :'spy');
CREATE TEMP TABLE g2 AS EXECUTE dataset_get(:'other', :'spy');
SELECT pg_temp.check((SELECT name FROM g1) = 'spy' AND (SELECT count(*) FROM g2) = 0, 'get, scoped');
CREATE TEMP TABLE h1 AS EXECUTE dataset_versions_of(:'ws', :'spy');
SELECT pg_temp.check((SELECT array_agg(version ORDER BY version DESC) FROM h1) = ARRAY[2, 1], 'history newest first');
SELECT pg_temp.check((SELECT run_count FROM h1 WHERE version = 1) = 1 AND (SELECT run_count FROM h1 WHERE version = 2) = 0,
                     'run counts');
SELECT pg_temp.check((SELECT rows_rejected FROM h1 WHERE version = 2) = 2 AND (SELECT warning_count FROM h1 WHERE version = 2) = 2,
                     'report summary');
SELECT pg_temp.check((SELECT filename FROM h1 WHERE version = 2) = 'spy.csv' AND (SELECT filename FROM h1 WHERE version = 1) IS NULL,
                     'source file name');
CREATE TEMP TABLE vg AS EXECUTE dataset_version_get(:'ws', :'v2');
SELECT pg_temp.check((SELECT dataset_name FROM vg) = 'spy' AND (SELECT blob_sha256 FROM vg) = :'b2', 'version detail');
CREATE TEMP TABLE vg2 AS EXECUTE dataset_version_get(:'other', :'v2');
SELECT pg_temp.check((SELECT count(*) FROM vg2) = 0, 'version detail scoped');
CREATE TEMP TABLE vr AS EXECUTE dataset_version_runs(:'ws', :'v1', 20);
SELECT pg_temp.check((SELECT job_id FROM vr) = '00000000-0000-7000-8000-00000000f0b1' AND (SELECT total_return FROM vr) = 0.125,
                     'runs of a version');

-- Rename and describe; a taken name fails; another workspace changes nothing.
CREATE TEMP TABLE u1 AS EXECUTE dataset_update(:'ws', :'spy', ' spy daily ', NULL);
SELECT pg_temp.check((SELECT name FROM u1) = 'spy daily', 'renamed (trimmed)');
CREATE TEMP TABLE u2 AS EXECUTE dataset_update(:'ws', :'spy', NULL, 'S&P 500 ETF');
SELECT pg_temp.check((SELECT name FROM u2) = 'spy daily' AND (SELECT description FROM u2) = 'S&P 500 ETF', 'description only');
CREATE TEMP TABLE u3 AS EXECUTE dataset_update(:'other', :'spy', 'x', NULL);
SELECT pg_temp.check((SELECT count(*) FROM u3) = 0, 'update scoped');
DO $$
BEGIN
    EXECUTE 'EXECUTE dataset_update(''00000000-0000-7000-8000-0000000000f1'', '''
            || (SELECT dataset_id FROM dataset_versions WHERE checksum = repeat('3', 64)) || ''', ''spy daily'', NULL)';
    RAISE EXCEPTION 'dataset check failed: duplicate name accepted';
EXCEPTION WHEN unique_violation THEN NULL;
END $$;

-- Delete: refused while a run (v1) or a queued backtest (v2) uses it.
CREATE TEMP TABLE d1 AS EXECUTE dataset_version_delete(:'ws', :'v1');
SELECT pg_temp.check((SELECT found AND in_use AND NOT deleted FROM d1), 'used by a run: kept');
CREATE TEMP TABLE d2 AS EXECUTE dataset_version_delete(:'ws', :'v2');
SELECT pg_temp.check((SELECT in_use AND NOT deleted FROM d2), 'used by a queued backtest: kept');
CREATE TEMP TABLE d3 AS EXECUTE dataset_version_delete(:'other', :'v3');
SELECT pg_temp.check((SELECT NOT found AND NOT deleted FROM d3), 'another workspace: not found');

-- Once the backtest finished, v2 can go: the dataset stays (v1), the upload is released, the blob is orphaned.
UPDATE jobs SET status = 'cancelled', finished_at = now() WHERE id = '00000000-0000-7000-8000-00000000f0b2';
CREATE TEMP TABLE d4 AS EXECUTE dataset_version_delete(:'ws', :'v2');
SELECT pg_temp.check((SELECT deleted AND NOT dataset_deleted FROM d4), 'deleted, dataset kept');
SELECT pg_temp.check((SELECT orphaned_blob FROM d4) = :'b2', 'its file is orphaned');
SELECT pg_temp.check((SELECT ingested_at IS NULL AND expires_at <= now() FROM uploads WHERE id = '00000000-0000-7000-8000-00000000f0a1'),
                     'source upload released');
-- The last version of qqq takes the dataset with it; a blob still used elsewhere is not orphaned.
INSERT INTO uploads (workspace_id, sha256, size_bytes, expires_at) VALUES (:'other', :'b3', 10, now() + interval '1 day');
CREATE TEMP TABLE d5 AS EXECUTE dataset_version_delete(:'ws', :'v3');
SELECT pg_temp.check((SELECT deleted AND dataset_deleted FROM d5), 'last version: dataset deleted');
SELECT pg_temp.check((SELECT orphaned_blob FROM d5) IS NULL, 'file still referenced: kept');
SELECT pg_temp.check(NOT EXISTS (SELECT 1 FROM datasets WHERE id = :'qqq'), 'dataset row gone');

\echo dataset checks passed
ROLLBACK;
