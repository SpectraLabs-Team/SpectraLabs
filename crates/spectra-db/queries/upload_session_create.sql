-- params: uuid, uuid, text, bigint, integer, integer
-- Open a resumable upload, unless the workspace already has $6 open (unfinished) sessions: then
-- no row. $1 workspace, $2 created by (nullable), $3 file name (cut to 255), $4 size in bytes,
-- $5 seconds of inactivity before it expires, $6 most open sessions per workspace.
WITH ws AS (
    SELECT id FROM workspaces WHERE id = $1 FOR UPDATE
), ins AS (
    INSERT INTO upload_sessions (workspace_id, created_by, filename, size_bytes, expires_at)
    SELECT $1, $2, left($3, 255), $4, now() + make_interval(secs => $5)
      FROM ws
     WHERE (SELECT count(*) FROM upload_sessions s
             WHERE s.workspace_id = $1 AND s.upload_id IS NULL AND s.expires_at > now()) < $6
    RETURNING id, filename, size_bytes, received_bytes, upload_id, expires_at
)
SELECT id, filename, size_bytes, received_bytes, upload_id, expires_at FROM ins
