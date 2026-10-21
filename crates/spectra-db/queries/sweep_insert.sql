-- params: uuid, uuid, uuid, uuid, uuid, uuid, text, text, jsonb, uuid
INSERT INTO optimization_sweeps (
    id, workspace_id, job_id, strategy_id, strategy_version_id, dataset_version_id,
    method, objective, param_grid, created_by
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
RETURNING *;
