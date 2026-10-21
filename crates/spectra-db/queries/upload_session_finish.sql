-- params: uuid, uuid, uuid
-- Point a fully received session at the upload it became. $1 workspace, $2 session, $3 upload id.
UPDATE upload_sessions SET upload_id = $3, updated_at = now()
 WHERE workspace_id = $1 AND id = $2 AND received_bytes = size_bytes
