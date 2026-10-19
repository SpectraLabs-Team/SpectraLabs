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

-- Fixtures: two users, two workspaces.
INSERT INTO users (id, email, password_hash, role) VALUES
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

-- Every table that holds user data has a non-null workspace_id.
DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['datasets', 'dataset_versions', 'strategies', 'strategy_versions', 'experiments',
                             'runs', 'run_artifacts', 'registry_events', 'jobs', 'job_events'] LOOP
        IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                        WHERE table_name = t AND column_name = 'workspace_id' AND is_nullable = 'NO') THEN
            RAISE EXCEPTION '% lacks a NOT NULL workspace_id', t;
        END IF;
    END LOOP;
END $$;

\echo 'schema checks passed'
ROLLBACK;
