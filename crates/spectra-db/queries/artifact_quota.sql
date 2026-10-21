-- params: uuid, text, bigint
-- Room for an artifact: the workspace's quota (or $3 when it has none), bytes used (each distinct
-- hash counted once) and whether this hash is already stored for the workspace.
-- $1 workspace, $2 sha256, $3 default quota in bytes.
SELECT COALESCE(q.artifact_quota_bytes, $3)::bigint AS quota,
       (SELECT COALESCE(sum(size_bytes), 0)::bigint FROM (
            SELECT DISTINCT ON (sha256) size_bytes FROM run_artifacts WHERE workspace_id = $1 ORDER BY sha256
        ) d) AS used,
       EXISTS (SELECT 1 FROM run_artifacts WHERE workspace_id = $1 AND sha256 = $2) AS present
  FROM workspaces w LEFT JOIN workspace_quotas q ON q.workspace_id = w.id WHERE w.id = $1
