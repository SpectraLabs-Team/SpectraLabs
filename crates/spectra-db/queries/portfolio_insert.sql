-- params: uuid, text, uuid (nullable). Inserts a portfolio; holdings follow in the same transaction.
-- $1 workspace, $2 name, $3 created_by.
INSERT INTO portfolios (workspace_id, name, created_by)
VALUES ($1, $2, $3)
RETURNING id, workspace_id, name, created_by, created_at
