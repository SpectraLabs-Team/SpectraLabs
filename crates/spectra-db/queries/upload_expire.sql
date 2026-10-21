-- params: integer
-- Remove up to $1 expired uploads that were never ingested. Returns the hashes nothing else
-- references any more (no other upload, run artifact or dataset file, in any workspace): their
-- bytes can be deleted from the artifact store.
WITH gone AS (
    DELETE FROM uploads
     WHERE id IN (SELECT id FROM uploads WHERE ingested_at IS NULL AND expires_at <= now()
                   ORDER BY expires_at LIMIT $1)
    RETURNING id, sha256::text AS sha256
)
SELECT DISTINCT g.sha256
  FROM gone g
 WHERE NOT EXISTS (SELECT 1 FROM uploads u WHERE u.sha256 = g.sha256 AND u.id NOT IN (SELECT id FROM gone))
   AND NOT EXISTS (SELECT 1 FROM run_artifacts a WHERE a.sha256 = g.sha256)
   AND NOT EXISTS (SELECT 1 FROM dataset_versions v WHERE v.blob_sha256 = g.sha256)
