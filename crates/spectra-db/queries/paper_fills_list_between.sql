-- params: uuid, uuid, timestamptz, timestamptz
-- List a strategy's paper fills recorded at or after `since` and, when given, before `until`,
-- oldest first.
SELECT
    id, order_id, workspace_id, strategy_id, symbol, side, qty, price, fee, cost_config, created_at
  FROM paper_fills
 WHERE workspace_id = $1
   AND strategy_id = $2
   AND created_at >= $3
   AND ($4::timestamptz IS NULL OR created_at < $4)
 ORDER BY created_at ASC, id ASC;
