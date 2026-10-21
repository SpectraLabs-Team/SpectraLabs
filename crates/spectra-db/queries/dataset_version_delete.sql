-- params: uuid, uuid
-- Delete a dataset version unless a run or a queued or running backtest uses it. The dataset goes
-- too when this was its last version, and the upload it was made from is released (it expires
-- now, so the worker removes it) unless another version came from the same upload.
-- One row: found, in_use, deleted, dataset_deleted, and the version's file hash when nothing else
-- references it any more (so the caller can delete it from the artifact store).
-- $1 workspace, $2 version id.
WITH target AS (
    SELECT id, dataset_id, blob_sha256::text AS blob_sha256, source
      FROM dataset_versions WHERE workspace_id = $1 AND id = $2
       FOR UPDATE
), used AS (
    SELECT EXISTS (SELECT 1 FROM runs WHERE workspace_id = $1 AND dataset_version_id = $2)
        OR EXISTS (SELECT 1 FROM jobs WHERE workspace_id = $1 AND kind = 'backtest'
                      AND status IN ('queued', 'running') AND payload->>'dataset_version_id' = $2::text)
           AS in_use
), del AS (
    DELETE FROM dataset_versions
     WHERE id IN (SELECT id FROM target) AND NOT (SELECT in_use FROM used)
    RETURNING id, dataset_id, blob_sha256::text AS blob_sha256, source
), ds AS (
    DELETE FROM datasets d
     WHERE d.id IN (SELECT dataset_id FROM del)
       AND NOT EXISTS (SELECT 1 FROM dataset_versions v WHERE v.dataset_id = d.id AND v.id NOT IN (SELECT id FROM del))
    RETURNING d.id
), released AS (
    UPDATE uploads u SET ingested_at = NULL, expires_at = now()
      FROM del
     WHERE u.workspace_id = $1 AND u.id::text = del.source->>'upload_id'
       AND NOT EXISTS (SELECT 1 FROM dataset_versions v WHERE v.workspace_id = $1 AND v.id <> del.id
                          AND v.source->>'upload_id' = del.source->>'upload_id')
    RETURNING u.id
)
SELECT EXISTS (SELECT 1 FROM target) AS found,
       (SELECT in_use FROM used) AS in_use,
       EXISTS (SELECT 1 FROM del) AS deleted,
       EXISTS (SELECT 1 FROM ds) AS dataset_deleted,
       (SELECT del.blob_sha256 FROM del
         WHERE del.blob_sha256 IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM dataset_versions v WHERE v.blob_sha256 = del.blob_sha256 AND v.id <> del.id)
           AND NOT EXISTS (SELECT 1 FROM uploads u WHERE u.sha256 = del.blob_sha256)
           AND NOT EXISTS (SELECT 1 FROM run_artifacts a WHERE a.sha256 = del.blob_sha256)) AS orphaned_blob
