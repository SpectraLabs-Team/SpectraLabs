//! `spectra-worker`: runs queued jobs (M20 step 6).
//!
//! Loop: claim the next job with `FOR UPDATE SKIP LOCKED`, run it on the
//! blocking pool while sending a heartbeat every few seconds, write the run
//! and its report artifact, then mark the job finished. Several workers can
//! run against one database. A job whose worker dies is requeued by whichever
//! worker next sweeps for stale heartbeats.
//!
//! Cancellation is checked at every heartbeat. Engine calls are not
//! interruptible, so a cancelled or timed-out computation is abandoned (its
//! result discarded) and its slot is freed only once it returns.
//!
//! Configuration: see [`spectra_api::config::WorkerConfig`]. Stops cleanly
//! on Ctrl-C or SIGTERM: no new claims, running jobs finish.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use spectra_api::config::WorkerConfig;
use spectra_api::{engine, observability, shutdown_signal};
use spectra_core::{Json, ENGINE_VERSION};
use spectra_db::artifacts::{store_for_run, ArtifactStore, NewArtifact};
use spectra_db::jobs::{self, Beat, Job, NewRun, WorkerInfo};
use spectra_db::{PgPool, PoolConfig};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;
use uuid::Uuid;
use tracing::{debug, error, info, warn};
use std::time::Instant;

/// Heartbeat interval for running jobs and for the worker itself.
const HEARTBEAT: Duration = Duration::from_secs(5);
/// A running job silent this long has lost its worker.
const STALE_AFTER: Duration = Duration::from_secs(30);
/// How often to sweep for stale jobs.
const SWEEP_EVERY: Duration = Duration::from_secs(15);
/// Wait between empty polls.
const IDLE_POLL: Duration = Duration::from_secs(1);

struct Ctx {
    pool: PgPool,
    store: ArtifactStore,
    worker_id: String,
    quota: i64,
}

#[tokio::main]
async fn main() -> ExitCode {
    observability::init_observability();
    let cfg = match WorkerConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("spectra-worker: {e}");
            return ExitCode::from(2);
        }
    };
    let pool = match spectra_db::connect(&PoolConfig {
        max_connections: (cfg.concurrency as u32) * 2 + 2,
        ..PoolConfig::new(cfg.database_url.expose_str())
    })
    .await
    {
        Ok(p) => p,
        Err(e) => {
            error!("cannot connect to the database: {e}");
            return ExitCode::from(1);
        }
    };

    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into());
    let worker_id =
        format!("{hostname}-{}-{}", std::process::id(), &Uuid::now_v7().simple().to_string()[20..]);
    let info = WorkerInfo {
        id: worker_id.clone(),
        hostname,
        version: ENGINE_VERSION.to_string(),
        kinds: cfg.kinds.clone(),
        concurrency: cfg.concurrency as i32,
    };
    if let Err(e) = jobs::worker_heartbeat(&pool, &info).await {
        error!("cannot register: {e}");
        return ExitCode::from(1);
    }
    info!(
        worker_id = %worker_id,
        kinds = %cfg.kinds.join(", "),
        concurrency = cfg.concurrency,
        workspace_max_running = cfg.workspace_max_running,
        artifact_dir = %cfg.artifact_dir.display(),
        "worker started"
    );

    let (stop_tx, mut stop_rx) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop_tx.send(true);
    });

    // Housekeeping: this worker's heartbeat, and requeueing jobs of dead workers.
    let (hk_pool, hk_info, mut hk_stop) = (pool.clone(), info.clone(), stop_rx.clone());
    let housekeeping = tokio::spawn(async move {
        let mut beat = tokio::time::interval(HEARTBEAT);
        let mut sweep = tokio::time::interval(SWEEP_EVERY);
        loop {
            tokio::select! {
                _ = hk_stop.changed() => break,
                _ = beat.tick() => {
                    if let Err(e) = jobs::worker_heartbeat(&hk_pool, &hk_info).await {
                        error!("heartbeat failed: {e}");
                    }
                }
                _ = sweep.tick() => match jobs::requeue_stale(&hk_pool, STALE_AFTER).await {
                    Ok(0) => {}
                    Ok(n) => info!("requeued {n} job(s) whose worker stopped responding"),
                    Err(e) => error!("stale sweep failed: {e}"),
                },
            }
        }
    });

    let ctx = Arc::new(Ctx {
        pool: pool.clone(),
        store: ArtifactStore::new(&cfg.artifact_dir),
        worker_id: worker_id.clone(),
        quota: cfg.artifact_quota_bytes,
    });
    let slots = Arc::new(Semaphore::new(cfg.concurrency));
    let mut running = JoinSet::new();

    loop {
        let permit = tokio::select! {
            _ = stop_rx.changed() => break,
            p = Arc::clone(&slots).acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => break,
            },
        };
        // Reap finished tasks so the set does not grow.
        while running.try_join_next().is_some() {}
        match jobs::claim(&pool, &worker_id, &cfg.kinds, cfg.workspace_max_running).await {
            Ok(Some(job)) => {
                info!(
                    job_id = %job.id,
                    job_kind = %job.kind,
                    attempts = job.attempts,
                    max_attempts = job.max_attempts,
                    "job claimed"
                );
                observability::JobMetrics::job_claimed(&job.kind);
                let _ = running.spawn(run_job(Arc::clone(&ctx), job, permit));
            }
            Ok(None) => {
                drop(permit);
                tokio::select! {
                    _ = stop_rx.changed() => break,
                    () = tokio::time::sleep(IDLE_POLL) => {}
                }
            }
            Err(e) => {
                drop(permit);
                error!("claim failed: {e}");
                tokio::select! {
                    _ = stop_rx.changed() => break,
                    () = tokio::time::sleep(IDLE_POLL * 3) => {}
                }
            }
        }
    }

    info!("stopping: no new jobs; waiting for running ones");
    while running.join_next().await.is_some() {}
    housekeeping.abort();
    let _ = jobs::worker_remove(&pool, &worker_id).await;
    pool.close().await;
    info!("stopped");
    ExitCode::SUCCESS
}

enum Outcome {
    Finished(engine::Reply),
    Panicked,
    Cancelled,
    TimedOut(Duration),
    Lost,
}

/// Runs one claimed job to a terminal state (or until another worker takes it over).
#[allow(clippy::needless_pass_by_value)] // the spawned task owns its job and permit
async fn run_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let job_start = Instant::now();
    let (kind, payload_text) = (job.kind.clone(), job.payload.to_string());
    let mut work = tokio::task::spawn_blocking(move || {
        let payload = Json::parse(&payload_text).map_err(|e| (422, format!("Invalid payload: {e}")))?;
        engine::run_job(&kind, &payload)
    });

    let timeout = engine::job_timeout(&job.kind);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await; // the first tick is immediate; the claim was the first heartbeat
    let outcome = loop {
        tokio::select! {
            res = &mut work => break match res {
                Ok(reply) => Outcome::Finished(reply),
                Err(_) => Outcome::Panicked,
            },
            _ = beat.tick() => match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, None).await {
                Ok(Beat::Continue) => {}
                Ok(Beat::CancelRequested) => break Outcome::Cancelled,
                Ok(Beat::Lost) => break Outcome::Lost,
                Err(e) => error!(job_id = %job.id, "heartbeat failed: {e}"),
            },
            () = tokio::time::sleep_until(deadline) => break Outcome::TimedOut(timeout),
        }
    };

    let job_duration = job_start.elapsed().as_secs_f64();

    // An abandoned computation keeps its slot until it actually returns, so a
    // burst of cancellations cannot oversubscribe the CPU.
    let finished = matches!(outcome, Outcome::Finished(_) | Outcome::Panicked);
    if finished {
        drop(permit);
    } else {
        tokio::spawn(async move {
            let _ = work.await;
            drop(permit);
        });
    }

    let pool = &ctx.pool;
    let wid = ctx.worker_id.as_str();
    let (status, res) = match outcome {
        Outcome::Finished(Ok(report)) => {
            observability::JobMetrics::job_completed(&job.kind, "succeeded");
            observability::JobMetrics::job_duration(&job.kind, "succeeded", job_duration);
            ("succeeded", finish(&ctx, &job, &report).await)
        }
        Outcome::Finished(Err((code, msg))) => {
            // 4xx: the input is wrong and will stay wrong. 5xx: worth retrying.
            let retryable = code >= 500;
            observability::JobMetrics::job_completed(&job.kind, if retryable { "failed_retryable" } else { "failed_permanent" });
            observability::JobMetrics::job_duration(&job.kind, if retryable { "failed_retryable" } else { "failed_permanent" }, job_duration);
            (if retryable { "failed_retryable" } else { "failed_permanent" }, jobs::fail(pool, job.id, wid, &msg, retryable).await.map(|_| ()))
        }
        Outcome::Panicked => {
            observability::JobMetrics::job_completed(&job.kind, "panicked");
            observability::JobMetrics::job_duration(&job.kind, "panicked", job_duration);
            ("panicked", jobs::fail(pool, job.id, wid, "The engine crashed while running this job.", true)
                .await
                .map(|_| ()))
        }
        Outcome::Cancelled => {
            observability::JobMetrics::job_completed(&job.kind, "cancelled");
            observability::JobMetrics::job_duration(&job.kind, "cancelled", job_duration);
            ("cancelled", jobs::mark_cancelled(pool, job.id, wid).await.map(|_| ()))
        }
        Outcome::TimedOut(t) => {
            let msg = format!("The job ran longer than its {} s limit.", t.as_secs());
            observability::JobMetrics::job_completed(&job.kind, "timeout");
            observability::JobMetrics::job_duration(&job.kind, "timeout", job_duration);
            ("timeout", jobs::fail(pool, job.id, wid, &msg, false).await.map(|_| ()))
        }
        Outcome::Lost => {
            warn!(job_id = %job.id, "job was taken over after a missed heartbeat; dropping it");
            observability::JobMetrics::job_completed(&job.kind, "lost");
            observability::JobMetrics::job_duration(&job.kind, "lost", job_duration);
            ("lost", Ok(()))
        }
    };
    if let Err(e) = res {
        error!(job_id = %job.id, status = status, "could not record the outcome: {e}");
    }
}

/// Records the run and its report artifact, then completes the job.
async fn finish(ctx: &Ctx, job: &Job, report: &Json) -> Result<(), spectra_db::Error> {
    let report_text = engine::to_text(report);
    let report_value: serde_json::Value =
        serde_json::from_str(&report_text).unwrap_or(serde_json::Value::Null);
    let payload = Json::parse(&job.payload.to_string()).unwrap_or(Json::Null);
    let params: serde_json::Value = serde_json::from_str(&engine::to_text(&engine::run_params(&payload)))
        .unwrap_or(serde_json::Value::Null);

    let run_id = jobs::insert_run(
        &ctx.pool,
        &NewRun {
            id: spectra_db::new_id(),
            workspace_id: job.workspace_id,
            job_id: job.id,
            kind: job.kind.clone(),
            params,
            metrics: report_value.clone(),
            seed: engine::job_seed(&job.kind, &payload),
            engine_version: ENGINE_VERSION.to_string(),
            created_by: job.created_by,
            started_at: job.started_at.unwrap_or_else(chrono::Utc::now),
        },
    )
    .await?;

    let artifact = NewArtifact {
        workspace_id: job.workspace_id,
        run_id,
        kind: "report",
        content_type: "application/json",
        bytes: report_text.as_bytes(),
    };
    let (artifact_json, artifact_error) =
        match store_for_run(&ctx.pool, &ctx.store, artifact, ctx.quota).await {
            Ok(s) => (
                serde_json::json!({ "sha256": s.sha256, "path": s.path, "size": s.size }),
                serde_json::Value::Null,
            ),
            Err(e) => {
                error!(job_id = %job.id, "report not stored: {e}");
                (serde_json::Value::Null, serde_json::Value::String(e.to_string()))
            }
        };
    let result = serde_json::json!({
        "run_id": run_id,
        "report": report_value,
        "artifact": artifact_json,
        "artifact_error": artifact_error,
    });
    if jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &result).await?.is_none() {
        log(&format!("{} finished, but another worker had taken it over", job.id));
    }
    Ok(())
}
