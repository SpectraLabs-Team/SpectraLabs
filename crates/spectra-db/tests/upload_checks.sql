-- Behaviour of the upload, ingest and storage SQL in queries/*.sql (M1 tasks 4-5).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/upload_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'upload check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000000e1'
\set other '00000000-0000-7000-8000-0000000000e2'
\set user '00000000-0000-7000-8000-0000000000e9'
\set f1 '1111111111111111111111111111111111111111111111111111111111111111'
\set f2 '2222222222222222222222222222222222222222222222222222222222222222'
\set f3 '3333333333333333333333333333333333333333333333333333333333333333'
\set bars '4444444444444444444444444444444444444444444444444444444444444444'
\set ds1 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'

INSERT INTO users (id, email, password_hash) VALUES (:'user', 'up@example.com', '!');
INSERT INTO workspaces (id, slug, name, artifact_quota_bytes) VALUES
    (:'ws', 'upl', 'Uploads', 1000),
    (:'other', 'upl-other', 'Other', NULL);

-- Uploads count against the quota; the same bytes again cost nothing.
CREATE TEMP TABLE u1 AS EXECUTE upload_insert(:'ws', :'f1', 600, 'prices.csv', :'user', 5000, 86400);
CREATE TEMP TABLE u2 AS EXECUTE upload_insert(:'ws', :'f2', 600, 'more.csv', :'user', 5000, 86400);
CREATE TEMP TABLE u3 AS EXECUTE upload_insert(:'ws', :'f1', 600, 'prices again.csv', NULL, 5000, 86400);
SELECT id AS u1 FROM u1 \gset
SELECT id AS u3 FROM u3 \gset
SELECT pg_temp.check((SELECT count(*) FROM u1) = 1, 'first upload recorded');
SELECT pg_temp.check((SELECT count(*) FROM u2) = 0, 'over the 1000-byte quota');
SELECT pg_temp.check((SELECT count(*) FROM u3) = 1, 'same bytes again: no extra usage');
SELECT pg_temp.check((SELECT expires_at FROM u1) BETWEEN now() + interval '23 hours' AND now() + interval '25 hours',
                     'expires in 24 hours');
CREATE TEMP TABLE qa AS EXECUTE artifact_quota(:'ws', :'f1', 5000);
SELECT pg_temp.check((SELECT used FROM qa) = 600 AND (SELECT present FROM qa), 'quota sees the upload once');
CREATE TEMP TABLE qb AS EXECUTE artifact_quota(:'other', :'f1', 5000);
SELECT pg_temp.check((SELECT used FROM qb) = 0 AND NOT (SELECT present FROM qb), 'per workspace');
-- A long file name is cut, not refused.
CREATE TEMP TABLE u4 AS EXECUTE upload_insert(:'other', :'f3', 10, repeat('x', 300), NULL, 5000, 86400);
SELECT pg_temp.check((SELECT length(filename) FROM uploads WHERE id = (SELECT id FROM u4)) = 255, 'name cut');

-- Get: only the owning workspace, and not once expired.
CREATE TEMP TABLE g1 AS EXECUTE upload_get(:'ws', :'u1');
CREATE TEMP TABLE g2 AS EXECUTE upload_get(:'other', :'u1');
SELECT pg_temp.check((SELECT sha256 FROM g1) = :'f1' AND (SELECT size_bytes FROM g1) = 600, 'get');
SELECT pg_temp.check((SELECT count(*) FROM g2) = 0, 'cross-workspace get refused');

-- Dataset files in the artifact store count too; registering keeps the source.
CREATE TEMP TABLE r1 AS EXECUTE dataset_version_register(:'ws', 'spy', :'ds1', 10, 300, '{}', :'user', :'bars',
    '{"upload_id":"x","filename":"prices.csv"}');
SELECT pg_temp.check((SELECT version FROM r1) = 1, 'registered');
SELECT pg_temp.check((SELECT blob_sha256 FROM dataset_versions WHERE id = (SELECT id FROM r1)) = :'bars', 'blob kept');
SELECT pg_temp.check((SELECT source->>'filename' FROM dataset_versions WHERE id = (SELECT id FROM r1)) = 'prices.csv',
                     'source kept');
CREATE TEMP TABLE qc AS EXECUTE artifact_quota(:'ws', :'bars', 5000);
SELECT pg_temp.check((SELECT used FROM qc) = 900 AND (SELECT present FROM qc), 'dataset file counted');
SELECT id AS r1 FROM r1 \gset
CREATE TEMP TABLE rv AS EXECUTE dataset_version_resolve(:'ws', :'r1');
SELECT pg_temp.check((SELECT blob_sha256 FROM rv) = :'bars', 'resolve returns the blob');

-- A command-line version (no blob) gains one when the same content is ingested again.
CREATE TEMP TABLE c1 AS EXECUTE dataset_version_register(:'ws', 'cli', :'ds1', 10, 300, '{}', NULL, NULL, NULL);
SELECT pg_temp.check((SELECT blob_sha256 FROM dataset_versions WHERE id = (SELECT id FROM c1)) IS NULL, 'cli: no blob');
CREATE TEMP TABLE c2 AS EXECUTE dataset_version_register(:'ws', 'cli', :'ds1', 10, 300, '{}', :'user', :'bars', '{}');
SELECT pg_temp.check((SELECT id FROM c2) = (SELECT id FROM c1) AND (SELECT existed FROM c2), 'same content: no new version');
SELECT pg_temp.check((SELECT blob_sha256 FROM dataset_versions WHERE id = (SELECT id FROM c1)) = :'bars', 'blob backfilled');
SELECT pg_temp.check((SELECT source FROM dataset_versions WHERE id = (SELECT id FROM c1)) IS NULL, 'existing row otherwise untouched');

-- Ingested uploads are kept; expired, un-ingested ones go, and only orphaned hashes come back.
EXECUTE upload_mark_ingested(:'ws', :'u1');
UPDATE uploads SET expires_at = now() - interval '1 minute' WHERE workspace_id IN (:'ws', :'other');
CREATE TEMP TABLE gx AS EXECUTE upload_get(:'ws', :'u1');
SELECT pg_temp.check((SELECT count(*) FROM gx) = 1, 'ingested uploads never expire');
CREATE TEMP TABLE gy AS EXECUTE upload_get(:'ws', :'u3');
SELECT pg_temp.check((SELECT count(*) FROM gy) = 0, 'expired upload is gone for get');
CREATE TEMP TABLE e1 AS EXECUTE upload_expire(100);
-- u3 (f1) expired, but u1 still references f1; u4 (f3) is referenced by nothing else.
SELECT pg_temp.check(EXISTS (SELECT 1 FROM e1 WHERE sha256 = :'f3'), 'orphaned hash returned');
SELECT pg_temp.check(NOT EXISTS (SELECT 1 FROM e1 WHERE sha256 = :'f1'), 'hash still used by an upload kept');
SELECT pg_temp.check(NOT EXISTS (SELECT 1 FROM uploads WHERE id IN (SELECT id FROM u3 UNION SELECT id FROM u4)),
                     'expired rows removed');
SELECT pg_temp.check(EXISTS (SELECT 1 FROM uploads WHERE id = (SELECT id FROM u1)), 'ingested row kept');
-- A run artifact with the hash also keeps the bytes.
INSERT INTO jobs (id, workspace_id, kind) VALUES ('00000000-0000-7000-8000-00000000e001', :'other', 'risk');
INSERT INTO runs (id, workspace_id, job_id) VALUES ('00000000-0000-7000-8000-00000000e002', :'other', '00000000-0000-7000-8000-00000000e001');
INSERT INTO run_artifacts (workspace_id, run_id, kind, sha256, path, size_bytes)
VALUES (:'other', '00000000-0000-7000-8000-00000000e002', 'report', :'f2', 'x', 5);
INSERT INTO uploads (workspace_id, sha256, size_bytes, expires_at) VALUES (:'ws', :'f2', 5, now() - interval '1 second');
CREATE TEMP TABLE e2 AS EXECUTE upload_expire(100);
SELECT pg_temp.check(NOT EXISTS (SELECT 1 FROM e2 WHERE sha256 = :'f2'), 'hash still used by a run artifact');

-- The audit log.
CREATE TEMP TABLE a1 AS EXECUTE audit_insert(:'ws', :'user', 'upload.created', 'upload', 'u-1', 'req-1', '{"size":600}');
SELECT pg_temp.check((SELECT action FROM audit_events WHERE id = (SELECT id FROM a1)) = 'upload.created', 'audited');

-- A rejected job keeps its result and is never retried.
INSERT INTO jobs (id, workspace_id, kind, status, attempts, started_at, worker_id, heartbeat_at)
VALUES ('00000000-0000-7000-8000-00000000e003', :'ws', 'ingest', 'running', 1, now(), 'w1', now());
CREATE TEMP TABLE j1 AS EXECUTE job_reject('00000000-0000-7000-8000-00000000e003', 'w1', 'bad rows', '{"quality_report":{}}');
SELECT pg_temp.check((SELECT status FROM j1) = 'failed' AND (SELECT result ? 'quality_report' FROM j1), 'rejected with result');
CREATE TEMP TABLE j2 AS EXECUTE job_reject('00000000-0000-7000-8000-00000000e003', 'w1', 'again', '{}');
SELECT pg_temp.check((SELECT count(*) FROM j2) = 0, 'only the owning worker, only while running');
SELECT pg_temp.check((SELECT kind FROM job_events WHERE job_id = '00000000-0000-7000-8000-00000000e003') = 'failed', 'event');

\echo upload checks passed
ROLLBACK;
