-- params: uuid, uuid
-- Deletes a universe by id. $1 workspace, $2 id.
DELETE FROM universes
 WHERE workspace_id = $1 AND id = $2
RETURNING id;
