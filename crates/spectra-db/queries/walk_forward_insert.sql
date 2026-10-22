-- params: uuid, uuid, uuid, uuid, uuid, uuid, text, int, int, int, int, text, jsonb, uuid
INSERT INTO walk_forward_reports (
    id, workspace_id, job_id, strategy_id, strategy_version_id, dataset_version_id,
    mode, train_size, test_size, step_size, embargo, objective, param_grid, created_by
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
RETURNING *;
