-- params: uuid, integer
-- List recent risk decisions in a workspace.
SELECT *
  FROM risk_decisions
 WHERE workspace_id = $1
 ORDER BY created_at DESC
 LIMIT $2;
