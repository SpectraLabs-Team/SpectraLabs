-- params: uuid, uuid, uuid, jsonb, jsonb, bigint, text, uuid, timestamptz, uuid, uuid, text, text
-- Record a finished backtest run, linked to the dataset version it ran on. $1 run id, $2 workspace,
-- $3 job id, $4 params, $5 metrics (the summary), $6 seed (nullable), $7 engine version,
-- $8 created by (nullable), $9 started at, $10 dataset version, $11 strategy version (nullable,
-- M2a task 4), $12 strategy name ('' for none), $13 strategy source hash ('' for none).
WITH r AS (
INSERT INTO runs (id, workspace_id, job_id, kind, status, params, metrics, seed, engine_version, created_by,
                  started_at, finished_at, dataset_version_id, strategy_version_id, strategy, strategy_sha256)
VALUES ($1, $2, $3, 'backtest', 'succeeded', $4, $5, $6, $7, $8, $9, now(), $10, $11, $12, $13)
RETURNING id
)
SELECT id FROM r
