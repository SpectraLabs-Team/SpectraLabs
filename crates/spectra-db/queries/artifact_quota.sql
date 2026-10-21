-- params: uuid, text, bigint
-- Room for an artifact: the workspace's quota (or $3 when it has none), bytes used (each distinct
-- hash counted once, across run artifacts, uploads and dataset files) and whether this hash is
-- already stored for the workspace. $1 workspace, $2 sha256, $3 default quota in bytes.
SELECT COALESCE(w.artifact_quota_bytes, $3)::bigint AS quota,
       (SELECT COALESCE(sum(size_bytes), 0)::bigint FROM workspace_storage WHERE workspace_id = $1) AS used,
       EXISTS (SELECT 1 FROM workspace_storage WHERE workspace_id = $1 AND sha256 = $2) AS present
  FROM workspaces w WHERE w.id = $1;
