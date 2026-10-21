-- params: uuid, uuid, bigint, bigint, integer
-- Record a chunk of $4 bytes written at offset $3, if that is exactly where the session is and
-- it still fits; pushes the expiry back $5 seconds. No row otherwise.
-- $1 workspace, $2 session id, $3 offset, $4 chunk length, $5 seconds until it expires.
WITH upd AS (
    UPDATE upload_sessions
       SET received_bytes = received_bytes + $4, updated_at = now(), expires_at = now() + make_interval(secs => $5)
     WHERE workspace_id = $1 AND id = $2 AND upload_id IS NULL
       AND received_bytes = $3 AND $3 + $4 <= size_bytes
    RETURNING received_bytes
)
SELECT received_bytes FROM upd
