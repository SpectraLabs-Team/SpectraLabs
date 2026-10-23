-- params: uuid, uuid, text, bigint
-- List paper orders in a workspace, optionally filtered by strategy and/or status.
SELECT
    id, workspace_id, strategy_id, symbol, side, order_type,
    qty, limit_price, stop_price, tif, status, filled_qty, filled_avg_price,
    reject_reason, created_by, created_at, updated_at
  FROM paper_orders
 WHERE workspace_id = $1
   AND ($2::uuid IS NULL OR strategy_id = $2::uuid)
   AND ($3::text IS NULL OR status = $3::text)
 ORDER BY created_at DESC
 LIMIT $4;
