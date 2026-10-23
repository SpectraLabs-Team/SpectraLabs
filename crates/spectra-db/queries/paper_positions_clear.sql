-- params: uuid, uuid
-- Clear all portfolio positions for a workspace and strategy (e.g. on paper account reset).
DELETE FROM portfolio_positions
 WHERE workspace_id = $1
   AND strategy_id = $2;
