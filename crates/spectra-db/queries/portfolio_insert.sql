-- params: uuid, text, uuid (nullable), text. Inserts a portfolio; holdings follow in the same transaction.
-- $1 workspace, $2 name, $3 created_by, $4 method.
INSERT INTO portfolios (workspace_id, name, created_by, method)
VALUES ($1, $2, $3, $4)
RETURNING id, workspace_id, name, method, created_by, created_at
