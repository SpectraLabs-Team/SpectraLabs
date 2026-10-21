-- params: uuid, uuid, text, jsonb, uuid
-- Insert or update a strategy draft.
-- $1 workspace_id, $2 strategy_id, $3 code, $4 parameters, $5 updated_by
INSERT INTO strategy_drafts (workspace_id, strategy_id, code, parameters, updated_at, updated_by)
VALUES ($1, $2, $3, $4, now(), $5)
ON CONFLICT (strategy_id) DO UPDATE
   SET code = EXCLUDED.code,
       parameters = EXCLUDED.parameters,
       updated_at = now(),
       updated_by = EXCLUDED.updated_by
RETURNING strategy_id, workspace_id, code, parameters, updated_at, updated_by
