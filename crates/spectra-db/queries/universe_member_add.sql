-- params: uuid, uuid, text, timestamptz
-- Adds a member symbol to a universe. $1 workspace, $2 universe id, $3 symbol, $4 added_at.
WITH ins AS (
    INSERT INTO universe_members (workspace_id, universe_id, symbol, added_at)
    VALUES ($1, $2, btrim($3), $4)
    RETURNING id, symbol, added_at, removed_at
)
SELECT id, symbol, added_at, removed_at FROM ins;
