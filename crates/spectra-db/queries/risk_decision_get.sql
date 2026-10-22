-- params: uuid, uuid
-- Fetch a single pre-trade risk decision by its ID for replay and audit inspection.
SELECT id, workspace_id, strategy_id, symbol, side, requested_qty, price,
       approved_qty, verdict, breaches, reasons, metrics, actor_id, created_at
  FROM risk_decisions
 WHERE workspace_id = $1 AND id = $2;
