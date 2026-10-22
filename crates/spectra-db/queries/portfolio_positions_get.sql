-- params: uuid, uuid
-- Fetch all current portfolio positions for a workspace and optional strategy.
SELECT symbol, qty, price, updated_at
  FROM portfolio_positions
 WHERE workspace_id = $1
   AND (($2::uuid IS NULL AND strategy_id IS NULL) OR strategy_id = $2::uuid);
