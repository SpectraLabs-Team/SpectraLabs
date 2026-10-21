-- params: uuid, uuid
-- Lock a saved strategy for the rest of the transaction, so its versions are numbered one at a
-- time. $1 workspace, $2 strategy id. No row: no such strategy in the workspace.
SELECT id FROM strategies WHERE workspace_id = $1 AND id = $2 FOR UPDATE
