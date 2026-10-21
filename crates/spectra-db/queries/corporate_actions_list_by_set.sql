-- params: uuid, uuid
-- Fetch all actions in a set, ordered by ex_date and symbol. $1 workspace, $2 set id.
SELECT id, symbol, ex_date, action_kind, split_from, split_to, dividend_amount::text AS dividend_amount, created_at
  FROM corporate_actions
 WHERE workspace_id = $1 AND set_id = $2
 ORDER BY ex_date ASC, symbol ASC;
