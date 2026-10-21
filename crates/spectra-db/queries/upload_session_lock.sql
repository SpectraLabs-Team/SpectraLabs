-- params: uuid, uuid
-- The same as upload_session_get, locking the row until the transaction ends, so chunks of one
-- session are written one at a time. $1 workspace, $2 session id.
SELECT id, filename, size_bytes, received_bytes, upload_id, expires_at
  FROM upload_sessions
 WHERE workspace_id = $1 AND id = $2 AND expires_at > now()
   FOR UPDATE
