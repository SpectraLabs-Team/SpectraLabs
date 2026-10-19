-- params: text
-- A worker shut down cleanly. $1 id.
DELETE FROM workers WHERE id = $1
