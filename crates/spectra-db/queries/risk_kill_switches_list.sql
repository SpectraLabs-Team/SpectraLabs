-- params: uuid
-- List all kill switches in a workspace.
SELECT *
  FROM risk_kill_switches
 WHERE workspace_id = $1
 ORDER BY active DESC, triggered_at DESC;
