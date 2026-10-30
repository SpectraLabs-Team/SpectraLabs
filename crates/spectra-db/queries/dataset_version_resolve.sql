-- params: uuid, uuid, text, uuid
-- The dataset-store checksum and size of a dataset version, if it belongs to the workspace, and
-- where its bars.qfb is in the artifact store (NULL: only in the local dataset store).
-- A sealed holdout comes back with NULL checksum and blob and the holdout's name in
-- sealed_holdout, and the attempt is logged (see resolve_dataset_version in the M4a guard migration).
-- $1 workspace, $2 dataset version id, $3 purpose ('job', 'sweep', ...), $4 user (NULL: engine).
SELECT id, checksum, row_count, dataset_name, version, blob_sha256, sealed_holdout
  FROM resolve_dataset_version($1, $2, $3, $4)
