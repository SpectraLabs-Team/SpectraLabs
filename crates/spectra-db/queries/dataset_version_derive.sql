-- params: uuid, text, text, bigint, bigint, jsonb, uuid, text, jsonb, uuid, text, jsonb
-- Register a derived dataset version with lineage.
-- $1 workspace, $2 dataset name, $3 checksum, $4 row count, $5 byte size, $6 quality report,
-- $7 created by (nullable), $8 SHA-256 of bars.qfb in artifact store, $9 source jsonb,
-- $10 derived_from_id, $11 derivation_kind, $12 derivation_params.
WITH ds AS (
    INSERT INTO datasets (workspace_id, name, created_by)
    VALUES ($1, btrim($2), $7)
    ON CONFLICT (workspace_id, name) DO UPDATE SET name = EXCLUDED.name
    RETURNING id
), existing AS (
    SELECT v.id, v.version FROM dataset_versions v, ds WHERE v.dataset_id = ds.id AND v.checksum = $3
), backfill AS (
    UPDATE dataset_versions SET blob_sha256 = $8
     WHERE id IN (SELECT id FROM existing) AND blob_sha256 IS NULL AND $8 IS NOT NULL
), ins AS (
    INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size,
                                  quality_report, created_by, blob_sha256, source,
                                  derived_from_id, derivation_kind, derivation_params)
    SELECT $1, ds.id,
           COALESCE((SELECT max(version) FROM dataset_versions WHERE dataset_id = ds.id), 0) + 1,
           $3, $4, $5, $6, $7, $8, $9, $10, $11, $12
      FROM ds
     WHERE NOT EXISTS (SELECT 1 FROM existing)
    RETURNING id, version
)
SELECT id, version, false AS existed FROM ins
UNION ALL
SELECT id, version, true AS existed FROM existing;
