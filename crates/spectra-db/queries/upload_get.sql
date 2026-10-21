-- params: uuid, uuid
-- An upload of the workspace that has not expired (ingested uploads never expire).
-- $1 workspace, $2 upload id.
SELECT id, sha256::text, size_bytes, filename, created_by, created_at, expires_at, ingested_at
  FROM uploads
 WHERE workspace_id = $1 AND id = $2 AND (ingested_at IS NOT NULL OR expires_at > now())
