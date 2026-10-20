-- params: uuid, integer
-- A workspace's dataset versions, newest first, with their dataset's name.
-- $1 workspace, $2 page size.
SELECT v.id, d.name AS dataset_name, v.version, v.checksum::text AS checksum, v.row_count, v.byte_size, v.created_at
  FROM dataset_versions v
  JOIN datasets d ON d.workspace_id = v.workspace_id AND d.id = v.dataset_id
 WHERE v.workspace_id = $1
 ORDER BY v.created_at DESC, v.id DESC
 LIMIT $2
