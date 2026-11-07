-- params: uuid, uuid
-- The holdings of a portfolio in a workspace, by symbol.
-- $1 workspace, $2 portfolio id.
SELECT h.symbol, h.weight
  FROM portfolio_holdings h
 WHERE h.workspace_id = $1 AND h.portfolio_id = $2
 ORDER BY h.symbol
