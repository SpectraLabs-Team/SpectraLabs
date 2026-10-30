-- Behaviour of the versioned-report SQL in queries/report_*.sql (M16 tasks 2 and 4).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/report_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

INSERT INTO users (id, email, password_hash) VALUES ('00000000-0000-7000-8000-0000000016a1', 'rep@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000016c1', 'repc', 'C'),
    ('00000000-0000-7000-8000-0000000016d1', 'repd', 'D');

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'report check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000016c1'
\set other '00000000-0000-7000-8000-0000000016d1'
\set user '00000000-0000-7000-8000-0000000016a1'
\set sweep '00000000-0000-7000-8000-0000000016e1'
\set sweep2 '00000000-0000-7000-8000-0000000016e2'

-- First export of a sweep: version 1 of a new report, with the content's hash and size.
EXECUTE report_lock(:'ws', 'validation', 'sweep', :'sweep');
CREATE TEMP TABLE v1 AS EXECUTE report_version_insert(:'ws', 'validation', 'sweep', :'sweep', 'pdf', 'application/pdf', '\x255044462d'::bytea, :'user');
SELECT report_id AS rid FROM v1 \gset
SELECT pg_temp.check((SELECT version FROM v1) = 1, 'first version is 1');
SELECT pg_temp.check((SELECT sha256 FROM v1) = encode(sha256('\x255044462d'::bytea), 'hex'), 'hash of the content');
SELECT pg_temp.check((SELECT size_bytes FROM v1) = 5 AND (SELECT created_by FROM v1) = :'user', 'size and author');

-- Exporting the same run again adds a version under the same report id, in either format.
EXECUTE report_lock(:'ws', 'validation', 'sweep', :'sweep');
CREATE TEMP TABLE v2 AS EXECUTE report_version_insert(:'ws', 'validation', 'sweep', :'sweep', 'csv', 'text/csv; charset=utf-8', 'a,b'::bytea, NULL);
EXECUTE report_lock(:'ws', 'validation', 'sweep', :'sweep');
CREATE TEMP TABLE v3 AS EXECUTE report_version_insert(:'ws', 'validation', 'sweep', :'sweep', 'pdf', 'application/pdf', '\x255044462d'::bytea, NULL);
SELECT pg_temp.check((SELECT version FROM v2) = 2 AND (SELECT version FROM v3) = 3, 'versions count up');
SELECT pg_temp.check((SELECT report_id FROM v2) = :'rid' AND (SELECT report_id FROM v3) = :'rid', 'one report id');
SELECT pg_temp.check((SELECT count(*) FROM reports WHERE report_id = :'rid') = 3, 'nothing overwritten');

-- Another run, another type, or another workspace starts its own numbering.
CREATE TEMP TABLE o1 AS EXECUTE report_version_insert(:'ws', 'validation', 'sweep', :'sweep2', 'pdf', 'application/pdf', 'x'::bytea, NULL);
CREATE TEMP TABLE o2 AS EXECUTE report_version_insert(:'ws', 'tear_sheet', 'sweep', :'sweep', 'pdf', 'application/pdf', 'x'::bytea, NULL);
CREATE TEMP TABLE o3 AS EXECUTE report_version_insert(:'other', 'validation', 'sweep', :'sweep', 'pdf', 'application/pdf', 'x'::bytea, NULL);
SELECT pg_temp.check((SELECT version FROM o1) = 1 AND (SELECT version FROM o2) = 1 AND (SELECT version FROM o3) = 1,
                     'separate numbering');
SELECT pg_temp.check((SELECT report_id FROM o1) <> :'rid' AND (SELECT report_id FROM o3) <> :'rid', 'separate report ids');

-- Fetch one version by report id and version, with its bytes; only in its workspace.
CREATE TEMP TABLE g1 AS EXECUTE report_version_get(:'ws', :'rid', 2);
CREATE TEMP TABLE g2 AS EXECUTE report_version_get(:'other', :'rid', 2);
CREATE TEMP TABLE g3 AS EXECUTE report_version_get(:'ws', :'rid', 9);
SELECT pg_temp.check((SELECT content FROM g1) = 'a,b'::bytea AND (SELECT format FROM g1) = 'csv', 'get a version');
SELECT pg_temp.check((SELECT count(*) FROM g2) = 0, 'get is workspace scoped');
SELECT pg_temp.check((SELECT count(*) FROM g3) = 0, 'missing version');

-- Version list, newest first; reports of a run, newest first within each type.
CREATE TEMP TABLE l1 AS EXECUTE report_versions_of(:'ws', :'rid');
SELECT pg_temp.check((SELECT array_agg(version) FROM l1) = '{3,2,1}', 'versions newest first');
CREATE TEMP TABLE r1 AS EXECUTE reports_for_run(:'ws', 'sweep', :'sweep');
CREATE TEMP TABLE r2 AS EXECUTE reports_for_run(:'other', 'sweep', :'sweep');
SELECT pg_temp.check((SELECT count(*) FROM r1) = 4 AND (SELECT count(*) FROM r2) = 1, 'reports of a run, per workspace');
SELECT pg_temp.check((SELECT array_agg(report_type || ':' || version) FROM r1) = '{tear_sheet:1,validation:3,validation:2,validation:1}',
                     'grouped by type, newest first');

-- Versions are immutable.
DO $$
BEGIN
    UPDATE reports SET content_type = 'text/plain' WHERE version = 1;
    RAISE EXCEPTION 'report check failed: update allowed';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'report versions are immutable' THEN RAISE; END IF;
END $$;

-- The table refuses a hash or size that does not match the content, and a second version 1.
DO $$
BEGIN
    INSERT INTO reports (workspace_id, report_id, report_type, run_kind, run_id, version, format, content_type,
                         sha256, size_bytes, content)
    VALUES ('00000000-0000-7000-8000-0000000016c1', spectra_uuidv7(), 'gap', 'paper_gap', spectra_uuidv7(), 1,
            'pdf', 'application/pdf', repeat('0', 64), 1, 'x'::bytea);
    RAISE EXCEPTION 'report check failed: wrong hash accepted';
EXCEPTION WHEN check_violation THEN NULL;
END $$;
DO $$
BEGIN
    INSERT INTO reports (workspace_id, report_id, report_type, run_kind, run_id, version, format, content_type,
                         sha256, size_bytes, content)
    VALUES ('00000000-0000-7000-8000-0000000016c1', spectra_uuidv7(), 'validation', 'sweep',
            '00000000-0000-7000-8000-0000000016e1', 1, 'pdf', 'application/pdf',
            encode(sha256('y'::bytea), 'hex'), 1, 'y'::bytea);
    RAISE EXCEPTION 'report check failed: second version 1 accepted';
EXCEPTION WHEN unique_violation THEN NULL;
END $$;

-- Unknown type, kind or format.
DO $$
BEGIN
    INSERT INTO reports (workspace_id, report_id, report_type, run_kind, run_id, version, format, content_type,
                         sha256, size_bytes, content)
    VALUES ('00000000-0000-7000-8000-0000000016c1', spectra_uuidv7(), 'memo', 'sweep', spectra_uuidv7(), 1,
            'pdf', 'application/pdf', encode(sha256('z'::bytea), 'hex'), 1, 'z'::bytea);
    RAISE EXCEPTION 'report check failed: unknown type accepted';
EXCEPTION WHEN check_violation THEN NULL;
END $$;

-- Deleting a workspace removes its reports.
DELETE FROM workspaces WHERE id = :'other';
SELECT pg_temp.check((SELECT count(*) FROM reports WHERE workspace_id = :'other') = 0, 'workspace delete cascades');

ROLLBACK;
