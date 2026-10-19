-- params: double precision
-- Running jobs whose heartbeat is older than $1 seconds lost their worker: requeue them, or fail
-- them once out of attempts (or cancel them, if cancel was requested). Returns how many.
WITH s AS (
    UPDATE jobs SET
        status      = CASE WHEN cancel_requested THEN 'cancelled'
                           WHEN attempts < max_attempts THEN 'queued' ELSE 'failed' END,
        finished_at = CASE WHEN NOT cancel_requested AND attempts < max_attempts THEN NULL ELSE now() END,
        error       = CASE WHEN cancel_requested THEN error
                           ELSE 'The worker running this job stopped responding.' END,
        worker_id = NULL, heartbeat_at = NULL, run_after = now()
     WHERE status = 'running' AND heartbeat_at < now() - make_interval(secs => $1)
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind, message)
    SELECT id, workspace_id, CASE status WHEN 'queued' THEN 'requeued' ELSE status END, COALESCE(error, '') FROM s
)
SELECT count(*) AS requeued FROM s
