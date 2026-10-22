-- Behaviour of the Risk Center SQL in queries/risk_*.sql, queries/portfolio_*.sql, queries/equity_*.sql (Module 8).
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/risk_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

INSERT INTO users (id, email, password_hash) VALUES ('00000000-0000-7000-8000-0000000003a1', 'risk@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000003c1', 'risk-ws', 'Risk Workspace');

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'risk check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000003c1'
\set user '00000000-0000-7000-8000-0000000003a1'

-- 1. Upsert & get limits
CREATE TEMP TABLE l1 AS EXECUTE risk_limits_upsert(:'ws', NULL, 800000.0, 400000.0, 80000.0, 0.20, 2.5, 0.12, 20000.0, 0.04, true, :'user');
SELECT pg_temp.check((SELECT max_gross_exposure FROM l1) = 800000.0, 'upsert limits');

CREATE TEMP TABLE l2 AS EXECUTE risk_limits_get(:'ws', NULL);
SELECT pg_temp.check((SELECT max_position_size FROM l2) = 80000.0, 'get limits');

-- 2. Kill switch set, get, update, and clear
CREATE TEMP TABLE k1 AS EXECUTE risk_kill_switch_set(:'ws', NULL, true, 'Emergency test halt', :'user');
SELECT pg_temp.check((SELECT active FROM k1) = true, 'kill switch activated');

CREATE TEMP TABLE k2 AS EXECUTE risk_kill_switch_get(:'ws', NULL);
SELECT pg_temp.check((SELECT active FROM k2) = true AND (SELECT reason FROM k2) = 'Emergency test halt', 'kill switch get');

CREATE TEMP TABLE k3 AS EXECUTE risk_kill_switch_set(:'ws', NULL, false, 'Cleared test halt', :'user');
SELECT pg_temp.check((SELECT active FROM k3) = false, 'kill switch cleared');

CREATE TEMP TABLE k4 AS EXECUTE risk_kill_switch_get(:'ws', NULL);
SELECT pg_temp.check((SELECT active FROM k4) = false, 'kill switch get after clear');

-- 3. Pre-trade decision record, get, and list
CREATE TEMP TABLE d1 AS EXECUTE risk_decision_insert(:'ws', NULL, 'AAPL', 'buy', 100.0, 150.0, 100.0, 'approved', '[]'::jsonb, ARRAY[]::text[], '{"gross_exposure":15000.0}'::jsonb, :'user');
SELECT id AS dec_id FROM d1 \gset
SELECT pg_temp.check((SELECT verdict FROM d1) = 'approved', 'decision recorded');

CREATE TEMP TABLE d_get AS EXECUTE risk_decision_get(:'ws', :'dec_id');
SELECT pg_temp.check((SELECT symbol FROM d_get) = 'AAPL' AND (SELECT requested_qty FROM d_get) = 100.0, 'decision get by id');

CREATE TEMP TABLE d2 AS EXECUTE risk_decisions_list(:'ws', 10);
SELECT pg_temp.check((SELECT count(*) FROM d2) = 1 AND (SELECT symbol FROM d2) = 'AAPL', 'list decisions');

-- 4. Server-built portfolio positions & balances (Task 1 & Task 2)
CREATE TEMP TABLE pos1 AS EXECUTE portfolio_position_upsert(:'ws', NULL, 'AAPL', 50.0, 150.0);
SELECT pg_temp.check((SELECT qty FROM pos1) = 50.0, 'upsert portfolio position');

CREATE TEMP TABLE pos_list AS EXECUTE portfolio_positions_get(:'ws', NULL);
SELECT pg_temp.check((SELECT count(*) FROM pos_list) = 1 AND (SELECT symbol FROM pos_list) = 'AAPL', 'get portfolio positions');

CREATE TEMP TABLE bal1 AS EXECUTE portfolio_balance_upsert(:'ws', NULL, 50000.0, 57500.0, 60000.0, 58000.0);
SELECT pg_temp.check((SELECT cash FROM bal1) = 50000.0, 'upsert portfolio balance');

CREATE TEMP TABLE bal_get AS EXECUTE portfolio_balance_get(:'ws', NULL);
SELECT pg_temp.check((SELECT equity FROM bal_get) = 57500.0 AND (SELECT daily_start_equity FROM bal_get) = 58000.0, 'get portfolio balance');

-- 5. Equity history snapshots (Task 2 & Task 10)
CREATE TEMP TABLE snap1 AS EXECUTE equity_snapshot_insert(:'ws', NULL, NULL, 1, 1000000000, 57500.0, 50000.0);
SELECT pg_temp.check((SELECT equity FROM snap1) = 57500.0, 'record equity snapshot');

CREATE TEMP TABLE snaps_list AS EXECUTE equity_snapshots_list(:'ws', NULL, 10);
SELECT pg_temp.check((SELECT count(*) FROM snaps_list) = 1, 'list equity snapshots');

\echo 'risk checks passed'
ROLLBACK;
