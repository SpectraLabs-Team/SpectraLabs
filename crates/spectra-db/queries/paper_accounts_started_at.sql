-- params: uuid
-- Every paper account in the workspace, with when its current account started.
SELECT strategy_id, started_at
  FROM portfolio_balances
 WHERE workspace_id = $1
 ORDER BY strategy_id;
