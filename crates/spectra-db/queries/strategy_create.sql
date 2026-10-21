-- params: uuid, text, text, uuid
-- Create a saved strategy (M2a task 3). $1 workspace, $2 name, $3 description, $4 created by
-- (nullable). No row when the workspace already has a strategy of that name.
WITH s AS (
INSERT INTO strategies (workspace_id, name, description, created_by)
VALUES ($1, $2, $3, $4)
ON CONFLICT (workspace_id, name) DO NOTHING
RETURNING id, name, description, created_by, created_at
)
SELECT id, name, description, created_by, created_at,
       0::bigint AS version_count, NULL::uuid AS latest_version_id, NULL::integer AS latest_version,
       NULL::text AS latest_builtin
  FROM s
