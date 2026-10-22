-- params: uuid, uuid
-- Lists all member symbols of a universe. $1 workspace, $2 universe id.
SELECT id, symbol, added_at, removed_at, created_at
  FROM universe_members
 WHERE workspace_id = $1 AND universe_id = $2
 ORDER BY symbol ASC, added_at ASC;
