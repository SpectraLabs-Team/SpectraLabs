-- params: uuid, uuid, text, text
-- Rename a dataset and/or change its description; NULL leaves a field as it is. No row if the
-- dataset is not in the workspace. A name another dataset of the workspace has fails the unique
-- constraint (23505). $1 workspace, $2 dataset id, $3 name, $4 description.
WITH upd AS (
    UPDATE datasets
       SET name = COALESCE(btrim($3), name), description = COALESCE($4, description)
     WHERE workspace_id = $1 AND id = $2
    RETURNING id, name, description, created_by, created_at
)
SELECT id, name, description, created_by, created_at FROM upd
