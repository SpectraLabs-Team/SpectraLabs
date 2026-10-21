-- params: uuid, int
SELECT *
  FROM walk_forward_reports
 WHERE workspace_id = $1
 ORDER BY created_at DESC
 LIMIT $2;
