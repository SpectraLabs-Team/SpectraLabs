-- params: uuid, text, bigint, text, uuid, bigint, integer
-- Record an uploaded file whose bytes are already in the artifact store, re-checking the quota
-- under a lock on the workspace row (as artifact_record does). No row when the quota would be
-- exceeded and the workspace does not already hold these bytes.
-- $1 workspace, $2 sha256, $3 size, $4 file name, $5 created by (nullable), $6 default quota,
-- $7 seconds until an un-ingested upload expires.
WITH ws AS (
    SELECT COALESCE(artifact_quota_bytes, $6)::bigint AS quota FROM workspaces WHERE id = $1 FOR UPDATE
), used AS (
    SELECT COALESCE(sum(size_bytes), 0)::bigint AS bytes FROM workspace_storage WHERE workspace_id = $1
), dup AS (
    SELECT EXISTS (SELECT 1 FROM workspace_storage WHERE workspace_id = $1 AND sha256 = $2) AS present
), ins AS (
    INSERT INTO uploads (workspace_id, sha256, size_bytes, filename, created_by, expires_at)
    SELECT $1, $2, $3, left($4, 255), $5, now() + make_interval(secs => $7)
      FROM ws, used, dup
     WHERE dup.present OR used.bytes + $3 <= ws.quota
    RETURNING id, expires_at
)
SELECT id, expires_at FROM ins
