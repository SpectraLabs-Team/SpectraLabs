-- params: uuid, text, jsonb, integer, integer, uuid
-- Queue a job. $1 workspace, $2 kind, $3 payload, $4 priority, $5 max attempts, $6 created by (nullable).
WITH j AS (
    INSERT INTO jobs (workspace_id, kind, payload, priority, max_attempts, created_by)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING *
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind) SELECT id, workspace_id, 'queued' FROM j
)
SELECT * FROM j
