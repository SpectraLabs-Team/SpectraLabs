-- Behaviour of the job queue, worker registry and artifact quota SQL in queries/*.sql.
--   node crates/spectra-db/scripts/prepare-queries.mjs > /tmp/prep.sql
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f /tmp/prep.sql -f crates/spectra-db/tests/queue_checks.sql
-- Runs in a transaction that is rolled back. Note: now() is fixed for a whole transaction.

BEGIN;
SET LOCAL client_min_messages = warning;

CREATE TEMP TABLE t_ids (name text PRIMARY KEY, id uuid);
INSERT INTO users (id, email, password_hash) VALUES ('00000000-0000-7000-8000-000000000001', 'q@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000000a1', 'qa', 'A'),
    ('00000000-0000-7000-8000-0000000000b1', 'qb', 'B');
-- A has its own artifact quota; B uses the deployment default.
INSERT INTO workspace_quotas (workspace_id, artifact_quota_bytes) VALUES
    ('00000000-0000-7000-8000-0000000000a1', 100);

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'queue check failed: %', what; END IF; END $$;

-- Enqueue: three in A (one high priority, one of another kind), one in B.
CREATE TEMP TABLE j1 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'benchmark', '{"n":1}', 0, 3, '00000000-0000-7000-8000-000000000001');
SELECT id AS j1 FROM j1 \gset
CREATE TEMP TABLE j2 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'benchmark', '{"n":2}', 10, 2, NULL);
SELECT id AS j2 FROM j2 \gset
CREATE TEMP TABLE j3 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'report', '{}', 99, 3, NULL);
SELECT id AS j3 FROM j3 \gset
CREATE TEMP TABLE j4 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000b1', 'benchmark', '{"n":4}', 0, 1, NULL);
SELECT id AS j4 FROM j4 \gset
SELECT pg_temp.check((SELECT status = 'queued' AND attempts = 0 AND progress = 0 FROM j1), 'enqueue defaults');
SELECT pg_temp.check((SELECT count(*) = 4 FROM job_events WHERE workspace_id IN ('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-0000000000b1') AND kind = 'queued'), 'queued events');

-- Claim: only kinds the worker runs; highest priority first; attempts counted.
CREATE TEMP TABLE c1 AS EXECUTE job_claim('w1', '{benchmark}', 1);
SELECT pg_temp.check((SELECT id FROM c1) = (SELECT id FROM j2), 'claim picks highest-priority benchmark, not the report job');
SELECT pg_temp.check((SELECT status = 'running' AND attempts = 1 AND worker_id = 'w1' FROM c1), 'claimed state');
-- Fair share: A already runs 1 job with cap 1, so the next claim goes to B, even though A's j1 is older.
CREATE TEMP TABLE c2 AS EXECUTE job_claim('w2', '{benchmark}', 1);
SELECT pg_temp.check((SELECT id FROM c2) = (SELECT id FROM j4), 'fair share skips the busy workspace');
CREATE TEMP TABLE c3 AS EXECUTE job_claim('w2', '{benchmark}', 1);
SELECT pg_temp.check((SELECT count(*) = 0 FROM c3), 'nothing claimable while both workspaces are at their cap');

-- Heartbeat: progress is clamped, NULL keeps it, and another worker cannot heartbeat it.
CREATE TEMP TABLE h1 AS EXECUTE job_heartbeat(:'j2', 'w1', 1.5);
CREATE TEMP TABLE h2 AS EXECUTE job_heartbeat(:'j2', 'w1', NULL);
CREATE TEMP TABLE h3 AS EXECUTE job_heartbeat(:'j2', 'intruder', 0.1);
SELECT pg_temp.check((SELECT progress = 1 FROM jobs WHERE id = (SELECT id FROM j2)), 'progress clamped and kept');
SELECT pg_temp.check((SELECT count(*) = 0 FROM h3), 'a non-owner heartbeat matches nothing');

-- Cancel while running: flagged; the heartbeat reports it; the worker acknowledges.
CREATE TEMP TABLE rc AS EXECUTE job_request_cancel('00000000-0000-7000-8000-0000000000a1', :'j2');
SELECT pg_temp.check((SELECT status = 'running' AND cancel_requested FROM rc), 'running job flagged, not stopped');
CREATE TEMP TABLE h4 AS EXECUTE job_heartbeat(:'j2', 'w1', 0.5);
SELECT pg_temp.check((SELECT cancel_requested FROM h4), 'heartbeat reports the cancel');
CREATE TEMP TABLE mc AS EXECUTE job_mark_cancelled(:'j2', 'w1');
SELECT pg_temp.check((SELECT status = 'cancelled' AND finished_at IS NOT NULL FROM mc), 'worker cancelled it');
-- A workspace cannot cancel another workspace's job.
CREATE TEMP TABLE rc2 AS EXECUTE job_request_cancel('00000000-0000-7000-8000-0000000000b1', :'j1');
SELECT pg_temp.check((SELECT count(*) = 0 FROM rc2), 'cross-workspace cancel matches nothing');

-- Fail with retry: back to the queue with a 5 s backoff, so not claimable yet.
CREATE TEMP TABLE c4 AS EXECUTE job_claim('w1', '{benchmark}', 1);
SELECT pg_temp.check((SELECT id FROM c4) = (SELECT id FROM j1), 'A is free again, j1 claimed');
CREATE TEMP TABLE f1 AS EXECUTE job_fail(:'j1', 'w1', 'boom', true);
SELECT pg_temp.check((SELECT status = 'queued' AND finished_at IS NULL AND worker_id IS NULL AND error = 'boom'
                        AND run_after = now() + interval '5 seconds' FROM f1), 'retry with 5 s backoff');
CREATE TEMP TABLE c5 AS EXECUTE job_claim('w1', '{benchmark}', 5);
SELECT pg_temp.check((SELECT count(*) = 0 FROM c5), 'backed-off job not claimable yet');
-- Pretend the backoff passed; second attempt; then fail on the last attempt → failed for good.
UPDATE jobs SET run_after = now() - interval '1 second', max_attempts = 2 WHERE id = (SELECT id FROM j1);
CREATE TEMP TABLE c6 AS EXECUTE job_claim('w1', '{benchmark}', 5);
SELECT pg_temp.check((SELECT attempts = 2 FROM c6), 'second attempt');
CREATE TEMP TABLE f2 AS EXECUTE job_fail(:'j1', 'w1', 'boom again', true);
SELECT pg_temp.check((SELECT status = 'failed' AND finished_at IS NOT NULL FROM f2), 'out of attempts → failed');

-- Complete: only the owner can; result stored.
CREATE TEMP TABLE j5 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'benchmark', '{}', 0, 3, NULL);
SELECT id AS j5 FROM j5 \gset
CREATE TEMP TABLE c7 AS EXECUTE job_claim('w3', '{benchmark}', 5);
CREATE TEMP TABLE d0 AS EXECUTE job_complete(:'j5', 'not-w3', '{"x":1}');
SELECT pg_temp.check((SELECT count(*) = 0 FROM d0), 'a non-owner cannot complete');
CREATE TEMP TABLE d1 AS EXECUTE job_complete(:'j5', 'w3', '{"x":1}');
SELECT pg_temp.check((SELECT status = 'succeeded' AND progress = 1 AND result = '{"x":1}' FROM d1), 'completed');

-- Stale heartbeat: requeued if attempts remain, failed if not, cancelled if cancel was asked.
CREATE TEMP TABLE j6 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'benchmark', '{}', 0, 3, NULL);
SELECT id AS j6 FROM j6 \gset
CREATE TEMP TABLE j7 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'benchmark', '{}', 0, 1, NULL);
SELECT id AS j7 FROM j7 \gset
CREATE TEMP TABLE j8 AS EXECUTE job_enqueue('00000000-0000-7000-8000-0000000000a1', 'benchmark', '{}', 0, 3, NULL);
SELECT id AS j8 FROM j8 \gset
UPDATE jobs SET status = 'running', attempts = 1, worker_id = 'dead', started_at = now(),
                heartbeat_at = now() - interval '2 minutes'
 WHERE id IN ((SELECT id FROM j6), (SELECT id FROM j7), (SELECT id FROM j8));
UPDATE jobs SET cancel_requested = true WHERE id = (SELECT id FROM j8);
CREATE TEMP TABLE s1 AS EXECUTE job_requeue_stale(30);
SELECT pg_temp.check((SELECT requeued = 3 FROM s1), 'three stale jobs handled');
SELECT pg_temp.check((SELECT status = 'queued' FROM jobs WHERE id = (SELECT id FROM j6)), 'stale with attempts left → queued');
SELECT pg_temp.check((SELECT status = 'failed' FROM jobs WHERE id = (SELECT id FROM j7)), 'stale out of attempts → failed');
SELECT pg_temp.check((SELECT status = 'cancelled' FROM jobs WHERE id = (SELECT id FROM j8)), 'stale with cancel → cancelled');
CREATE TEMP TABLE s2 AS EXECUTE job_requeue_stale(30);
SELECT pg_temp.check((SELECT requeued = 0 FROM s2), 'nothing stale left');

-- Cancel while queued: cancelled at once.
CREATE TEMP TABLE rc3 AS EXECUTE job_request_cancel('00000000-0000-7000-8000-0000000000a1', :'j6');
SELECT pg_temp.check((SELECT status = 'cancelled' AND finished_at IS NOT NULL FROM rc3), 'queued job cancelled immediately');

-- Listing: newest first, cursor pages, status filter, workspace scoped.
CREATE TEMP TABLE p1 AS EXECUTE job_list('00000000-0000-7000-8000-0000000000a1', NULL, NULL, 2);
SELECT id AS p1_min FROM p1 ORDER BY id LIMIT 1 \gset
CREATE TEMP TABLE p2 AS EXECUTE job_list('00000000-0000-7000-8000-0000000000a1', NULL, :'p1_min', 100);
SELECT pg_temp.check((SELECT count(*) = 2 FROM p1)
                     AND (SELECT id FROM p2 ORDER BY id DESC LIMIT 1) < (SELECT id FROM p1 ORDER BY id LIMIT 1)
                     AND (SELECT count(*) FROM p1) + (SELECT count(*) FROM p2)
                         = (SELECT count(*) FROM jobs WHERE workspace_id = '00000000-0000-7000-8000-0000000000a1'),
                     'cursor pagination covers every job once');
CREATE TEMP TABLE p3 AS EXECUTE job_list('00000000-0000-7000-8000-0000000000a1', 'failed', NULL, 100);
SELECT pg_temp.check((SELECT bool_and(status = 'failed') AND count(*) = 2 FROM p3), 'status filter');
CREATE TEMP TABLE pk AS EXECUTE job_list_kind('00000000-0000-7000-8000-0000000000a1', 'benchmark', NULL, NULL, 100);
SELECT pg_temp.check((SELECT bool_and(kind = 'benchmark') FROM pk)
                     AND (SELECT count(*) FROM pk) = (SELECT count(*) FROM jobs
                          WHERE workspace_id = '00000000-0000-7000-8000-0000000000a1' AND kind = 'benchmark')
                     AND (SELECT count(*) FROM pk) > 0, 'kind filter');
CREATE TEMP TABLE pk2 AS EXECUTE job_list_kind('00000000-0000-7000-8000-0000000000a1', 'nope', NULL, NULL, 100);
SELECT pg_temp.check((SELECT count(*) = 0 FROM pk2), 'unknown kind lists nothing');
CREATE TEMP TABLE g1 AS EXECUTE job_get('00000000-0000-7000-8000-0000000000b1', :'j1');
SELECT pg_temp.check((SELECT count(*) = 0 FROM g1), 'get is workspace scoped');

-- Workers.
EXECUTE worker_heartbeat('w1', 'host', '0.1.0', '{benchmark}', 2);
EXECUTE worker_heartbeat('w1', 'host', '0.1.0', '{benchmark,risk}', 4);
CREATE TEMP TABLE wa AS EXECUTE workers_alive(30);
SELECT pg_temp.check((SELECT alive = 1 FROM wa) AND (SELECT concurrency = 4 FROM workers WHERE id = 'w1'), 'worker upsert');
EXECUTE worker_remove('w1');
CREATE TEMP TABLE wb AS EXECUTE workers_alive(30);
SELECT pg_temp.check((SELECT alive = 0 FROM wb), 'worker removed');

-- Runs and artifacts: quota 100 bytes in A; the same hash counts once.
CREATE TEMP TABLE r1 AS EXECUTE run_insert('00000000-0000-7000-8000-00000000c001', '00000000-0000-7000-8000-0000000000a1',
    :'j5', 'benchmark', '{}', '{}', NULL, '0.1.0', NULL, now());
CREATE TEMP TABLE q1 AS EXECUTE artifact_quota('00000000-0000-7000-8000-0000000000a1', repeat('a', 64), 1000);
SELECT pg_temp.check((SELECT quota = 100 AND used = 0 AND NOT present FROM q1), 'quota from the workspace row');
CREATE TEMP TABLE a1 AS EXECUTE artifact_record('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-00000000c001',
    'report', repeat('a', 64), 'aa/aa/x', 60, 'application/json', 1000);
SELECT pg_temp.check((SELECT inserted = 1 FROM a1), 'first artifact fits');
CREATE TEMP TABLE a2 AS EXECUTE artifact_record('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-00000000c001',
    'trades', repeat('b', 64), 'bb/bb/x', 60, 'text/csv', 1000);
SELECT pg_temp.check((SELECT inserted = 0 AND NOT present AND used = 60 FROM a2), 'second artifact would exceed 100 bytes');
CREATE TEMP TABLE a3 AS EXECUTE artifact_record('00000000-0000-7000-8000-0000000000a1', '00000000-0000-7000-8000-00000000c001',
    'report-copy', repeat('a', 64), 'aa/aa/x', 60, 'application/json', 1000);
SELECT pg_temp.check((SELECT inserted = 1 AND present FROM a3), 'an already-stored hash costs nothing');
CREATE TEMP TABLE q2 AS EXECUTE artifact_quota('00000000-0000-7000-8000-0000000000b1', repeat('a', 64), 1000);
SELECT pg_temp.check((SELECT quota = 1000 AND used = 0 AND NOT present FROM q2), 'default quota; hashes are per workspace');

\echo 'queue checks passed'
ROLLBACK;
