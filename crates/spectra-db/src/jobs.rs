//! The job queue: a `jobs` table claimed with `FOR UPDATE SKIP LOCKED` (no
//! separate broker). Every statement lives in `queries/`, where
//! `tests/queue_checks.sql` exercises the same text.
//!
//! Lifecycle: `queued` → `running` (claimed; heartbeats every few seconds) →
//! `succeeded` | `failed` | `cancelled`. A retryable failure goes back to
//! `queued` with exponential backoff until `max_attempts`. A running job whose
//! heartbeat goes stale is requeued by [`requeue_stale`].

use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// Where a job is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Waiting for a worker.
    Queued,
    /// Claimed by a worker.
    Running,
    /// Finished with a result.
    Succeeded,
    /// Finished with an error.
    Failed,
    /// Stopped on request.
    Cancelled,
}

impl JobStatus {
    /// Every status.
    pub const ALL: [JobStatus; 5] = [
        JobStatus::Queued,
        JobStatus::Running,
        JobStatus::Succeeded,
        JobStatus::Failed,
        JobStatus::Cancelled,
    ];

    /// The database spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    /// Parses the database spelling.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Whether the job is finished.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled)
    }
}

/// A row of `jobs`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Job {
    /// UUIDv7.
    pub id: Uuid,
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// What to run, e.g. `benchmark`.
    pub kind: String,
    /// Kind-specific input.
    pub payload: JsonValue,
    /// See [`JobStatus`].
    pub status: String,
    /// Higher runs first.
    pub priority: i32,
    /// Attempts started so far.
    pub attempts: i32,
    /// Attempts allowed.
    pub max_attempts: i32,
    /// 0..1.
    pub progress: f32,
    /// A cancel was asked for while it ran.
    pub cancel_requested: bool,
    /// Not claimed before this time (retry backoff).
    pub run_after: DateTime<Utc>,
    /// Last heartbeat while running.
    pub heartbeat_at: Option<DateTime<Utc>>,
    /// When the latest attempt started.
    pub started_at: Option<DateTime<Utc>>,
    /// When it reached a terminal status.
    pub finished_at: Option<DateTime<Utc>>,
    /// The worker running it.
    pub worker_id: Option<String>,
    /// The latest error.
    pub error: Option<String>,
    /// Kind-specific output.
    pub result: Option<JsonValue>,
    /// Who queued it.
    pub created_by: Option<Uuid>,
    /// When it was queued.
    pub created_at: DateTime<Utc>,
}

impl Job {
    /// The parsed status (`Failed` for an unknown spelling, which the schema forbids).
    pub fn status(&self) -> JobStatus {
        JobStatus::parse(&self.status).unwrap_or(JobStatus::Failed)
    }
}

/// A job to queue.
#[derive(Debug, Clone)]
pub struct NewJob {
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// What to run.
    pub kind: String,
    /// Kind-specific input.
    pub payload: JsonValue,
    /// Higher runs first.
    pub priority: i32,
    /// Attempts allowed (at least 1).
    pub max_attempts: i32,
    /// Who queued it.
    pub created_by: Option<Uuid>,
}

/// Queues a job.
///
/// # Errors
/// Any database error, including a kind or attempt count the schema refuses.
pub async fn enqueue(pool: &PgPool, job: &NewJob) -> Result<Job, Error> {
    sqlx::query_as(include_str!("../queries/job_enqueue.sql"))
        .bind(job.workspace_id)
        .bind(&job.kind)
        .bind(&job.payload)
        .bind(job.priority)
        .bind(job.max_attempts)
        .bind(job.created_by)
        .fetch_one(pool)
        .await
}

/// One of a workspace's jobs.
///
/// # Errors
/// Any database error.
pub async fn get(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<Job>, Error> {
    sqlx::query_as(include_str!("../queries/job_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// A page of results with a cursor for the next one.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    /// This page, newest first.
    pub items: Vec<T>,
    /// Pass back to get the next page; `None` on the last one.
    pub next_cursor: Option<Uuid>,
}

/// A page of a workspace's jobs, newest first. `cursor` is the previous
/// page's `next_cursor`.
///
/// # Errors
/// Any database error.
pub async fn list(
    pool: &PgPool,
    workspace_id: Uuid,
    status: Option<JobStatus>,
    cursor: Option<Uuid>,
    limit: u32,
) -> Result<Page<Job>, Error> {
    let limit = limit.clamp(1, 200);
    // One extra row tells us whether another page exists.
    let mut items: Vec<Job> = sqlx::query_as(include_str!("../queries/job_list.sql"))
        .bind(workspace_id)
        .bind(status.map(JobStatus::as_str))
        .bind(cursor)
        .bind(i64::from(limit) + 1)
        .fetch_all(pool)
        .await?;
    let next_cursor = if items.len() > limit as usize {
        items.truncate(limit as usize);
        items.last().map(|j| j.id)
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}

/// Asks for a job to stop: a queued job is cancelled at once, a running one
/// is flagged for its worker. Finished jobs come back unchanged.
///
/// # Errors
/// Any database error.
pub async fn request_cancel(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<Job>, Error> {
    sqlx::query_as(include_str!("../queries/job_request_cancel.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Claims the next due job of one of `kinds`, skipping workspaces that
/// already run `per_workspace_cap` jobs.
///
/// # Errors
/// Any database error.
pub async fn claim(
    pool: &PgPool,
    worker_id: &str,
    kinds: &[String],
    per_workspace_cap: i64,
) -> Result<Option<Job>, Error> {
    sqlx::query_as(include_str!("../queries/job_claim.sql"))
        .bind(worker_id)
        .bind(kinds)
        .bind(per_workspace_cap)
        .fetch_optional(pool)
        .await
}

/// What a heartbeat learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    /// Keep going.
    Continue,
    /// Stop: someone asked to cancel.
    CancelRequested,
    /// This worker no longer owns the job (it was requeued as stale): drop it.
    Lost,
}

/// Records a heartbeat and, optionally, progress (0..1).
///
/// # Errors
/// Any database error.
pub async fn heartbeat(
    pool: &PgPool,
    id: Uuid,
    worker_id: &str,
    progress: Option<f32>,
) -> Result<Beat, Error> {
    let cancel: Option<bool> = sqlx::query_scalar(include_str!("../queries/job_heartbeat.sql"))
        .bind(id)
        .bind(worker_id)
        .bind(progress)
        .fetch_optional(pool)
        .await?;
    Ok(match cancel {
        None => Beat::Lost,
        Some(true) => Beat::CancelRequested,
        Some(false) => Beat::Continue,
    })
}

/// Marks a job succeeded. `None` if this worker no longer owns it.
///
/// # Errors
/// Any database error.
pub async fn complete(
    pool: &PgPool,
    id: Uuid,
    worker_id: &str,
    result: &JsonValue,
) -> Result<Option<Job>, Error> {
    sqlx::query_as(include_str!("../queries/job_complete.sql"))
        .bind(id)
        .bind(worker_id)
        .bind(result)
        .fetch_optional(pool)
        .await
}

/// Records a failed attempt: requeued with backoff when `retryable` and
/// attempts remain, otherwise failed. `None` if this worker no longer owns it.
///
/// # Errors
/// Any database error.
pub async fn fail(
    pool: &PgPool,
    id: Uuid,
    worker_id: &str,
    error: &str,
    retryable: bool,
) -> Result<Option<Job>, Error> {
    sqlx::query_as(include_str!("../queries/job_fail.sql"))
        .bind(id)
        .bind(worker_id)
        .bind(error)
        .bind(retryable)
        .fetch_optional(pool)
        .await
}

/// Marks a running job cancelled after its worker stopped it.
///
/// # Errors
/// Any database error.
pub async fn mark_cancelled(pool: &PgPool, id: Uuid, worker_id: &str) -> Result<Option<Job>, Error> {
    sqlx::query_as(include_str!("../queries/job_mark_cancelled.sql"))
        .bind(id)
        .bind(worker_id)
        .fetch_optional(pool)
        .await
}

/// Requeues (or fails, or cancels) running jobs silent for `stale_after`.
/// Returns how many it handled.
///
/// # Errors
/// Any database error.
pub async fn requeue_stale(pool: &PgPool, stale_after: std::time::Duration) -> Result<i64, Error> {
    sqlx::query_scalar(include_str!("../queries/job_requeue_stale.sql"))
        .bind(stale_after.as_secs_f64())
        .fetch_one(pool)
        .await
}

/// A worker's registration, refreshed by [`worker_heartbeat`].
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// Unique per process.
    pub id: String,
    /// Host name.
    pub hostname: String,
    /// Engine version.
    pub version: String,
    /// Job kinds it runs.
    pub kinds: Vec<String>,
    /// Jobs it runs at once.
    pub concurrency: i32,
}

/// Registers or refreshes a worker.
///
/// # Errors
/// Any database error.
pub async fn worker_heartbeat(pool: &PgPool, w: &WorkerInfo) -> Result<(), Error> {
    sqlx::query(include_str!("../queries/worker_heartbeat.sql"))
        .bind(&w.id)
        .bind(&w.hostname)
        .bind(&w.version)
        .bind(&w.kinds)
        .bind(w.concurrency)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Removes a worker that shut down cleanly.
///
/// # Errors
/// Any database error.
pub async fn worker_remove(pool: &PgPool, id: &str) -> Result<(), Error> {
    sqlx::query(include_str!("../queries/worker_remove.sql")).bind(id).execute(pool).await.map(|_| ())
}

/// Workers with a heartbeat in the last `within`.
///
/// # Errors
/// Any database error.
pub async fn workers_alive(pool: &PgPool, within: std::time::Duration) -> Result<i64, Error> {
    sqlx::query_scalar(include_str!("../queries/workers_alive.sql"))
        .bind(within.as_secs_f64())
        .fetch_one(pool)
        .await
}

/// A finished run produced by a job.
#[derive(Debug, Clone)]
pub struct NewRun {
    /// UUIDv7.
    pub id: Uuid,
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// The job that produced it.
    pub job_id: Uuid,
    /// e.g. `benchmark`.
    pub kind: String,
    /// Inputs (large inputs replaced by their hash).
    pub params: JsonValue,
    /// Headline numbers.
    pub metrics: JsonValue,
    /// RNG seed, when there is one.
    pub seed: Option<i64>,
    /// Engine version.
    pub engine_version: String,
    /// Who queued the job.
    pub created_by: Option<Uuid>,
    /// When the job's attempt started.
    pub started_at: DateTime<Utc>,
}

/// Records a run.
///
/// # Errors
/// Any database error.
pub async fn insert_run(pool: &PgPool, r: &NewRun) -> Result<Uuid, Error> {
    sqlx::query_scalar(include_str!("../queries/run_insert.sql"))
        .bind(r.id)
        .bind(r.workspace_id)
        .bind(r.job_id)
        .bind(&r.kind)
        .bind(&r.params)
        .bind(&r.metrics)
        .bind(r.seed)
        .bind(&r.engine_version)
        .bind(r.created_by)
        .bind(r.started_at)
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for s in JobStatus::ALL {
            assert_eq!(JobStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(JobStatus::parse("paused"), None);
        assert!(JobStatus::Cancelled.is_terminal() && !JobStatus::Running.is_terminal());
    }
}
