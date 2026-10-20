-- params: uuid, text, text, bigint, bigint, jsonb, uuid
-- Register a dataset-store entry as a version of a named dataset (created if new). The same
-- checksum under the same dataset returns the existing version instead of a new one; otherwise
-- the version number is the next one. $1 workspace, $2 dataset name, $3 checksum, $4 row count,
-- $5 byte size, $6 quality report, $7 created by (nullable).
WITH ds AS (
    INSERT INTO datasets (workspace_id, name, created_by)
    VALUES ($1, btrim($2), $7)
    ON CONFLICT (workspace_id, name) DO UPDATE SET name = EXCLUDED.name
    RETURNING id
), existing AS (
    SELECT v.id, v.version FROM dataset_versions v, ds WHERE v.dataset_id = ds.id AND v.checksum = $3
), ins AS (
    INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size,
                                  quality_report, created_by)
    SELECT $1, ds.id,
           COALESCE((SELECT max(version) FROM dataset_versions WHERE dataset_id = ds.id), 0) + 1,
           $3, $4, $5, $6, $7
      FROM ds
     WHERE NOT EXISTS (SELECT 1 FROM existing)
    RETURNING id, version
)
SELECT id, version, false AS existed FROM ins
UNION ALL
SELECT id, version, true AS existed FROM existing
