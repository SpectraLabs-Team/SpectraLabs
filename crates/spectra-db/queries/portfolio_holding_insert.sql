-- params: uuid, uuid, text, float8
-- One holding of a portfolio.
-- $1 portfolio, $2 workspace, $3 symbol, $4 weight.
INSERT INTO portfolio_holdings (portfolio_id, workspace_id, symbol, weight)
VALUES ($1, $2, $3, $4)
