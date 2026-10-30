-- params: uuid, uuid, int4
-- One version of a report, with its content. $1 workspace, $2 report id, $3 version.
SELECT id, report_id, report_type, run_kind, run_id, version, format, content_type, sha256, size_bytes,
       created_by, created_at, content
  FROM reports
 WHERE workspace_id = $1 AND report_id = $2 AND version = $3
