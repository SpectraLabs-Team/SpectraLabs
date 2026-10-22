-- params: uuid
-- Lists all instrument delistings for a workspace. $1 workspace.
SELECT id, symbol, delisted_at, reason, created_at
  FROM instrument_delistings
 WHERE workspace_id = $1
 ORDER BY delisted_at DESC, symbol ASC;
