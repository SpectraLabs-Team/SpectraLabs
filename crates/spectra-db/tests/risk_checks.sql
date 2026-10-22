-- Behaviour of the Risk Center SQL in queries/risk_*.sql (Module 8).
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

-- 3. Pre-trade decision record & list
CREATE TEMP TABLE d1 AS EXECUTE risk_decision_insert(:'ws', NULL, 'AAPL', 'buy', 100.0, 150.0, 100.0, 'approved', '[]'::jsonb, ARRAY[]::text[], '{"gross_exposure":15000.0}'::jsonb, :'user');
SELECT pg_temp.check((SELECT verdict FROM d1) = 'approved', 'decision recorded');

CREATE TEMP TABLE d2 AS EXECUTE risk_decisions_list(:'ws', 10);
SELECT pg_temp.check((SELECT count(*) FROM d2) = 1 AND (SELECT symbol FROM d2) = 'AAPL', 'list decisions');

\echo 'risk checks passed'
ROLLBACK;
