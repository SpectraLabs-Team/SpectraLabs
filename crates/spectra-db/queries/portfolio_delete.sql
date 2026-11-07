-- params: uuid, uuid
-- Deletes a portfolio and its holdings; its runs keep their inputs and lose the link.
-- $1 workspace, $2 portfolio id. No row if it is not in the workspace.
DELETE FROM portfolios
 WHERE workspace_id = $1 AND id = $2
RETURNING id
