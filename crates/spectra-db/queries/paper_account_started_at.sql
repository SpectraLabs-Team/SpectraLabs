-- params: uuid, uuid
-- When the strategy's current paper account started.
SELECT started_at
  FROM portfolio_balances
 WHERE workspace_id = $1
   AND strategy_id = $2;
