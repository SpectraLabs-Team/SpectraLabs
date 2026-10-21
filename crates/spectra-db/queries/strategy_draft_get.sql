-- params: uuid, uuid
-- The current draft of a strategy in a workspace.
-- $1 workspace_id, $2 strategy_id
SELECT strategy_id, workspace_id, code, parameters, updated_at, updated_by
  FROM strategy_drafts
 WHERE workspace_id = $1 AND strategy_id = $2
