-- params: uuid, uuid
-- One portfolio run in a workspace. $1 workspace, $2 run id.
SELECT * FROM portfolio_runs
 WHERE workspace_id = $1 AND id = $2
