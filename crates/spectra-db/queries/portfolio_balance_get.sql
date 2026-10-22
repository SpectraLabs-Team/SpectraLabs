-- params: uuid, uuid
-- Fetch current portfolio balance and equity metrics for a workspace and optional strategy.
SELECT cash, equity, peak_equity, daily_start_equity, updated_at
  FROM portfolio_balances
 WHERE workspace_id = $1
   AND (($2::uuid IS NULL AND strategy_id IS NULL) OR strategy_id = $2::uuid);
