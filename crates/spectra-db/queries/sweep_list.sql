-- params: uuid, int
SELECT *
  FROM optimization_sweeps
 WHERE workspace_id = $1
 ORDER BY created_at DESC
 LIMIT $2;
