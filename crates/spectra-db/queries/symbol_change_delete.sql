-- params: uuid, uuid
-- Deletes a symbol change by id. $1 workspace, $2 id.
DELETE FROM symbol_changes
 WHERE workspace_id = $1 AND id = $2
RETURNING id;
