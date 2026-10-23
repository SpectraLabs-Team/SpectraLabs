-- params: uuid, uuid, uuid, text, text, double precision, double precision, double precision
-- Record a fill for a paper order.
INSERT INTO paper_fills (
    order_id, workspace_id, strategy_id, symbol, side, qty, price, fee
)
VALUES ($1, $2, $3, btrim($4), $5, $6, $7, $8)
RETURNING
    id, order_id, workspace_id, strategy_id, symbol, side, qty, price, fee, created_at;
