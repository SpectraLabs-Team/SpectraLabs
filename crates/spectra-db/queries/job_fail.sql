-- params: uuid, text, text, boolean
-- A failed attempt. Retryable failures go back to the queue with exponential backoff (5 s, 10 s,
-- 20 s ... capped at 5 min) until max_attempts; others fail the job. No row if this worker no
-- longer owns it. $1 job id, $2 worker id, $3 error message, $4 retryable.
WITH f AS (
    UPDATE jobs SET
        status      = CASE WHEN $4 AND attempts < max_attempts THEN 'queued' ELSE 'failed' END,
        run_after   = CASE WHEN $4 AND attempts < max_attempts
                           THEN now() + make_interval(secs => LEAST(300, 5 * power(2, attempts - 1)))
                           ELSE run_after END,
        finished_at = CASE WHEN $4 AND attempts < max_attempts THEN NULL ELSE now() END,
        worker_id = NULL, heartbeat_at = NULL, error = $3
     WHERE id = $1 AND worker_id = $2 AND status = 'running'
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind, message)
    SELECT id, workspace_id, CASE WHEN status = 'queued' THEN 'retried' ELSE 'failed' END, $3 FROM f
)
SELECT * FROM f
