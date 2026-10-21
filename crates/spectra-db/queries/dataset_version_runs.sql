-- params: uuid, uuid, integer
-- Runs that used a dataset version, newest first: the run, its job (a backtest's id in the API),
-- what ran and how it went. $1 workspace, $2 version id, $3 limit.
SELECT r.id, r.job_id, r.kind, r.status, r.params, r.created_at, r.finished_at,
       (r.metrics #>> '{metrics,total_return}')::double precision AS total_return
  FROM runs r
 WHERE r.workspace_id = $1 AND r.dataset_version_id = $2
 ORDER BY r.id DESC
 LIMIT $3
