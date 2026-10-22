-- params: uuid, uuid, text, double precision, double precision
-- Upsert a portfolio position for a workspace and optional strategy.
WITH upsert AS (
    INSERT INTO portfolio_positions (workspace_id, strategy_id, symbol, qty, price, updated_at)
    VALUES ($1, $2, btrim($3), $4, $5, now())
    ON CONFLICT (workspace_id, strategy_id, symbol) DO UPDATE SET
        qty = EXCLUDED.qty,
        price = EXCLUDED.price,
        updated_at = now()
    RETURNING id, workspace_id, strategy_id, symbol, qty, price, updated_at
)
SELECT * FROM upsert;
