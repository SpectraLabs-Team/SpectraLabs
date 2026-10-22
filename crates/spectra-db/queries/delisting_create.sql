-- params: uuid, text, timestamptz, text
-- Record an instrument delisting. $1 workspace, $2 symbol, $3 delisted_at, $4 reason.
WITH ins AS (
    INSERT INTO instrument_delistings (workspace_id, symbol, delisted_at, reason)
    VALUES ($1, btrim($2), $3, $4)
    RETURNING id, symbol, delisted_at, reason, created_at
)
SELECT id, symbol, delisted_at, reason, created_at FROM ins;
