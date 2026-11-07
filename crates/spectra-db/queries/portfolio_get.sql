-- params: uuid, uuid
-- One portfolio in a workspace. $1 workspace, $2 portfolio id.
SELECT id, workspace_id, name, method, created_by, created_at
  FROM portfolios
 WHERE workspace_id = $1 AND id = $2
