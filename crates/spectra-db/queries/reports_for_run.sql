-- params: uuid, text, uuid
-- Every report version for a run, newest first within each report, without content.
-- $1 workspace, $2 run kind, $3 run id.
SELECT id, report_id, report_type, run_kind, run_id, version, format, content_type, sha256, size_bytes,
       created_by, created_at
  FROM reports
 WHERE workspace_id = $1 AND run_kind = $2 AND run_id = $3
 ORDER BY report_type, version DESC
