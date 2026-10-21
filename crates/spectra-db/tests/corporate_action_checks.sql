-- Behaviour of corporate action and derived dataset SQL (M1b task 11).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/corporate_action_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'corporate action check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000006a1'
\set other '00000000-0000-7000-8000-0000000006a2'
\set user '00000000-0000-7000-8000-0000000006a9'
INSERT INTO users (id, email, password_hash) VALUES (:'user', 'corp@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES (:'ws', 'corp-ws', 'Corporate Actions'), (:'other', 'corp-other', 'Other');

-- Create a corporate action set
CREATE TEMP TABLE set1 AS EXECUTE corporate_action_set_create(:'ws', 'US Splits 2024', 'Major tech splits', :'user');
SELECT id AS s1 FROM set1 \gset
SELECT pg_temp.check((SELECT name FROM set1) = 'US Splits 2024', 'set created with name');

-- Insert corporate actions into the set
CREATE TEMP TABLE a1 AS EXECUTE corporate_action_insert(:'ws', :'s1', 'AAPL', '2020-08-31 00:00:00+00', 'split', 1, 4, '0');
CREATE TEMP TABLE a2 AS EXECUTE corporate_action_insert(:'ws', :'s1', 'AAPL', '2021-02-05 00:00:00+00', 'dividend', 1, 1, '0.205');
SELECT pg_temp.check((SELECT count(*) FROM a1) = 1 AND (SELECT count(*) FROM a2) = 1, 'actions inserted');

-- List sets in workspace: shows action count
CREATE TEMP TABLE sets_list AS EXECUTE corporate_action_set_list(:'ws');
SELECT pg_temp.check((SELECT action_count FROM sets_list WHERE id = :'s1') = 2, 'action count matches');

-- Workspace scoping: another workspace sees nothing
CREATE TEMP TABLE other_list AS EXECUTE corporate_action_set_list(:'other');
SELECT pg_temp.check((SELECT count(*) FROM other_list) = 0, 'other workspace sees no sets');

-- Get set detail
CREATE TEMP TABLE set_get AS EXECUTE corporate_action_set_get(:'ws', :'s1');
SELECT pg_temp.check((SELECT count(*) FROM set_get) = 1, 'get set by id');
CREATE TEMP TABLE other_get AS EXECUTE corporate_action_set_get(:'other', :'s1');
SELECT pg_temp.check((SELECT count(*) FROM other_get) = 0, 'cross-workspace get refused');

-- List actions by set
CREATE TEMP TABLE acts AS EXECUTE corporate_actions_list_by_set(:'ws', :'s1');
SELECT pg_temp.check((SELECT count(*) FROM acts) = 2, 'listed 2 actions');
SELECT pg_temp.check((SELECT symbol FROM acts LIMIT 1) = 'AAPL', 'first action symbol');

-- Register a derived dataset version with lineage
\set base_chk 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
\set derived_chk 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'

-- First register a base version
CREATE TEMP TABLE v1 AS EXECUTE dataset_version_register(:'ws', 'Tech Stocks', :'base_chk', 1000, 50000, '{"symbols":["AAPL"]}'::jsonb, :'user', repeat('c', 64), '{"filename":"raw.csv"}'::jsonb);
SELECT id AS vid1 FROM v1 \gset

-- Now register a derived version from v1
CREATE TEMP TABLE v_derived AS EXECUTE dataset_version_derive(
    :'ws',
    'Tech Stocks Adjusted',
    :'derived_chk',
    1000,
    50000,
    '{"symbols":["AAPL"]}'::jsonb,
    :'user',
    repeat('d', 64),
    '{"type":"derived"}'::jsonb,
    :'vid1',
    'adjusted',
    jsonb_build_object('set_id', :'s1')
);
SELECT id AS vid_derived FROM v_derived \gset
SELECT pg_temp.check((SELECT version FROM v_derived) = 1, 'derived version created');

-- Check dataset_version_get returns lineage
CREATE TEMP TABLE v_get AS EXECUTE dataset_version_get(:'ws', :'vid_derived');
SELECT pg_temp.check((SELECT derived_from_id FROM v_get) = :'vid1', 'derived_from_id linked');
SELECT pg_temp.check((SELECT derivation_kind FROM v_get) = 'adjusted', 'derivation_kind recorded');

-- Delete action set cascades and cleans up actions
EXECUTE corporate_action_set_delete(:'ws', :'s1');
SELECT pg_temp.check((SELECT count(*) FROM corporate_action_sets WHERE id = :'s1') = 0, 'set deleted');
SELECT pg_temp.check((SELECT count(*) FROM corporate_actions WHERE set_id = :'s1') = 0, 'actions cascaded');

ROLLBACK;
