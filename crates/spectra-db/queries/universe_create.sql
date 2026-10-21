-- params: uuid, text, text
-- Create a universe. $1 workspace, $2 name, $3 description.
WITH ins AS (
    INSERT INTO universes (workspace_id, name, description)
    VALUES ($1, btrim($2), $3)
    RETURNING id, name, description, created_at
)
SELECT id, name, description, created_at FROM ins;
