-- Behaviour of the resumable-upload SQL in queries/upload_session_*.sql (M1b task 9).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/session_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'session check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000005a1'
\set other '00000000-0000-7000-8000-0000000005a2'
\set user '00000000-0000-7000-8000-0000000005a9'
INSERT INTO users (id, email, password_hash) VALUES (:'user', 'sess@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES (:'ws', 'sess', 'Sessions'), (:'other', 'sess-other', 'Other');

-- Open: at most 2 here (the limit is a parameter).
CREATE TEMP TABLE s1 AS EXECUTE upload_session_create(:'ws', :'user', 'big.csv', 100, 3600, 2);
CREATE TEMP TABLE s2 AS EXECUTE upload_session_create(:'ws', NULL, repeat('x', 300), 50, 3600, 2);
CREATE TEMP TABLE s3 AS EXECUTE upload_session_create(:'ws', NULL, 'third.csv', 10, 3600, 2);
SELECT id AS s1 FROM s1 \gset
SELECT id AS s2 FROM s2 \gset
SELECT pg_temp.check((SELECT received_bytes FROM s1) = 0 AND (SELECT size_bytes FROM s1) = 100, 'opened empty');
SELECT pg_temp.check((SELECT length(filename) FROM s2) = 255, 'name cut');
SELECT pg_temp.check((SELECT count(*) FROM s3) = 0, 'no more than 2 open sessions');
CREATE TEMP TABLE s4 AS EXECUTE upload_session_create(:'other', NULL, 'x.csv', 10, 3600, 2);
SELECT pg_temp.check((SELECT count(*) FROM s4) = 1, 'the limit is per workspace');

-- Chunks advance only from exactly where the session is, and never past its size.
CREATE TEMP TABLE a1 AS EXECUTE upload_session_advance(:'ws', :'s1', 0, 40, 3600);
CREATE TEMP TABLE a2 AS EXECUTE upload_session_advance(:'ws', :'s1', 0, 40, 3600);
CREATE TEMP TABLE a3 AS EXECUTE upload_session_advance(:'ws', :'s1', 40, 61, 3600);
CREATE TEMP TABLE a4 AS EXECUTE upload_session_advance(:'other', :'s1', 40, 10, 3600);
CREATE TEMP TABLE a5 AS EXECUTE upload_session_advance(:'ws', :'s1', 40, 60, 3600);
SELECT pg_temp.check((SELECT received_bytes FROM a1) = 40, 'first chunk');
SELECT pg_temp.check((SELECT count(*) FROM a2) = 0, 'a repeated offset is refused');
SELECT pg_temp.check((SELECT count(*) FROM a3) = 0, 'past the size is refused');
SELECT pg_temp.check((SELECT count(*) FROM a4) = 0, 'another workspace is refused');
SELECT pg_temp.check((SELECT received_bytes FROM a5) = 100, 'complete');

-- Get and lock: only the owning workspace, and not once expired.
CREATE TEMP TABLE g1 AS EXECUTE upload_session_get(:'ws', :'s1');
CREATE TEMP TABLE g2 AS EXECUTE upload_session_get(:'other', :'s1');
CREATE TEMP TABLE l1 AS EXECUTE upload_session_lock(:'ws', :'s1');
SELECT pg_temp.check((SELECT received_bytes FROM g1) = 100 AND (SELECT count(*) FROM g2) = 0, 'get, scoped');
SELECT pg_temp.check((SELECT count(*) FROM l1) = 1, 'lock');

-- Finish: only when fully received; it then no longer counts as open.
INSERT INTO uploads (id, workspace_id, sha256, size_bytes, expires_at)
VALUES ('00000000-0000-7000-8000-0000000005b1', :'ws', repeat('a', 64), 100, now() + interval '1 day');
EXECUTE upload_session_finish(:'ws', :'s2', '00000000-0000-7000-8000-0000000005b1');
SELECT pg_temp.check((SELECT upload_id FROM upload_sessions WHERE id = :'s2') IS NULL, 'not finished before the last byte');
EXECUTE upload_session_finish(:'ws', :'s1', '00000000-0000-7000-8000-0000000005b1');
SELECT pg_temp.check((SELECT upload_id FROM upload_sessions WHERE id = :'s1') = '00000000-0000-7000-8000-0000000005b1', 'finished');
CREATE TEMP TABLE a6 AS EXECUTE upload_session_advance(:'ws', :'s1', 100, 0, 3600);
SELECT pg_temp.check((SELECT count(*) FROM a6) = 0, 'no chunks after finishing');
CREATE TEMP TABLE s5 AS EXECUTE upload_session_create(:'ws', NULL, 'next.csv', 10, 3600, 2);
SELECT pg_temp.check((SELECT count(*) FROM s5) = 1, 'a finished session frees its slot');

-- Delete and expire.
CREATE TEMP TABLE d1 AS EXECUTE upload_session_delete(:'other', :'s2');
CREATE TEMP TABLE d2 AS EXECUTE upload_session_delete(:'ws', :'s2');
SELECT pg_temp.check((SELECT count(*) FROM d1) = 0 AND (SELECT id FROM d2) = :'s2', 'delete, scoped');
UPDATE upload_sessions SET expires_at = now() - interval '1 second' WHERE workspace_id IN (:'ws', :'other');
CREATE TEMP TABLE g3 AS EXECUTE upload_session_get(:'ws', :'s1');
SELECT pg_temp.check((SELECT count(*) FROM g3) = 0, 'expired sessions are gone for get');
CREATE TEMP TABLE e1 AS EXECUTE upload_session_expire(100);
SELECT pg_temp.check((SELECT count(*) FROM e1) >= 3, 'expired sessions removed');
SELECT pg_temp.check(NOT EXISTS (SELECT 1 FROM upload_sessions WHERE workspace_id IN (:'ws', :'other')), 'none left');

\echo session checks passed
ROLLBACK;
