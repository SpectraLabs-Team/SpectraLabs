-- params: integer
-- Remove up to $1 expired sessions; returns their ids, so their staging files can be removed.
WITH gone AS (
    DELETE FROM upload_sessions
     WHERE id IN (SELECT id FROM upload_sessions WHERE expires_at <= now() ORDER BY expires_at LIMIT $1)
    RETURNING id
)
SELECT id FROM gone
