-- params: uuid, uuid, uuid, uuid, int, jsonb, double precision, jsonb, uuid
INSERT INTO holdout_evaluations (
    id, holdout_id, workspace_id, strategy_version_id, evaluation_num, metrics, dsr, warnings, evaluated_by
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
RETURNING *;
