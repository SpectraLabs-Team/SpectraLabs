-- params: uuid, text, int, int, jsonb, jsonb, double precision, double precision, double precision, double precision, double precision
UPDATE optimization_sweeps
   SET status = $2,
       trial_count = $3,
       best_trial_num = $4,
       best_params = $5,
       best_metrics = $6,
       best_objective = $7,
       dsr = $8,
       psr = $9,
       min_trl = $10,
       pbo = $11,
       finished_at = CASE WHEN $2 IN ('succeeded', 'failed', 'cancelled') THEN now() ELSE finished_at END
 WHERE id = $1
RETURNING *;
