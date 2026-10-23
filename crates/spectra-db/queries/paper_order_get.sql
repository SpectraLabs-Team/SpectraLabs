-- params: uuid, uuid
-- Fetch a paper order by id within a workspace.
SELECT
    id, workspace_id, strategy_id, symbol, side, order_type,
    qty, limit_price, stop_price, tif, status, filled_qty, filled_avg_price,
    reject_reason, created_by, created_at, updated_at
  FROM paper_orders
 WHERE workspace_id = $1
   AND id = $2;
