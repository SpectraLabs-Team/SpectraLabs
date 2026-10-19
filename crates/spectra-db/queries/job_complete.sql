-- params: uuid, text, jsonb
-- Mark a job succeeded with its result. No row if this worker no longer owns it.
-- $1 job id, $2 worker id, $3 result.
WITH done AS (
    UPDATE jobs SET status = 'succeeded', progress = 1, result = $3, finished_at = now(), heartbeat_at = now()
     WHERE id = $1 AND worker_id = $2 AND status = 'running'
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind) SELECT id, workspace_id, 'succeeded' FROM done
)
SELECT * FROM done
