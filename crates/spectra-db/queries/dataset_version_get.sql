-- params: uuid, uuid
-- One dataset version of the workspace, in full: its dataset, quality report, source and where
-- its file is. $1 workspace, $2 version id.
SELECT v.id, v.dataset_id, d.name AS dataset_name, v.version, v.checksum::text AS checksum, v.row_count,
       v.byte_size, v.quality_report, v.source, v.blob_sha256::text AS blob_sha256, v.created_by, v.created_at,
       (SELECT count(*) FROM runs r WHERE r.workspace_id = v.workspace_id AND r.dataset_version_id = v.id)::bigint
           AS run_count
  FROM dataset_versions v
  JOIN datasets d ON d.workspace_id = v.workspace_id AND d.id = v.dataset_id
 WHERE v.workspace_id = $1 AND v.id = $2
