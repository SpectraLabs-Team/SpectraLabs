-- params: uuid, uuid, text, jsonb, text
-- Ends a running portfolio run. $1 workspace, $2 run id, $3 status (succeeded or failed),
-- $4 result (succeeded only), $5 error (failed only). No row if the run is not running.
UPDATE portfolio_runs
   SET status = $3, result = $4, error = $5, finished_at = clock_timestamp()
 WHERE workspace_id = $1 AND id = $2 AND status = 'running'
RETURNING *
