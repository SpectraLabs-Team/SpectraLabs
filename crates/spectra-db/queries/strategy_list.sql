-- params: uuid, bigint
-- A workspace's saved strategies, newest first, each with its version count and latest version.
-- $1 workspace, $2 limit.
SELECT s.id, s.name, s.description, s.created_by, s.created_at,
       (SELECT count(*) FROM strategy_versions v WHERE v.strategy_id = s.id) AS version_count,
       l.id AS latest_version_id, l.version AS latest_version, l.builtin AS latest_builtin
  FROM strategies s
  LEFT JOIN LATERAL (
        SELECT v.id, v.version, v.builtin FROM strategy_versions v
         WHERE v.strategy_id = s.id ORDER BY v.version DESC LIMIT 1
       ) l ON true
 WHERE s.workspace_id = $1
 ORDER BY s.created_at DESC, s.id DESC
 LIMIT $2
