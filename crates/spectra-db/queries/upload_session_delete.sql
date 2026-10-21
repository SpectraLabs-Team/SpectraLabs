-- params: uuid, uuid
-- Abandon a session (its staging file is removed by the caller). $1 workspace, $2 session id.
WITH del AS (DELETE FROM upload_sessions WHERE workspace_id = $1 AND id = $2 RETURNING id)
SELECT id FROM del
