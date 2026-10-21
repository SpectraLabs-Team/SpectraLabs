-- params: uuid, uuid
-- A session of the workspace that has not expired. $1 workspace, $2 session id.
SELECT id, filename, size_bytes, received_bytes, upload_id, expires_at
  FROM upload_sessions
 WHERE workspace_id = $1 AND id = $2 AND expires_at > now()
