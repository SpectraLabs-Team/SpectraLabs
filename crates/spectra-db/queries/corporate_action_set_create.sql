-- params: uuid, text, text, uuid
-- Create a corporate action set in the workspace. $1 workspace, $2 name, $3 description, $4 created_by.
WITH ins AS (
    INSERT INTO corporate_action_sets (workspace_id, name, description, created_by)
    VALUES ($1, btrim($2), $3, $4)
    RETURNING id, name, description, created_at
)
SELECT id, name, description, created_at FROM ins;
