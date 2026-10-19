-- params: uuid, uuid
-- Cancel a job: a queued job is cancelled at once; a running one is flagged and its worker stops
-- it at the next check; a finished job is returned unchanged. $1 workspace, $2 job id.
WITH c AS (
    UPDATE jobs SET
        status           = CASE WHEN status = 'queued' THEN 'cancelled' ELSE status END,
        finished_at      = CASE WHEN status = 'queued' THEN now() ELSE finished_at END,
        cancel_requested = cancel_requested OR status IN ('queued', 'running')
     WHERE workspace_id = $1 AND id = $2
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind)
    SELECT id, workspace_id, CASE WHEN status = 'cancelled' THEN 'cancelled' ELSE 'cancel_requested' END
      FROM c WHERE status IN ('cancelled', 'running') AND cancel_requested
)
SELECT * FROM c
