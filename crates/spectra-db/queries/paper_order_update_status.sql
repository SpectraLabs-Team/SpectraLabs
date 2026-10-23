-- params: uuid, uuid, text, double precision, double precision, text
-- Update the status, filled quantity, filled average price and reject reason of a paper order.
UPDATE paper_orders
   SET status = $3,
       filled_qty = $4,
       filled_avg_price = $5,
       reject_reason = $6,
       updated_at = now()
 WHERE workspace_id = $1
   AND id = $2
RETURNING
    id, workspace_id, strategy_id, symbol, side, order_type,
    qty, limit_price, stop_price, tif, status, filled_qty, filled_avg_price,
    reject_reason, created_by, created_at, updated_at;
