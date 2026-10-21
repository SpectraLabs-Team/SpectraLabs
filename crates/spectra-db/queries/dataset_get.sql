-- params: uuid, uuid
-- One dataset of the workspace. $1 workspace, $2 dataset id.
SELECT id, name, description, created_by, created_at FROM datasets WHERE workspace_id = $1 AND id = $2
