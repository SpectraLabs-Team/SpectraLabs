-- params: uuid, text, text, timestamptz, text
-- Record a symbol change. $1 workspace, $2 old symbol, $3 new symbol, $4 effective date, $5 notes.
WITH ins AS (
    INSERT INTO symbol_changes (workspace_id, old_symbol, new_symbol, effective_date, notes)
    VALUES ($1, btrim($2), btrim($3), $4, $5)
    RETURNING id, old_symbol, new_symbol, effective_date, notes, created_at
)
SELECT id, old_symbol, new_symbol, effective_date, notes, created_at FROM ins;
