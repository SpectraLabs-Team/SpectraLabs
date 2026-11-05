-- params: uuid, uuid, uuid, text, text, double precision, double precision, double precision, jsonb
-- Record a fill for a paper order, with the cost configuration that priced it.
INSERT INTO paper_fills (
    order_id, workspace_id, strategy_id, symbol, side, qty, price, fee, cost_config
)
VALUES ($1, $2, $3, btrim($4), $5, $6, $7, $8, $9)
RETURNING
    id, order_id, workspace_id, strategy_id, symbol, side, qty, price, fee, cost_config, created_at;
