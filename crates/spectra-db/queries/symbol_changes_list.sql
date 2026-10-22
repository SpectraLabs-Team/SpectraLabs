-- params: uuid
-- Lists all symbol changes for a workspace. $1 workspace.
SELECT id, old_symbol, new_symbol, effective_date, notes, created_at
  FROM symbol_changes
 WHERE workspace_id = $1
 ORDER BY effective_date DESC, old_symbol ASC;
