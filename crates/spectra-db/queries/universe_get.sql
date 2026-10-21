-- params: uuid, uuid
-- Fetches a universe by id. $1 workspace, $2 id.
SELECT id, name, description, created_at
  FROM universes
 WHERE workspace_id = $1 AND id = $2;
