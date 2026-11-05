-- params: uuid, uuid
-- Mark the strategy's paper account as starting now (after a reset).
UPDATE portfolio_balances
   SET started_at = now()
 WHERE workspace_id = $1
   AND strategy_id = $2;
