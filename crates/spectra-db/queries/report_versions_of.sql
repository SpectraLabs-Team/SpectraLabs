-- params: uuid, uuid
-- Every version of a report, newest first, without content. $1 workspace, $2 report id.
SELECT id, report_id, report_type, run_kind, run_id, version, format, content_type, sha256, size_bytes,
       created_by, created_at
  FROM reports
 WHERE workspace_id = $1 AND report_id = $2
 ORDER BY version DESC
