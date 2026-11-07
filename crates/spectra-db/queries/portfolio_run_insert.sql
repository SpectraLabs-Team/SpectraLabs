-- params: uuid, uuid, jsonb, uuid
-- Starts a portfolio run as running. $1 workspace, $2 portfolio, $3 request, $4 created_by (nullable).
INSERT INTO portfolio_runs (workspace_id, portfolio_id, status, request, created_by)
VALUES ($1, $2, 'running', $3, $4)
RETURNING *
