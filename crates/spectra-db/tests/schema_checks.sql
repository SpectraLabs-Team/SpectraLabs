-- Schema checks, run with psql against a freshly migrated database:
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f crates/spectra-db/tests/schema_checks.sql
-- Everything runs in one transaction that is rolled back, so the database is left unchanged.
-- Each check raises an exception on failure.

BEGIN;

-- UUIDv7: version nibble 7, variant 10, and time-ordered.
DO $$
DECLARE a uuid; b uuid;
BEGIN
    a := spectra_uuidv7();
    PERFORM pg_sleep(0.002);
    b := spectra_uuidv7();
    IF substr(a::text, 15, 1) <> '7' THEN RAISE EXCEPTION 'uuid version: %', a; END IF;
    IF substr(a::text, 20, 1) NOT IN ('8', '9', 'a', 'b') THEN RAISE EXCEPTION 'uuid variant: %', a; END IF;
    IF NOT a < b THEN RAISE EXCEPTION 'uuidv7 not time-ordered: % then %', a, b; END IF;
    -- The first 48 bits are the Unix time in ms (within a second of now).
    IF abs(('x' || replace(substr(a::text, 1, 13), '-', ''))::bit(48)::bigint
           - floor(extract(epoch FROM clock_timestamp()) * 1000)::bigint) > 1000 THEN
        RAISE EXCEPTION 'uuidv7 timestamp is off: %', a;
    END IF;
END $$;

-- Fixtures: two users, two workspaces (the dashboard's tables: web/frontend/db/migrations).
INSERT INTO users (id, email, password_hash, platform_role) VALUES
    ('00000000-0000-7000-8000-000000000001', 'alice@example.com', '!', 'admin'),
    ('00000000-0000-7000-8000-000000000002', 'bob@example.com', '!', 'user');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000000a1', 'alpha', 'Alpha'),
    ('00000000-0000-7000-8000-0000000000b1', 'beta', 'Beta');

-- Emails are stored lower-case and trimmed.
DO $$ BEGIN
    INSERT INTO users (email, password_hash) VALUES ('Carol@Example.com', '!');
    RAISE EXCEPTION 'mixed-case email accepted';
EXCEPTION WHEN check_violation THEN NULL; END $$;

-- Hashes must be 64 lower-case hex characters.
DO $$ BEGIN
    INSERT INTO datasets (id, workspace_id, name) VALUES
        ('00000000-0000-7000-8000-0000000000d1', '00000000-0000-7000-8000-0000000000a1', 'prices');
    INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size)
        VALUES ('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-0000000000d1', 1,
                'ABC', 10, 100);
    RAISE EXCEPTION 'bad checksum accepted';
EXCEPTION WHEN check_violation THEN NULL; END $$;

-- A dataset version cannot point at another workspace's dataset.
INSERT INTO datasets (id, workspace_id, name) VALUES
    ('00000000-0000-7000-8000-0000000000d1', '00000000-0000-7000-8000-0000000000a1', 'prices');
DO $$ BEGIN
    INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size)
        VALUES ('00000000-0000-7000-8000-0000000000b1', '00000000-0000-7000-8000-0000000000d1', 1,
                repeat('a', 64), 10, 100);
    RAISE EXCEPTION 'cross-workspace dataset version accepted';
EXCEPTION WHEN foreign_key_violation THEN NULL; END $$;
INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size)
    VALUES ('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-0000000000d1', 1,
            repeat('a', 64), 10, 100);

-- M5: experiments carry tags and same-workspace lineage; runs freeze their results at completion.
INSERT INTO experiments (id, workspace_id, name, tags, registry_id)
    VALUES ('00000000-0000-7000-8000-00000000e001', '00000000-0000-7000-8000-0000000000a1',
            'M5 parent', ARRAY['baseline', 'daily'], 'import-exp-1');
INSERT INTO experiments (id, workspace_id, name, parent_id, registry_id)
    VALUES ('00000000-0000-7000-8000-00000000e002', '00000000-0000-7000-8000-0000000000a1',
            'M5 child', '00000000-0000-7000-8000-00000000e001', 'import-exp-2');
INSERT INTO experiment_notes (workspace_id, experiment_id, body)
    VALUES ('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-00000000e001', 'Initial note');
DO $$ BEGIN
    INSERT INTO experiments (workspace_id, name, parent_id)
        VALUES ('00000000-0000-7000-8000-0000000000b1', 'cross-workspace child',
                '00000000-0000-7000-8000-00000000e001');
    RAISE EXCEPTION 'cross-workspace experiment parent accepted';
EXCEPTION WHEN foreign_key_violation THEN NULL; END $$;

INSERT INTO runs (id, workspace_id, experiment_id, kind, status)
    VALUES ('00000000-0000-7000-8000-00000000e101', '00000000-0000-7000-8000-0000000000a1',
            '00000000-0000-7000-8000-00000000e002', 'backtest', 'queued');
UPDATE runs SET status = 'running', started_at = now()
 WHERE id = '00000000-0000-7000-8000-00000000e101';
DO $$ BEGIN
    UPDATE runs SET metrics = '{"sharpe":1.0}'::jsonb WHERE id = '00000000-0000-7000-8000-00000000e101';
    RAISE EXCEPTION 'running run results changed before completion';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'run results may only be written when the run succeeds' THEN RAISE; END IF;
END $$;
UPDATE runs SET status = 'succeeded', finished_at = now(),
    dataset_bars_sha256 = repeat('a', 64), strategy_sha256 = repeat('b', 64),
    config = '{"seed":1}'::jsonb, config_sha256 = repeat('c', 64), result_sha256 = repeat('d', 64),
    sharpe_per_period = 0.5, n_obs = 10, bar_count = 11, metrics = '{"sharpe":1.0}'::jsonb,
    registry_id = 'import-run-1'
 WHERE id = '00000000-0000-7000-8000-00000000e101';
DO $$ BEGIN
    UPDATE runs SET metrics = '{"sharpe":2.0}'::jsonb WHERE id = '00000000-0000-7000-8000-00000000e101';
    RAISE EXCEPTION 'finished run was mutable';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'finished runs are immutable' THEN RAISE; END IF;
END $$;
INSERT INTO run_notes (workspace_id, run_id, body)
    VALUES ('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-00000000e101', 'Reviewed');
DO $$ BEGIN
    INSERT INTO runs (workspace_id, experiment_id, kind, status, finished_at)
        VALUES ('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-00000000e002',
                'backtest', 'succeeded', now());
    RAISE EXCEPTION 'registry run created already finished';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'registry runs must be created in queued status' THEN RAISE; END IF;
END $$;
-- A plain job result (no experiment) is recorded once, already finished, and then frozen.
INSERT INTO runs (id, workspace_id, kind, status, finished_at)
    VALUES ('00000000-0000-7000-8000-00000000e102', '00000000-0000-7000-8000-0000000000a1',
            'benchmark', 'succeeded', now());
DO $$ BEGIN
    UPDATE runs SET metrics = '{"x":1}'::jsonb WHERE id = '00000000-0000-7000-8000-00000000e102';
    RAISE EXCEPTION 'finished job run was mutable';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'finished runs are immutable' THEN RAISE; END IF;
END $$;
DO $$ BEGIN
    UPDATE run_notes SET body = 'edited' WHERE run_id = '00000000-0000-7000-8000-00000000e101';
    RAISE EXCEPTION 'run note was mutable';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'run notes are append-only' THEN RAISE; END IF;
END $$;

-- Jobs: finished_at is set exactly when the job is in a terminal state.
DO $$ BEGIN
    INSERT INTO jobs (workspace_id, kind, status) VALUES ('00000000-0000-7000-8000-0000000000a1', 'benchmark', 'succeeded');
    RAISE EXCEPTION 'terminal job without finished_at accepted';
EXCEPTION WHEN check_violation THEN NULL; END $$;

-- The claim query (step 6) picks the highest priority, then the oldest due job, and skips locked rows.
INSERT INTO jobs (id, workspace_id, kind, priority, run_after) VALUES
    ('00000000-0000-7000-8000-00000000f001', '00000000-0000-7000-8000-0000000000a1', 'benchmark', 0, now() - interval '2 min'),
    ('00000000-0000-7000-8000-00000000f002', '00000000-0000-7000-8000-0000000000a1', 'benchmark', 5, now() - interval '1 min'),
    ('00000000-0000-7000-8000-00000000f003', '00000000-0000-7000-8000-0000000000a1', 'benchmark', 9, now() + interval '1 hour');
DO $$
DECLARE claimed uuid;
BEGIN
    SELECT id INTO claimed FROM jobs
     WHERE status = 'queued' AND run_after <= now()
     ORDER BY priority DESC, run_after, id
     LIMIT 1 FOR UPDATE SKIP LOCKED;
    IF claimed <> '00000000-0000-7000-8000-00000000f002' THEN
        RAISE EXCEPTION 'claim picked %', claimed;
    END IF;
END $$;

-- Registry chain: must start at seq 0 with the zero hash, each row must extend the last one,
-- and rows can never be updated or deleted.
DO $$ BEGIN
    INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash)
        VALUES ('00000000-0000-7000-8000-0000000000a1', 1, 'x', 0, '{}', repeat('0', 64), repeat('1', 64));
    RAISE EXCEPTION 'chain starting at seq 1 accepted';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM NOT LIKE 'registry chain must start%' THEN RAISE; END IF;
END $$;
INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash) VALUES
    ('00000000-0000-7000-8000-0000000000a1', 0, 'experiment.created', 1, '{"b":1,"a":2}', repeat('0', 64), repeat('1', 64)),
    ('00000000-0000-7000-8000-0000000000a1', 1, 'run.recorded', 2, '{}', repeat('1', 64), repeat('2', 64));
-- Each workspace has its own chain.
INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash) VALUES
    ('00000000-0000-7000-8000-0000000000b1', 0, 'experiment.created', 1, '{}', repeat('0', 64), repeat('3', 64));
DO $$ BEGIN
    INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash)
        VALUES ('00000000-0000-7000-8000-0000000000a1', 2, 'x', 3, '{}', repeat('9', 64), repeat('4', 64));
    RAISE EXCEPTION 'wrong prev_hash accepted';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM NOT LIKE 'registry chain broken%' THEN RAISE; END IF;
END $$;
DO $$ BEGIN
    UPDATE registry_events SET type = 'tampered' WHERE seq = 0;
    RAISE EXCEPTION 'update accepted';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'registry_events is append-only' THEN RAISE; END IF;
END $$;
DO $$ BEGIN
    DELETE FROM registry_events;
    RAISE EXCEPTION 'delete accepted';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'registry_events is append-only' THEN RAISE; END IF;
END $$;
-- The payload text is kept byte for byte (key order matters to the hash).
DO $$ BEGIN
    IF (SELECT payload::text FROM registry_events
         WHERE workspace_id = '00000000-0000-7000-8000-0000000000a1' AND seq = 0) <> '{"b":1,"a":2}' THEN
        RAISE EXCEPTION 'payload text was rewritten';
    END IF;
END $$;

-- Shared database: deleting a user or a workspace in the dashboard must work even when the
-- engine holds frozen rows for them, and must never remove part of a chain.
INSERT INTO users (id, email, password_hash) VALUES
    ('00000000-0000-7000-8000-000000000003', 'dave@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000000c1', 'gamma', 'Gamma');
INSERT INTO experiments (id, workspace_id, name, created_by) VALUES
    ('00000000-0000-7000-8000-00000000c001', '00000000-0000-7000-8000-0000000000c1', 'doomed', '00000000-0000-7000-8000-000000000003');
INSERT INTO runs (id, workspace_id, experiment_id, kind, status, created_by) VALUES
    ('00000000-0000-7000-8000-00000000c101', '00000000-0000-7000-8000-0000000000c1',
     '00000000-0000-7000-8000-00000000c001', 'backtest', 'queued', '00000000-0000-7000-8000-000000000003');
UPDATE runs SET status = 'running', started_at = now() WHERE id = '00000000-0000-7000-8000-00000000c101';
UPDATE runs SET status = 'succeeded', finished_at = now(), dataset_bars_sha256 = repeat('a', 64),
    config_sha256 = repeat('c', 64), result_sha256 = repeat('d', 64)
 WHERE id = '00000000-0000-7000-8000-00000000c101';
INSERT INTO run_notes (workspace_id, run_id, author_id, body) VALUES
    ('00000000-0000-7000-8000-0000000000c1', '00000000-0000-7000-8000-00000000c101',
     '00000000-0000-7000-8000-000000000003', 'by dave');
INSERT INTO experiment_notes (workspace_id, experiment_id, author_id, body) VALUES
    ('00000000-0000-7000-8000-0000000000c1', '00000000-0000-7000-8000-00000000c001',
     '00000000-0000-7000-8000-000000000003', 'also by dave');
INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash) VALUES
    ('00000000-0000-7000-8000-0000000000c1', 0, 'experiment.created', 1, '{}', repeat('0', 64), repeat('5', 64)),
    ('00000000-0000-7000-8000-0000000000c1', 1, 'run.recorded', 2, '{}', repeat('5', 64), repeat('6', 64));
INSERT INTO workspace_quotas (workspace_id, artifact_quota_bytes) VALUES ('00000000-0000-7000-8000-0000000000c1', 10);

-- Piecemeal removal is still refused while the workspace exists.
DO $$ BEGIN
    DELETE FROM registry_events WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1' AND seq = 1;
    RAISE EXCEPTION 'single registry event deleted';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'registry_events is append-only' THEN RAISE; END IF;
END $$;
DO $$ BEGIN
    DELETE FROM run_notes WHERE body = 'by dave';
    RAISE EXCEPTION 'single note deleted';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'run notes are append-only' THEN RAISE; END IF;
END $$;
DO $$ BEGIN
    UPDATE runs SET created_by = NULL, metrics = '{"x":1}'::jsonb WHERE id = '00000000-0000-7000-8000-00000000c101';
    RAISE EXCEPTION 'finished run changed alongside created_by';
EXCEPTION WHEN raise_exception THEN
    IF SQLERRM <> 'finished runs are immutable' THEN RAISE; END IF;
END $$;

-- Deleting the author's account clears authorship on frozen rows and keeps the rows.
DELETE FROM users WHERE id = '00000000-0000-7000-8000-000000000003';
DO $$ BEGIN
    IF (SELECT created_by FROM runs WHERE id = '00000000-0000-7000-8000-00000000c101') IS NOT NULL
       OR (SELECT author_id FROM run_notes WHERE body = 'by dave') IS NOT NULL
       OR (SELECT author_id FROM experiment_notes WHERE body = 'also by dave') IS NOT NULL THEN
        RAISE EXCEPTION 'authorship survived the account deletion';
    END IF;
    IF (SELECT status FROM runs WHERE id = '00000000-0000-7000-8000-00000000c101') <> 'succeeded' THEN
        RAISE EXCEPTION 'run changed when its creator was deleted';
    END IF;
END $$;

-- Deleting the workspace removes everything it owned, the whole chain included.
DELETE FROM workspaces WHERE id = '00000000-0000-7000-8000-0000000000c1';
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM registry_events WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM runs WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM run_notes WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM experiment_notes WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM experiments WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM workspace_quotas WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM risk_limits WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM risk_kill_switches WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM risk_decisions WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM portfolio_positions WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM portfolio_balances WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1')
       OR EXISTS (SELECT 1 FROM equity_snapshots WHERE workspace_id = '00000000-0000-7000-8000-0000000000c1') THEN
        RAISE EXCEPTION 'workspace deletion left engine rows behind';
    END IF;
    -- Other workspaces' chains are untouched.
    IF (SELECT count(*) FROM registry_events WHERE workspace_id = '00000000-0000-7000-8000-0000000000a1') <> 2 THEN
        RAISE EXCEPTION 'another workspace lost registry events';
    END IF;
END $$;

-- Every table that holds user data has a non-null workspace_id.
DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['datasets', 'dataset_versions', 'strategies', 'strategy_versions', 'experiments',
                             'runs', 'run_artifacts', 'registry_events', 'jobs', 'job_events',
                             'symbol_changes', 'instrument_delistings', 'universes', 'universe_members',
                             'data_provider_configs', 'dataset_refresh_schedules',
                             'risk_limits', 'risk_kill_switches', 'risk_decisions',
                             'portfolio_positions', 'portfolio_balances', 'equity_snapshots'] LOOP
        IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                        WHERE table_name = t AND column_name = 'workspace_id' AND is_nullable = 'NO') THEN
            RAISE EXCEPTION '% lacks a NOT NULL workspace_id', t;
        END IF;
    END LOOP;
END $$;

\echo 'schema checks passed'
ROLLBACK;
