-- params: uuid, uuid, text, uuid, int, text, uuid
INSERT INTO holdout_vault (
    id, workspace_id, name, dataset_version_id, budget, notes, sealed_by
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING *;
