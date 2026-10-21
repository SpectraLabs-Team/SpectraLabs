-- Behaviour of queries/identity_mirror.sql (M1 task 1): signed-in M19 users and workspaces
-- are mirrored into the engine's tables with the same ids, once.
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/identity_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'identity check failed: %', what; END IF; END $$;

\set u1 '6f1c2d3e-0000-4000-8000-0000000000a1'
\set u2 '6f1c2d3e-0000-4000-8000-0000000000a2'
\set w1 '7a2b3c4d-0000-4000-8000-0000000000b1'
\set w2 '7a2b3c4d-0000-4000-8000-0000000000b2'
\set w3 '7a2b3c4d-0000-4000-8000-0000000000b3'

-- Before mirroring, a job for a fresh M19 user fails the foreign keys (the M1 blocker).
DO $$
BEGIN
    INSERT INTO jobs (workspace_id, kind, created_by)
    VALUES ('7a2b3c4d-0000-4000-8000-0000000000b1', 'backtest', '6f1c2d3e-0000-4000-8000-0000000000a1');
    RAISE EXCEPTION 'identity check failed: job inserted for an unknown workspace';
EXCEPTION WHEN foreign_key_violation THEN NULL;
END $$;

-- A fresh user and workspace: both rows appear, with placeholders and the user as creator.
EXECUTE identity_mirror(:'u1', :'w1');
SELECT pg_temp.check((SELECT email FROM users WHERE id = :'u1') = 'u-6f1c2d3e0000400080000000000000a1@mirror.invalid',
                     'placeholder email');
SELECT pg_temp.check((SELECT password_hash FROM users WHERE id = :'u1') = '!', 'no usable password');
SELECT pg_temp.check((SELECT platform_role FROM users WHERE id = :'u1') = 'user', 'never an admin');
SELECT pg_temp.check((SELECT slug FROM workspaces WHERE id = :'w1') = 'ws-7a2b3c4d0000400080000000000000b1', 'slug');
SELECT pg_temp.check((SELECT created_by FROM workspaces WHERE id = :'w1') = :'u1', 'created_by is the mirrored user');

-- Now the same job inserts.
INSERT INTO jobs (workspace_id, kind, created_by) VALUES (:'w1', 'backtest', :'u1');

-- Running it again changes nothing, and rows edited since are left alone.
UPDATE workspaces SET name = 'Renamed' WHERE id = :'w1';
EXECUTE identity_mirror(:'u1', :'w1');
SELECT pg_temp.check((SELECT count(*) FROM users WHERE id = :'u1') = 1, 'one user row');
SELECT pg_temp.check((SELECT name FROM workspaces WHERE id = :'w1') = 'Renamed', 'existing workspace untouched');

-- An existing user joining another (team) workspace: the workspace is created by them.
EXECUTE identity_mirror(:'u1', :'w2');
SELECT pg_temp.check((SELECT created_by FROM workspaces WHERE id = :'w2') = :'u1', 'existing user as creator');

-- Another user in an already-mirrored workspace: the user is added, the workspace untouched.
EXECUTE identity_mirror(:'u2', :'w2');
SELECT pg_temp.check(EXISTS (SELECT 1 FROM users WHERE id = :'u2'), 'second user mirrored');
SELECT pg_temp.check((SELECT created_by FROM workspaces WHERE id = :'w2') = :'u1', 'creator unchanged');

-- No user (an API token without one): only the workspace, with no creator.
EXECUTE identity_mirror(NULL, :'w3');
SELECT pg_temp.check((SELECT created_by FROM workspaces WHERE id = :'w3') IS NULL, 'no creator');

\echo identity checks passed
ROLLBACK;
