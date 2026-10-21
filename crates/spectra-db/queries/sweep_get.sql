-- params: uuid, uuid
SELECT *
  FROM optimization_sweeps
 WHERE workspace_id = $1 AND id = $2;
