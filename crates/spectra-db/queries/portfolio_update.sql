-- params: uuid, uuid, text, text
-- Renames a portfolio and records its method; its holdings are replaced in the same transaction.
-- $1 workspace, $2 portfolio id, $3 name, $4 method. No row if it is not in the workspace.
UPDATE portfolios
   SET name = $3, method = $4
 WHERE workspace_id = $1 AND id = $2
RETURNING id, workspace_id, name, method, created_by, created_at
