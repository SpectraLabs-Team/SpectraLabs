-- params: uuid, uuid, uuid, text, jsonb, jsonb, bigint, text, uuid, timestamptz
-- Record a finished run produced by a job. $1 run id, $2 workspace, $3 job id, $4 kind,
-- $5 params, $6 metrics, $7 seed (nullable), $8 engine version, $9 created by (nullable),
-- $10 started at.
WITH r AS (
INSERT INTO runs (id, workspace_id, job_id, kind, status, params, metrics, seed, engine_version, created_by,
                  started_at, finished_at)
VALUES ($1, $2, $3, $4, 'succeeded', $5, $6, $7, $8, $9, $10, now())
RETURNING id
)
SELECT id FROM r
