-- params: uuid
-- The portfolios of a workspace, by name. $1 workspace.
SELECT id, workspace_id, name, method, created_by, created_at
  FROM portfolios
 WHERE workspace_id = $1
 ORDER BY name
