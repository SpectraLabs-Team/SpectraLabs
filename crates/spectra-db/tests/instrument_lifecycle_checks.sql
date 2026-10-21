-- Behaviour of instrument lifecycles and survivorship-free universes SQL (M1b task 12).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/instrument_lifecycle_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'lifecycle check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000007a1'
\set other '00000000-0000-7000-8000-0000000007a2'
\set user '00000000-0000-7000-8000-0000000007a9'
INSERT INTO users (id, email, password_hash) VALUES (:'user', 'lifecycle@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES (:'ws', 'life-ws', 'Lifecycles'), (:'other', 'life-other', 'Other');

-- 1. Symbol changes
CREATE TEMP TABLE sc1 AS EXECUTE symbol_change_create(:'ws', 'FB', 'META', '2022-06-09 00:00:00+00', 'Rebranding to Meta');
SELECT id AS sc_id FROM sc1 \gset
SELECT pg_temp.check((SELECT old_symbol FROM sc1) = 'FB' AND (SELECT new_symbol FROM sc1) = 'META', 'symbol change recorded');

CREATE TEMP TABLE sc_list AS EXECUTE symbol_changes_list(:'ws');
SELECT pg_temp.check((SELECT count(*) FROM sc_list) = 1, 'listed symbol change');

-- Workspace isolation for symbol changes
CREATE TEMP TABLE sc_other AS EXECUTE symbol_changes_list(:'other');
SELECT pg_temp.check((SELECT count(*) FROM sc_other) = 0, 'workspace isolation on symbol changes');

-- 2. Instrument delistings
CREATE TEMP TABLE del1 AS EXECUTE delisting_create(:'ws', 'ENRON', '2001-11-28 00:00:00+00', 'Bankruptcy');
SELECT id AS d_id FROM del1 \gset
SELECT pg_temp.check((SELECT symbol FROM del1) = 'ENRON', 'delisting recorded');

CREATE TEMP TABLE del_list AS EXECUTE delistings_list(:'ws');
SELECT pg_temp.check((SELECT count(*) FROM del_list) = 1, 'listed delisting');

CREATE TEMP TABLE del_other AS EXECUTE delistings_list(:'other');
SELECT pg_temp.check((SELECT count(*) FROM del_other) = 0, 'workspace isolation on delistings');

-- 3. Universes and Constituents
CREATE TEMP TABLE u1 AS EXECUTE universe_create(:'ws', 'US LargeCap Tech', 'Top tech universe');
SELECT id AS u_id FROM u1 \gset
SELECT pg_temp.check((SELECT name FROM u1) = 'US LargeCap Tech', 'universe created');

-- Add members: FB (which became META in 2022), ENRON (which delisted on 2001-11-28), and AAPL
EXECUTE universe_member_add(:'ws', :'u_id', 'FB', '1990-01-01 00:00:00+00');
EXECUTE universe_member_add(:'ws', :'u_id', 'ENRON', '1990-01-01 00:00:00+00');
EXECUTE universe_member_add(:'ws', :'u_id', 'AAPL', '1990-01-01 00:00:00+00');

CREATE TEMP TABLE u_list AS EXECUTE universe_list(:'ws');
SELECT pg_temp.check((SELECT member_count FROM u_list WHERE id = :'u_id') = 3, 'universe has 3 members');

-- 4. Point-in-time universe resolution (SURVIVORSHIP BIAS TEST):
-- As of 2000-01-01 (prior to Enron delisting and prior to FB->META change):
-- ENRON must be active (not yet delisted!) -> survivorship-free!
-- FB must be resolved as 'FB' (not META yet).
CREATE TEMP TABLE pit_2000 AS EXECUTE universe_resolve_as_of(:'ws', :'u_id', '2000-01-01 00:00:00+00');
SELECT pg_temp.check((SELECT is_active FROM pit_2000 WHERE symbol = 'ENRON') = true, 'ENRON was active in 2000 (no survivorship bias)');
SELECT pg_temp.check((SELECT resolved_symbol FROM pit_2000 WHERE symbol = 'FB') = 'FB', 'FB was FB in 2000');
SELECT pg_temp.check((SELECT is_active FROM pit_2000 WHERE symbol = 'AAPL') = true, 'AAPL active in 2000');

-- As of 2010-01-01 (after Enron delisting, before FB->META change):
-- ENRON must be NOT active (delisted).
-- FB must still be resolved as 'FB'.
CREATE TEMP TABLE pit_2010 AS EXECUTE universe_resolve_as_of(:'ws', :'u_id', '2010-01-01 00:00:00+00');
SELECT pg_temp.check((SELECT is_active FROM pit_2010 WHERE symbol = 'ENRON') = false, 'ENRON is inactive in 2010');
SELECT pg_temp.check((SELECT resolved_symbol FROM pit_2010 WHERE symbol = 'FB') = 'FB', 'FB was FB in 2010');

-- As of 2023-01-01 (after FB->META change):
-- FB must be resolved as 'META'.
CREATE TEMP TABLE pit_2023 AS EXECUTE universe_resolve_as_of(:'ws', :'u_id', '2023-01-01 00:00:00+00');
SELECT pg_temp.check((SELECT resolved_symbol FROM pit_2023 WHERE symbol = 'FB') = 'META', 'FB resolved to META in 2023');

-- 5. Deletion checks
EXECUTE symbol_change_delete(:'ws', :'sc_id');
SELECT pg_temp.check((SELECT count(*) FROM symbol_changes WHERE id = :'sc_id') = 0, 'symbol change deleted');

EXECUTE delisting_delete(:'ws', :'d_id');
SELECT pg_temp.check((SELECT count(*) FROM instrument_delistings WHERE id = :'d_id') = 0, 'delisting deleted');

EXECUTE universe_delete(:'ws', :'u_id');
SELECT pg_temp.check((SELECT count(*) FROM universes WHERE id = :'u_id') = 0, 'universe deleted');
SELECT pg_temp.check((SELECT count(*) FROM universe_members WHERE universe_id = :'u_id') = 0, 'members cascade deleted');

ROLLBACK;
