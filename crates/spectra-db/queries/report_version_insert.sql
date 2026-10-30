-- params: uuid, text, text, uuid, text, text, bytea, uuid
-- Add the next version of a report (run after report_lock, in the same transaction). The first
-- version starts a new report_id; later ones reuse it. Returns the new row without its content.
-- $1 workspace, $2 report type, $3 run kind, $4 run id, $5 format, $6 content type,
-- $7 content, $8 created by (nullable).
WITH prev AS (
    SELECT report_id, max(version) AS version
      FROM reports
     WHERE workspace_id = $1 AND report_type = $2 AND run_kind = $3 AND run_id = $4
     GROUP BY report_id
), v AS (
INSERT INTO reports (workspace_id, report_id, report_type, run_kind, run_id, version, format, content_type,
                     sha256, size_bytes, content, created_by)
SELECT $1, coalesce((SELECT report_id FROM prev), spectra_uuidv7()), $2, $3, $4,
       coalesce((SELECT version FROM prev), 0) + 1, $5, $6, encode(sha256($7), 'hex'), octet_length($7), $7, $8
RETURNING id, report_id, report_type, run_kind, run_id, version, format, content_type, sha256, size_bytes,
          created_by, created_at
)
SELECT id, report_id, report_type, run_kind, run_id, version, format, content_type, sha256, size_bytes,
       created_by, created_at
  FROM v
