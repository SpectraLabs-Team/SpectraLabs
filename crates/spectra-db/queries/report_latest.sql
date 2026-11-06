-- params: uuid, text, text, uuid
-- The latest version of a report for a run, without its content. $1 workspace, $2 report type,
-- $3 run kind, $4 run id.
SELECT id, report_id, report_type, run_kind, run_id, version, format, content_type, sha256, size_bytes,
       created_by, created_at
  FROM reports
 WHERE workspace_id = $1 AND report_type = $2 AND run_kind = $3 AND run_id = $4
 ORDER BY version DESC
 LIMIT 1
