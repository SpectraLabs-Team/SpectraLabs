-- params: uuid, uuid, text, timestamptz, text, integer, integer, text
-- Insert a corporate action into a set. $1 workspace, $2 set id, $3 symbol, $4 ex date, $5 action kind, $6 split from, $7 split to, $8 dividend amount text.
INSERT INTO corporate_actions (workspace_id, set_id, symbol, ex_date, action_kind, split_from, split_to, dividend_amount)
VALUES ($1, $2, btrim($3), $4, $5, $6, $7, $8::numeric)
RETURNING id;
