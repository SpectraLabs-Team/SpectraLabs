-- params: uuid, text, text, jsonb
-- Fail a job for good, keeping a result that explains why (an ingest whose file was rejected
-- carries its quality report). Never retried. No row if this worker no longer owns it.
-- $1 job id, $2 worker id, $3 error message, $4 result.
WITH f AS (
    UPDATE jobs SET status = 'failed', finished_at = now(), worker_id = NULL, heartbeat_at = NULL,
                    error = $3, result = $4
     WHERE id = $1 AND worker_id = $2 AND status = 'running'
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind, message) SELECT id, workspace_id, 'failed', $3 FROM f
)
SELECT * FROM f
