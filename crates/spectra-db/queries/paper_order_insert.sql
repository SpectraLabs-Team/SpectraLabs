-- params: uuid, uuid, text, text, text, double precision, double precision, double precision, text, text, text, uuid
-- Insert a new paper order.
INSERT INTO paper_orders (
    workspace_id, strategy_id, symbol, side, order_type,
    qty, limit_price, stop_price, tif, status, reject_reason, created_by
)
VALUES ($1, $2, btrim($3), $4, $5, $6, $7, $8, $9, $10, $11, $12)
RETURNING
    id, workspace_id, strategy_id, symbol, side, order_type,
    qty, limit_price, stop_price, tif, status, filled_qty, filled_avg_price,
    reject_reason, created_by, created_at, updated_at;
