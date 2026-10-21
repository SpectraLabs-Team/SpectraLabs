-- params: text
-- A workspace by id (as text) or slug. $1 id or slug.
SELECT id FROM workspaces WHERE id::text = lower($1) OR slug = $1
