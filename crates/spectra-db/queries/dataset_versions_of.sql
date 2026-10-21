-- params: uuid, uuid
-- Every version of a dataset, newest first, with how many runs use each.
-- $1 workspace, $2 dataset id.
SELECT v.id, v.version, v.checksum::text AS checksum, v.row_count, v.byte_size, v.created_by, v.created_at,
       COALESCE(v.quality_report->'symbols', '[]'::jsonb) AS symbols,
       (v.quality_report->>'first_ts_ns')::bigint AS first_ts_ns,
       (v.quality_report->>'last_ts_ns')::bigint AS last_ts_ns,
       COALESCE((v.quality_report->>'rows_rejected')::bigint, 0) AS rows_rejected,
       jsonb_array_length(COALESCE(v.quality_report->'warnings', '[]'::jsonb))::bigint AS warning_count,
       v.source->>'filename' AS filename,
       (SELECT count(*) FROM runs r WHERE r.workspace_id = v.workspace_id AND r.dataset_version_id = v.id)::bigint AS run_count
  FROM dataset_versions v
 WHERE v.workspace_id = $1 AND v.dataset_id = $2
 ORDER BY v.version DESC
