-- params: uuid, text, real
-- A running job's heartbeat and progress (0..1, nullable to leave it). Returns no row if this
-- worker no longer owns the job (it was requeued as stale); otherwise whether cancel was asked.
-- $1 job id, $2 worker id, $3 progress.
WITH h AS (
    UPDATE jobs SET heartbeat_at = now(),
                    progress = CASE WHEN $3 IS NULL THEN progress ELSE LEAST(GREATEST($3, 0), 1) END
     WHERE id = $1 AND worker_id = $2 AND status = 'running'
    RETURNING cancel_requested
)
SELECT cancel_requested FROM h
