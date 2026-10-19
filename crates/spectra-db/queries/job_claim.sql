-- params: text, text[], bigint
-- Claim the next due job: highest priority, then oldest. Rows another worker is claiming are
-- skipped, not waited on. Workspaces already running $3 jobs are passed over (fair share; two
-- workers claiming at the same instant can overshoot the cap by one).
-- $1 worker id, $2 kinds this worker runs, $3 per-workspace running cap.
WITH busy AS (
    SELECT workspace_id, count(*) AS running FROM jobs WHERE status = 'running' GROUP BY workspace_id
), next AS (
    SELECT j.id FROM jobs j
    LEFT JOIN busy b ON b.workspace_id = j.workspace_id
     WHERE j.status = 'queued' AND j.run_after <= now() AND j.kind = ANY ($2)
       AND COALESCE(b.running, 0) < $3
     ORDER BY j.priority DESC, j.run_after, j.id
     LIMIT 1
       FOR UPDATE OF j SKIP LOCKED
), claimed AS (
    UPDATE jobs SET status = 'running', attempts = jobs.attempts + 1, started_at = now(), heartbeat_at = now(),
                    worker_id = $1, progress = 0, error = NULL
      FROM next WHERE jobs.id = next.id
    RETURNING jobs.*
), ev AS (
    INSERT INTO job_events (job_id, workspace_id, kind, data)
    SELECT id, workspace_id, 'claimed', jsonb_build_object('worker', $1::text, 'attempt', attempts) FROM claimed
)
SELECT * FROM claimed
