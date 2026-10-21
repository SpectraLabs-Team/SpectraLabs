-- params: uuid, integer
-- The workspace's datasets, most recently updated first, each with its version count and its
-- latest version (symbols and date range from that version's quality report).
-- $1 workspace, $2 limit.
SELECT d.id, d.name, d.description, d.created_at,
       (SELECT count(*) FROM dataset_versions c WHERE c.dataset_id = d.id)::bigint AS version_count,
       l.id AS latest_id, l.version AS latest_version, l.row_count, l.byte_size,
       l.created_at AS updated_at,
       COALESCE(l.quality_report->'symbols', '[]'::jsonb) AS symbols,
       (l.quality_report->>'first_ts_ns')::bigint AS first_ts_ns,
       (l.quality_report->>'last_ts_ns')::bigint AS last_ts_ns
  FROM datasets d
  LEFT JOIN LATERAL (
        SELECT v.* FROM dataset_versions v WHERE v.dataset_id = d.id ORDER BY v.version DESC LIMIT 1
       ) l ON true
 WHERE d.workspace_id = $1
 ORDER BY COALESCE(l.created_at, d.created_at) DESC, d.id DESC
 LIMIT $2
