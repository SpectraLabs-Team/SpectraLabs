-- params: uuid, uuid, text, uuid
-- Spends one evaluation of a holdout, logs the grant, and returns the sealed data's checksum.
-- No row: no such holdout in the workspace, or its budget is spent (logged as 'exhausted').
-- $1 workspace, $2 holdout id, $3 purpose, $4 user.
SELECT evaluation_num, budget, dataset_version_id, checksum, row_count, dataset_name, version, blob_sha256
  FROM grant_holdout_evaluation($1, $2, $3, $4)
