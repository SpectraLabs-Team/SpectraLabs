-- params: uuid, uuid, text, jsonb, uuid, int
-- Insert or update a strategy draft with optimistic concurrency control.
-- $1 workspace_id, $2 strategy_id, $3 code, $4 parameters, $5 updated_by, $6 expected_version
WITH existing AS (
    SELECT version FROM strategy_drafts WHERE strategy_id = $2
)
INSERT INTO strategy_drafts (workspace_id, strategy_id, code, parameters, updated_at, updated_by, version)
SELECT $1, id, $3::text, $4::jsonb, now(), $5::uuid, 1
  FROM strategies
 WHERE id = $2
   AND workspace_id = $1
   AND (
       ($6::int = 0 AND NOT EXISTS (SELECT 1 FROM existing))
       OR
       EXISTS (SELECT 1 FROM existing WHERE version = $6::int)
   )
ON CONFLICT (strategy_id) DO UPDATE
   SET code = EXCLUDED.code,
       parameters = EXCLUDED.parameters,
       updated_at = now(),
       updated_by = EXCLUDED.updated_by,
       version = strategy_drafts.version + 1
 WHERE strategy_drafts.workspace_id = EXCLUDED.workspace_id
   AND strategy_drafts.version = $6::int
RETURNING strategy_id, workspace_id, code, parameters, updated_at, updated_by, version
