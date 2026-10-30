-- params: uuid, text, text, uuid
-- Serialize version numbering for one report (run first, in the same transaction as
-- report_version_insert). $1 workspace, $2 report type, $3 run kind, $4 run id.
SELECT pg_advisory_xact_lock(hashtextextended('report:' || $1::text || ':' || $2 || ':' || $3 || ':' || $4::text, 0))
