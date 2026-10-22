-- params: uuid, text, jsonb, double precision, double precision, double precision
UPDATE walk_forward_reports
   SET status = $2,
       folds = $3,
       efficiency_ratio = $4,
       in_sample_sharpe = $5,
       out_of_sample_sharpe = $6,
       finished_at = CASE WHEN $2 IN ('succeeded', 'failed', 'cancelled') THEN now() ELSE finished_at END
 WHERE id = $1
RETURNING *;
