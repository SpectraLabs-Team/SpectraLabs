-- params: uuid, uuid
SELECT *
  FROM walk_forward_reports
 WHERE workspace_id = $1 AND id = $2;
