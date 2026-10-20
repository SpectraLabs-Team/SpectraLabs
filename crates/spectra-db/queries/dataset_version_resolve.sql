-- params: uuid, uuid
-- The dataset-store checksum and size of a dataset version, if it belongs to the workspace.
-- $1 workspace, $2 dataset version id.
SELECT v.id, v.checksum::text AS checksum, v.row_count, d.name AS dataset_name, v.version
  FROM dataset_versions v
  JOIN datasets d ON d.workspace_id = v.workspace_id AND d.id = v.dataset_id
 WHERE v.workspace_id = $1 AND v.id = $2
