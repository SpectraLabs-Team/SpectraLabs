-- Behaviour of data provider configs and refresh schedules SQL (M1b task 13).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/provider_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'provider check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000008a1'
\set other '00000000-0000-7000-8000-0000000008a2'
\set user '00000000-0000-7000-8000-0000000008a9'
INSERT INTO users (id, email, password_hash) VALUES (:'user', 'provider@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES (:'ws', 'prov-ws', 'Providers'), (:'other', 'prov-other', 'Other');

-- 1. Create provider configuration
CREATE TEMP TABLE p1 AS EXECUTE provider_config_upsert(
    :'ws', 'Mock Market Data', 'fake', NULL, '{"feed": "simulated"}'::jsonb, :'user'
);
SELECT id AS p_id FROM p1 \gset
SELECT pg_temp.check((SELECT name FROM p1) = 'Mock Market Data', 'provider config created');
SELECT pg_temp.check((SELECT provider FROM p1) = 'fake', 'provider is fake');

-- Update config in-place
CREATE TEMP TABLE p1_up AS EXECUTE provider_config_upsert(
    :'ws', 'Mock Market Data', 'fake', NULL, '{"feed": "simulated_v2"}'::jsonb, :'user'
);
SELECT pg_temp.check((SELECT id FROM p1_up) = :'p_id', 'id preserved on upsert');
SELECT pg_temp.check((SELECT (settings->>'feed')::text FROM p1_up) = 'simulated_v2', 'settings updated');

-- Workspace isolation for configs
CREATE TEMP TABLE p_list AS EXECUTE provider_configs_list(:'ws');
SELECT pg_temp.check((SELECT count(*) FROM p_list) = 1, 'listed 1 provider in ws');

CREATE TEMP TABLE p_other AS EXECUTE provider_configs_list(:'other');
SELECT pg_temp.check((SELECT count(*) FROM p_other) = 0, 'workspace isolation on provider configs');

-- 2. Create a dataset and refresh schedule
\set ds_id '00000000-0000-7000-8000-0000000008d1'
INSERT INTO datasets (id, workspace_id, name) VALUES (:'ds_id', :'ws', 'sp500_daily');

CREATE TEMP TABLE s1 AS EXECUTE refresh_schedule_upsert(
    :'ws', :'ds_id', :'p_id', ARRAY['AAPL', 'MSFT'], 86400, 5, true, :'user'
);
SELECT id AS sched_id FROM s1 \gset
SELECT pg_temp.check((SELECT interval_seconds FROM s1) = 86400, 'interval seconds recorded');
SELECT pg_temp.check((SELECT lookback_bars FROM s1) = 5, 'lookback bars recorded');
SELECT pg_temp.check((SELECT symbols FROM s1) = ARRAY['AAPL', 'MSFT'], 'symbols recorded');

-- Get schedule
CREATE TEMP TABLE s_get AS EXECUTE refresh_schedule_get(:'ws', :'ds_id');
SELECT pg_temp.check((SELECT id FROM s_get) = :'sched_id', 'retrieved schedule by dataset');

CREATE TEMP TABLE s_get_id AS EXECUTE refresh_schedule_get_by_id(:'ws', :'sched_id');
SELECT pg_temp.check((SELECT id FROM s_get_id) = :'sched_id', 'retrieved schedule by id');

-- 3. Due schedules query
CREATE TEMP TABLE due_now AS EXECUTE refresh_schedules_due(now() + interval '1 minute', 10);
SELECT pg_temp.check((SELECT count(*) FROM due_now WHERE id = :'sched_id') = 1, 'schedule is due for refresh');

-- Advance schedule run
EXECUTE refresh_schedule_update_run(:'sched_id', now(), now() + interval '1 day');

CREATE TEMP TABLE due_after AS EXECUTE refresh_schedules_due(now(), 10);
SELECT pg_temp.check((SELECT count(*) FROM due_after WHERE id = :'sched_id') = 0, 'schedule is no longer due after advance');

-- 4. Deletion and cascade
EXECUTE refresh_schedule_delete(:'ws', :'sched_id');
SELECT pg_temp.check((SELECT count(*) FROM dataset_refresh_schedules WHERE id = :'sched_id') = 0, 'schedule deleted');

EXECUTE provider_config_delete(:'ws', :'p_id');
SELECT pg_temp.check((SELECT count(*) FROM data_provider_configs WHERE id = :'p_id') = 0, 'provider config deleted');

-- 5. Vault queries
INSERT INTO workspace_keys (workspace_id, wrapped_key, master_version)
VALUES (:'ws', '\x01020304', 1);

CREATE TEMP TABLE wk_get AS EXECUTE vault_workspace_key_get(:'ws');
SELECT pg_temp.check((SELECT master_version FROM wk_get) = 1, 'retrieved workspace key');

\set sec_id '00000000-0000-7000-8000-0000000008c1'
INSERT INTO secrets (id, workspace_id, kind, ciphertext, hint, created_by)
VALUES (:'sec_id', :'ws', 'alpaca_paper', '\x0102030405', 'ABCD', :'user');

CREATE TEMP TABLE sec_by_id AS EXECUTE vault_secret_get(:'ws', :'sec_id', NULL);
SELECT pg_temp.check((SELECT hint FROM sec_by_id) = 'ABCD', 'retrieved secret by id');

CREATE TEMP TABLE sec_by_kind AS EXECUTE vault_secret_get(:'ws', NULL, 'alpaca_paper');
SELECT pg_temp.check((SELECT id FROM sec_by_kind) = :'sec_id', 'retrieved secret by kind');

EXECUTE vault_secret_mark_used(:'ws', :'sec_id');
SELECT pg_temp.check((SELECT last_used_at IS NOT NULL FROM secrets WHERE id = :'sec_id'), 'marked secret used');

\echo provider checks passed
ROLLBACK;
