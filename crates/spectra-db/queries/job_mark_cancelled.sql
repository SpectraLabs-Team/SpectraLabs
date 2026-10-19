-- params: uuid, text
-- The worker stopped a running job because cancel was requested. $1 job id, $2 worker id.
WITH c AS (
    UPDATE jobs SET status = 'cancelled', finished_at = now(), worker_id = NULL, heartbeat_at = NULL
     WHERE id = $1 AND worker_id = $2 AND status = 'running'
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind) SELECT id, workspace_id, 'cancelled' FROM c
)
SELECT * FROM c
