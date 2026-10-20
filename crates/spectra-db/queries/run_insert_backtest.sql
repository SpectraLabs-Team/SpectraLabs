-- params: uuid, uuid, uuid, jsonb, jsonb, bigint, text, uuid, timestamptz, uuid
-- Record a finished backtest run, linked to the dataset version it ran on. $1 run id, $2 workspace,
-- $3 job id, $4 params, $5 metrics (the summary), $6 seed (nullable), $7 engine version,
-- $8 created by (nullable), $9 started at, $10 dataset version.
WITH r AS (
INSERT INTO runs (id, workspace_id, job_id, kind, status, params, metrics, seed, engine_version, created_by,
                  started_at, finished_at, dataset_version_id)
VALUES ($1, $2, $3, 'backtest', 'succeeded', $4, $5, $6, $7, $8, $9, now(), $10)
RETURNING id
)
SELECT id FROM r
