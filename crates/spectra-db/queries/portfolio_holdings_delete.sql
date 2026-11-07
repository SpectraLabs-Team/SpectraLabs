-- params: uuid, uuid
-- Removes every holding of a portfolio, before new ones are inserted in the same transaction.
-- $1 workspace, $2 portfolio id.
DELETE FROM portfolio_holdings
 WHERE workspace_id = $1 AND portfolio_id = $2
