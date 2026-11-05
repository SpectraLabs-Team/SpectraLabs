-- params: uuid, uuid, timestamptz
-- List a strategy's paper fills recorded at or after a time, oldest first.
SELECT
    id, order_id, workspace_id, strategy_id, symbol, side, qty, price, fee, cost_config, created_at
  FROM paper_fills
 WHERE workspace_id = $1
   AND strategy_id = $2
   AND created_at >= $3
 ORDER BY created_at ASC, id ASC;
