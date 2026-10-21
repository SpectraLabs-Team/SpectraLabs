-- params: uuid, uuid, int, jsonb, jsonb, double precision, double precision, double precision
INSERT INTO optimization_trials (
    id, sweep_id, trial_num, params, metrics, objective_value, returns_mean, returns_vol
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
RETURNING *;
