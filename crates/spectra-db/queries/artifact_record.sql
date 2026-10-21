-- params: uuid, uuid, text, text, text, bigint, text, bigint
-- Record a stored artifact against a run, re-checking the quota under a lock on the workspace row
<<<<<<< HEAD
-- so two writers cannot both squeeze under it (NO KEY UPDATE: foreign-key checks are not blocked). inserted = 0 with present = false means the quota
-- would be exceeded. $1 workspace, $2 run, $3 kind, $4 sha256, $5 path, $6 size, $7 content type,
=======
-- so two writers cannot both squeeze under it. inserted = 0 with present = false means the quota
-- would be exceeded (usage counts every hash in workspace_storage once). $1 workspace, $2 run, $3 kind, $4 sha256, $5 path, $6 size, $7 content type,
>>>>>>> origin/main
-- $8 default quota.
WITH ws AS (
    SELECT COALESCE(q.artifact_quota_bytes, $8)::bigint AS quota
      FROM workspaces w LEFT JOIN workspace_quotas q ON q.workspace_id = w.id
     WHERE w.id = $1 FOR NO KEY UPDATE OF w
), used AS (
    SELECT COALESCE(sum(size_bytes), 0)::bigint AS bytes FROM workspace_storage WHERE workspace_id = $1
), dup AS (
    SELECT EXISTS (SELECT 1 FROM workspace_storage WHERE workspace_id = $1 AND sha256 = $4) AS present
), ins AS (
    INSERT INTO run_artifacts (workspace_id, run_id, kind, sha256, path, size_bytes, content_type)
    SELECT $1, $2, $3, $4, $5, $6, $7 FROM ws, used, dup
     WHERE dup.present OR used.bytes + $6 <= ws.quota
    ON CONFLICT (run_id, kind, sha256) DO NOTHING
    RETURNING id
)
SELECT (SELECT count(*) FROM ins) AS inserted, (SELECT present FROM dup) AS present,
       (SELECT quota FROM ws) AS quota, (SELECT bytes FROM used) AS used
