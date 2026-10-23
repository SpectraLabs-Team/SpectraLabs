-- params: uuid, uuid
-- List fills for a given paper order.
SELECT
    id, order_id, workspace_id, strategy_id, symbol, side, qty, price, fee, created_at
  FROM paper_fills
 WHERE workspace_id = $1
   AND order_id = $2
 ORDER BY created_at ASC;
