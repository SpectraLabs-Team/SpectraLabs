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
//!
//! With `SPECTRA_WORKER_METRICS_ADDR` set, the worker also serves `/health`
//! (for container health checks) and Prometheus `/metrics` (job counts and
//! durations, which only the worker process records).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use spectra_api::config::WorkerConfig;
use spectra_api::ingest::{self, IngestEnv, IngestOutcome};
use spectra_api::{engine, observability, shutdown_signal};
use spectra_core::{Json, ENGINE_VERSION};
use spectra_db::artifacts::{store_for_run, ArtifactBackend, NewArtifact};
use spectra_db::datasets::{self, Access, Resolved};
use spectra_db::jobs::{self, Beat, Job, NewRun, WorkerInfo};
use spectra_db::optimization;
use spectra_db::{PgPool, PoolConfig};
use spectra_engine::RunRecord;
use spectra_registry::{
    registry::{config_from_json, metrics_from_json, params_from_json, RunInput},
    PgRegistry, StoredArtifact,
};
use spectra_research::{run_survival_lab_controlled, SurvivalProgress, Verdict};
use sqlx::Row;
use std::time::Instant;
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Heartbeat interval for running jobs and for the worker itself.
const HEARTBEAT: Duration = Duration::from_secs(5);
/// A running job silent this long has lost its worker.
const STALE_AFTER: Duration = Duration::from_secs(30);
/// How often to sweep for stale jobs.
const SWEEP_EVERY: Duration = Duration::from_secs(15);
/// Wait between empty polls.
const IDLE_POLL: Duration = Duration::from_secs(1);
/// How often to remove uploads that were never ingested (M1 task 4).
const EXPIRE_UPLOADS_EVERY: Duration = Duration::from_secs(600);
/// Uploads removed per sweep.
const EXPIRE_UPLOADS_BATCH: i32 = 500;
/// How often to delete daily activity rows past the retention window (M21).
const ACTIVITY_PURGE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

struct Ctx {
    pool: PgPool,
    store: ArtifactBackend,
    worker_id: String,
    quota: i64,
    /// Dataset store that backtests read.
    data_dir: PathBuf,
    /// Size limits for one backtest.
    backtest_max_bars: usize,
    backtest_max_instruments: usize,
}

/// Deletes daily activity rows past the retention window and records the result: a log line,
/// Prometheus counters and a last-success timestamp. Failures are logged at error level, which
/// Sentry captures when `SENTRY_DSN` is set.
async fn purge_activity_now(pool: &PgPool) {
    use spectra_db::activity::{purge_activity, PurgeOutcome, ACTIVITY_RETENTION_DAYS};
    match purge_activity(pool, ACTIVITY_RETENTION_DAYS, false).await {
        Ok(PurgeOutcome::Done { rows, .. }) => {
            metrics::counter!("activity_purge_runs_total", "status" => "ok").increment(1);
            metrics::counter!("activity_purge_rows_total").increment(rows);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64());
            metrics::gauge!("activity_purge_last_success_timestamp_seconds").set(now);
            info!(
                rows,
                retention_days = ACTIVITY_RETENTION_DAYS,
                "activity purge removed {rows} daily activity row(s)"
            );
        }
        Ok(PurgeOutcome::Busy) => {
            metrics::counter!("activity_purge_runs_total", "status" => "busy").increment(1);
            info!("activity purge skipped: another purge is running");
        }
        Err(e) => {
            metrics::counter!("activity_purge_runs_total", "status" => "error").increment(1);
            error!("activity purge failed: {e}");
        }
    }
}

/// Prometheus text for the worker's own metrics.
#[allow(clippy::unused_async)] // axum handlers are async fns
async fn metrics_text() -> axum::response::Response {
    use axum::response::IntoResponse;
    match observability::render_metrics() {
        Some(text) => {
            ([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], text)
                .into_response()
        }
        None => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "metrics not initialised").into_response(),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    observability::init_observability();
    let cfg = match WorkerConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "invalid configuration");
            return ExitCode::from(2);
        }
    };
    let store = match spectra_api::config::artifact_backend(&cfg.artifact_storage) {
        Ok(s) => s,
        Err(e) => {
            error!("{e}");
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
        artifact_store = %store.describe(),
        data_dir = %cfg.data_dir.display(),
        "worker started"
    );

    let (stop_tx, mut stop_rx) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop_tx.send(true);
    });

    if let Some(addr) = cfg.metrics_addr {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("cannot listen for /metrics on {addr}: {e}");
                return ExitCode::from(1);
            }
        };
        info!(%addr, "serving /health and /metrics");
        let mut stop = stop_rx.clone();
        tokio::spawn(async move {
            let app = axum::Router::new()
                .route("/health", axum::routing::get(|| async { "ok" }))
                .route("/metrics", axum::routing::get(metrics_text));
            let shutdown = async move {
                let _ = stop.changed().await;
            };
            if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
                error!("metrics listener failed: {e}");
            }
        });
    }

    // Housekeeping: this worker's heartbeat, and requeueing jobs of dead workers.
    let (hk_pool, hk_info, mut hk_stop, hk_store) =
        (pool.clone(), info.clone(), stop_rx.clone(), store.clone());
    let activity_purge_on = cfg.activity_purge;
    let housekeeping = tokio::spawn(async move {
        let mut beat = tokio::time::interval(HEARTBEAT);
        let mut sweep = tokio::time::interval(SWEEP_EVERY);
        let mut expire = tokio::time::interval(EXPIRE_UPLOADS_EVERY);
        let mut sample = tokio::time::interval(Duration::from_secs(15));
        let mut refresh_sweep = tokio::time::interval(Duration::from_secs(30));
        let mut artifact_sweep = tokio::time::interval(Duration::from_secs(1800));
        let mut activity_purge = tokio::time::interval(ACTIVITY_PURGE_EVERY);
        loop {
            tokio::select! {
                _ = hk_stop.changed() => break,
                _ = beat.tick() => {
                    if let Err(e) = jobs::worker_heartbeat(&hk_pool, &hk_info).await {
                        error!("heartbeat failed: {e}");
                    }
                }
                _ = expire.tick() => expire_uploads(&hk_pool, &hk_store).await,
                _ = artifact_sweep.tick() => {
                    match spectra_db::artifacts::sweep_unreferenced(&hk_pool, &hk_store).await {
                        Ok(pruned) if !pruned.is_empty() => {
                            info!("garbage collected {} unreferenced artifact(s) from deleted workspaces", pruned.len());
                        }
                        Ok(_) => {}
                        Err(e) => warn!("unreferenced artifact sweep failed: {e}"),
                    }
                }
                _ = activity_purge.tick(), if activity_purge_on => purge_activity_now(&hk_pool).await,
                _ = refresh_sweep.tick() => {
                    spectra_api::providers::runner::sweep_due_refresh_schedules(&hk_pool).await;
                }
                _ = sweep.tick() => match jobs::requeue_stale(&hk_pool, STALE_AFTER).await {
                    Ok(0) => {}
                    Ok(n) => info!("requeued {n} job(s) whose worker stopped responding"),
                    Err(e) => error!("stale sweep failed: {e}"),
                },
                _ = sample.tick() => {
                    observability::DatabaseMetrics::pool_stats(hk_pool.size(), hk_pool.num_idle());
                    match jobs::queue_depth(&hk_pool).await {
                        Ok((queued, running)) => observability::DatabaseMetrics::queue_depth(queued, running),
                        Err(e) => warn!("could not sample job queue depth: {e}"),
                    }
                }
            }
        }
    });

    let ctx = Arc::new(Ctx {
        pool: pool.clone(),
        store,
        worker_id: worker_id.clone(),
        quota: cfg.artifact_quota_bytes,
        data_dir: cfg.data_dir.clone(),
        backtest_max_bars: cfg.backtest_max_bars,
        backtest_max_instruments: cfg.backtest_max_instruments,
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
    if job.kind == "survival" {
        run_survival_job(ctx, job, permit).await;
        return;
    }
    if job.kind == "sweep" {
        run_sweep_job(ctx, job, permit).await;
        return;
    }
    if job.kind == "walk_forward" {
        run_walk_forward_job(ctx, job, permit).await;
        return;
    }
    if job.kind == spectra_backtest::JOB_KIND {
        // If the job payload contains registry_run_id or csv, it is a legacy/registry backtest run
        if job.payload.get("registry_run_id").is_none() && job.payload.get("csv").is_none() {
            run_backtest_job(ctx, job, permit).await;
            return;
        }
    }
    if job.kind == engine::INGEST_JOB_KIND {
        run_ingest_job(ctx, job, permit).await;
        return;
    }
    if job.kind == spectra_api::providers::runner::PROVIDER_REFRESH_JOB_KIND {
        run_refresh_job(ctx, job, permit).await;
        return;
    }
    let job_start = Instant::now();
    if job.kind == "backtest" {
        let run_id = job
            .payload
            .get("registry_run_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok());
        let Some(run_id) = run_id else {
            let _ = jobs::fail(
                &ctx.pool,
                job.id,
                &ctx.worker_id,
                "Backtest job has no valid registry run id.",
                false,
            )
            .await;
            return;
        };
        let experiment_id = job
            .payload
            .get("experiment_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok());
        let Some(experiment_id) = experiment_id else {
            let _ = jobs::fail(
                &ctx.pool,
                job.id,
                &ctx.worker_id,
                "Backtest job has no valid experiment id.",
                false,
            )
            .await;
            let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
            let _ = registry.fail_run(run_id, "Backtest job has no valid experiment id.").await;
            return;
        };
        let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
        if let Err(error) = registry.start_run(run_id).await {
            error!(job_id = %job.id, run_id = %run_id, error = %error, "could not start registry run");
            let _ = jobs::fail(&ctx.pool, job.id, &ctx.worker_id, "Could not start the registry run.", false)
                .await;
            return;
        }
        let _ = experiment_id;
    }
    let (kind, payload_text) = (job.kind.clone(), job.payload.to_string());
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut work = tokio::task::spawn_blocking(move || {
        let payload = Json::parse(&payload_text).map_err(|e| (422, format!("Invalid payload: {e}")))?;
        engine::run_job_with_progress(&kind, &payload, |progress| {
            let _ = progress_tx.send(progress);
        })
    });

    let timeout = engine::job_timeout(&job.kind);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut beat = tokio::time::interval(HEARTBEAT);
    let mut progress_open = true;
    beat.tick().await; // the first tick is immediate; the claim was the first heartbeat
    let outcome = loop {
        tokio::select! {
            res = &mut work => break match res {
                Ok(reply) => Outcome::Finished(reply),
                Err(_) => Outcome::Panicked,
            },
            progress = progress_rx.recv(), if progress_open => match progress {
                Some(progress) => match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, Some(progress)).await {
                    Ok(Beat::Continue) => {}
                    Ok(Beat::CancelRequested) => break Outcome::Cancelled,
                    Ok(Beat::Lost) => break Outcome::Lost,
                    Err(e) => error!(job_id = %job.id, "progress heartbeat failed: {e}"),
                },
                None => progress_open = false,
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
            observability::JobMetrics::job_completed(
                &job.kind,
                if retryable { "failed_retryable" } else { "failed_permanent" },
            );
            observability::JobMetrics::job_duration(
                &job.kind,
                if retryable { "failed_retryable" } else { "failed_permanent" },
                job_duration,
            );
            (
                if retryable { "failed_retryable" } else { "failed_permanent" },
                async {
                    let failed = jobs::fail(pool, job.id, wid, &msg, retryable).await?;
                    if job.kind == "backtest" && failed.as_ref().is_some_and(|j| j.status == "failed") {
                        if let Some(id) = job.payload.get("registry_run_id").and_then(serde_json::Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) {
                            let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
                            if let Err(error) = registry.fail_run(id, &msg).await {
                                error!(job_id = %job.id, run_id = %id, error = %error, "could not fail registry run");
                            }
                        }
                    }
                    Ok(())
                }.await,
            )
        }
        Outcome::Panicked => {
            observability::JobMetrics::job_completed(&job.kind, "panicked");
            observability::JobMetrics::job_duration(&job.kind, "panicked", job_duration);
            let message = "The engine crashed while running this job.";
            ("panicked", async {
                let failed = jobs::fail(pool, job.id, wid, message, true).await?;
                if job.kind == "backtest" && failed.as_ref().is_some_and(|j| j.status == "failed") {
                    if let Some(id) = job.payload.get("registry_run_id").and_then(serde_json::Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) {
                        let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
                        if let Err(error) = registry.fail_run(id, message).await {
                            error!(job_id = %job.id, run_id = %id, error = %error, "could not fail panicked registry run");
                        }
                    }
                }
                Ok(())
            }.await)
        }
        Outcome::Cancelled => {
            observability::JobMetrics::job_completed(&job.kind, "cancelled");
            observability::JobMetrics::job_duration(&job.kind, "cancelled", job_duration);
            ("cancelled", async {
                jobs::mark_cancelled(pool, job.id, wid).await?;
                if job.kind == "backtest" {
                    if let Some(id) = job.payload.get("registry_run_id").and_then(serde_json::Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) {
                        let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
                        if let Err(error) = registry.cancel_run(id).await {
                            error!(job_id = %job.id, run_id = %id, error = %error, "could not cancel registry run");
                        }
                    }
                }
                Ok(())
            }.await)
        }
        Outcome::TimedOut(t) => {
            let msg = format!("The job ran longer than its {} s limit.", t.as_secs());
            observability::JobMetrics::job_completed(&job.kind, "timeout");
            observability::JobMetrics::job_duration(&job.kind, "timeout", job_duration);
            ("timeout", async {
                jobs::fail(pool, job.id, wid, &msg, false).await?;
                if job.kind == "backtest" {
                    if let Some(id) = job.payload.get("registry_run_id").and_then(serde_json::Value::as_str).and_then(|s| Uuid::parse_str(s).ok()) {
                        let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
                        if let Err(error) = registry.fail_run(id, &msg).await {
                            error!(job_id = %job.id, run_id = %id, error = %error, "could not fail timed-out registry run");
                        }
                    }
                }
                Ok(())
            }.await)
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
    if job.kind == "backtest" {
        return finish_registry_backtest(ctx, job, report)
            .await
            .map_err(|error| sqlx::Error::Protocol(error.to_string()));
    }
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
        warn!(job_id = %job.id, "job finished, but another worker had taken it over");
    }
    Ok(())
}

async fn finish_registry_backtest(
    ctx: &Ctx,
    job: &Job,
    report: &Json,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parse_string =
        |object: &Json, key: &str| -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok(object.at(key)?.as_str()?.to_owned())
        };
    let run_id = job
        .payload
        .get("registry_run_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or("missing registry run id")?;
    let run = report.at("run")?;
    let params = params_from_json(report.at("params")?)?;
    let config = config_from_json(report.at("config")?)?;
    let metrics = metrics_from_json(report.at("metrics")?)?;
    let returns = report
        .at("returns")?
        .items()?
        .iter()
        .map(spectra_core::Json::as_double)
        .collect::<Result<Vec<_>, _>>()?;
    let input = RunInput {
        experiment_id: parse_string(report, "experiment_id")?,
        kind: "backtest".into(),
        dataset_id: parse_string(report, "dataset_id")?,
        strategy: parse_string(report, "strategy")?,
        strategy_sha256: parse_string(report, "strategy_sha256")?,
        params,
        config,
        run: RunRecord {
            engine_version: parse_string(run, "engine_version")?,
            dataset_sha256: parse_string(run, "dataset_sha256")?,
            config_sha256: parse_string(run, "config_sha256")?,
            result_sha256: parse_string(run, "result_sha256")?,
            seed: parse_string(run, "seed")?.parse()?,
            bar_count: usize::try_from(run.at("bar_count")?.as_int()?)?,
        },
        metrics,
        returns,
        counts_as_trial: true,
        note: parse_string(report, "note")?,
    };
    let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
    // The series files are stored on their own; the report keeps the summary only.
    let files = report.at(engine::BACKTEST_ARTIFACTS_KEY)?;
    let summary_report = spectra_core::Json::Object(
        report
            .entries()?
            .iter()
            .filter(|(key, _)| key != engine::BACKTEST_ARTIFACTS_KEY && key != "returns")
            .cloned()
            .collect(),
    );
    let report_text = engine::to_text(&summary_report);
    let report_value: serde_json::Value = serde_json::from_str(&report_text)?;
    let mut uploads: Vec<(&str, &str, Vec<u8>)> =
        vec![("report", "application/json", report_text.as_bytes().to_vec())];
    for kind in ["equity_curve", "trades", "returns"] {
        uploads.push((kind, "text/csv", files.at(kind)?.as_str()?.as_bytes().to_vec()));
    }
    let mut stored_artifacts = Vec::with_capacity(uploads.len());
    let mut report_stored = None;
    for (kind, content_type, bytes) in &uploads {
        let artifact = NewArtifact { workspace_id: job.workspace_id, run_id, kind, content_type, bytes };
        let stored = store_for_run(&ctx.pool, &ctx.store, artifact, ctx.quota).await?;
        // CSV rows below the header line.
        let rows = bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()).count().saturating_sub(1);
        let summary = if *kind == "report" {
            spectra_core::Json::object()
        } else {
            spectra_core::Json::object().with("rows", spectra_core::Json::integer(i64::try_from(rows)?))
        };
        stored_artifacts.push(StoredArtifact {
            kind: (*kind).to_owned(),
            sha256: stored.sha256.clone(),
            uri: stored.path.clone(),
            size_bytes: stored.size,
            content_type: (*content_type).to_owned(),
            summary,
        });
        if *kind == "report" {
            report_stored = Some(stored);
        }
    }
    let stored = report_stored.ok_or("report artifact was not stored")?;
    let registry_run_id = registry.complete_run_with_artifacts(run_id, &input, &stored_artifacts).await?;
    let result = serde_json::json!({
        "run_id": registry_run_id,
        "report": report_value,
        "artifact": { "sha256": stored.sha256, "path": stored.path, "size": stored.size },
    });
    if jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &result).await?.is_none() {
        warn!(job_id = %job.id, "registry backtest finished, but another worker had taken it over");
    }
    Ok(())
}

/// Records a job's terminal status in the metrics.
fn record_metrics(kind: &str, status: &str, started: Instant) {
    observability::JobMetrics::job_completed(kind, status);
    observability::JobMetrics::job_duration(kind, status, started.elapsed().as_secs_f64());
}

/// Fails a job (retryable failures go back to the queue) and records the metrics.
async fn fail_job(ctx: &Ctx, job: &Job, started: Instant, message: &str, retryable: bool) {
    record_metrics(&job.kind, if retryable { "failed_retryable" } else { "failed_permanent" }, started);
    if let Err(e) = jobs::fail(&ctx.pool, job.id, &ctx.worker_id, message, retryable).await {
        error!(job_id = %job.id, "could not record the failure: {e}");
    }
}

/// A `backtest` job (M3 task 5).
#[allow(clippy::needless_pass_by_value)] // the spawned task owns its job and permit
async fn run_backtest_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let env = spectra_api::worker::BacktestWorkerEnv {
        pool: ctx.pool.clone(),
        store: ctx.store.clone(),
        data_dir: ctx.data_dir.clone(),
        quota: ctx.quota,
        backtest_max_bars: ctx.backtest_max_bars,
        backtest_max_instruments: ctx.backtest_max_instruments,
    };
    let _ = spectra_api::worker::run_backtest_job(&env, &job, &ctx.worker_id).await;
    drop(permit);
}

/// Removes uploads that were never ingested within their 24 hours, and the
/// stored files nothing else uses any more.
async fn expire_uploads(pool: &PgPool, store: &ArtifactBackend) {
    match spectra_db::uploads::expire(pool, EXPIRE_UPLOADS_BATCH).await {
        Ok(orphans) => {
            for sha256 in &orphans {
                if let Err(e) = store.delete(sha256).await {
                    warn!(%sha256, "could not delete an expired upload's file: {e}");
                }
            }
            if !orphans.is_empty() {
                info!("removed {} expired upload file(s)", orphans.len());
            }
        }
        Err(e) => error!("upload expiry sweep failed: {e}"),
    }
}

/// An `ingest` job (M1 task 5): an upload becomes a dataset version. Progress
/// (rows read) goes out with each heartbeat; a cancel request stops it.
async fn run_ingest_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = Instant::now();
    let env = IngestEnv {
        pool: ctx.pool.clone(),
        store: ctx.store.clone(),
        data_dir: ctx.data_dir.clone(),
        quota: ctx.quota,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
    let mut work = {
        let (job, cancel, progress) = (job.clone(), Arc::clone(&cancel), Arc::clone(&progress));
        tokio::spawn(async move { ingest::run_ingest(&env, &job, cancel, progress).await })
    };
    let deadline = tokio::time::Instant::now() + engine::job_timeout(&job.kind);
    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await; // the first tick is immediate; the claim was the first heartbeat
    let (mut cancel_requested, mut lost) = (false, false);
    let outcome = loop {
        tokio::select! {
            res = &mut work => break res.ok(),
            _ = beat.tick() => {
                let p = f32::from_bits(progress.load(Ordering::Relaxed));
                match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, Some(p)).await {
                    Ok(Beat::Continue) => {}
                    Ok(Beat::CancelRequested) => {
                        cancel_requested = true;
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(Beat::Lost) => {
                        lost = true;
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Err(e) => error!(job_id = %job.id, "heartbeat failed: {e}"),
                }
            },
            () = tokio::time::sleep_until(deadline) => {
                cancel.store(true, Ordering::Relaxed);
                work.abort();
                break Some(IngestOutcome::Failed {
                    message: "The ingest ran longer than its limit.".into(),
                    retryable: false,
                });
            }
        }
    };
    drop(permit);
    if lost {
        warn!(job_id = %job.id, "job was taken over after a missed heartbeat; dropping it");
        record_metrics(&job.kind, "lost", started);
        return;
    }
    match outcome {
        Some(IngestOutcome::Registered(result)) => {
            match jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &result).await {
                Ok(Some(_)) => {
                    record_metrics(&job.kind, "succeeded", started);
                    info!(job_id = %job.id, version = %result["dataset_version_id"], "dataset version registered");
                }
                Ok(None) => warn!(job_id = %job.id, "ingest finished, but another worker had taken it over"),
                Err(e) => error!(job_id = %job.id, "could not record the ingest: {e}"),
            }
        }
        Some(IngestOutcome::Rejected { message, result }) => {
            record_metrics(&job.kind, "failed_permanent", started);
            if let Err(e) = jobs::reject(&ctx.pool, job.id, &ctx.worker_id, &message, &result).await {
                error!(job_id = %job.id, "could not record the rejection: {e}");
            }
        }
        Some(IngestOutcome::Cancelled) => {
            if !cancel_requested {
                warn!(job_id = %job.id, "ingest stopped without a cancel request");
            }
            record_metrics(&job.kind, "cancelled", started);
            if let Err(e) = jobs::mark_cancelled(&ctx.pool, job.id, &ctx.worker_id).await {
                error!(job_id = %job.id, "could not record the cancellation: {e}");
            }
        }
        Some(IngestOutcome::Failed { message, retryable }) => {
            fail_job(&ctx, &job, started, &message, retryable).await;
        }
        None => fail_job(&ctx, &job, started, "The ingest crashed.", true).await,
    }
}
async fn run_refresh_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = Instant::now();
    let env = spectra_api::providers::runner::RefreshEnv {
        pool: ctx.pool.clone(),
        store: ctx.store.clone(),
        data_dir: ctx.data_dir.clone(),
        quota: ctx.quota,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
    let mut work = {
        let (job, cancel, progress) = (job.clone(), Arc::clone(&cancel), Arc::clone(&progress));
        tokio::spawn(async move {
            spectra_api::providers::runner::run_refresh(&env, &job, cancel, progress).await
        })
    };
    let deadline = tokio::time::Instant::now() + engine::job_timeout(&job.kind);
    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await;
    let (mut cancel_requested, mut lost) = (false, false);
    let outcome = loop {
        tokio::select! {
            res = &mut work => break res.ok(),
            _ = beat.tick() => {
                let p = f32::from_bits(progress.load(Ordering::Relaxed));
                match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, Some(p)).await {
                    Ok(Beat::Continue) => {}
                    Ok(Beat::CancelRequested) => {
                        cancel_requested = true;
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(Beat::Lost) => {
                        lost = true;
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Err(e) => error!(job_id = %job.id, "heartbeat failed: {e}"),
                }
            },
            () = tokio::time::sleep_until(deadline) => {
                cancel.store(true, Ordering::Relaxed);
                work.abort();
                break Some(spectra_api::providers::runner::RefreshOutcome::Failed {
                    message: "The refresh job ran longer than its limit.".into(),
                    retryable: false,
                });
            }
        }
    };
    drop(permit);
    if lost {
        warn!(job_id = %job.id, "job was taken over after a missed heartbeat; dropping it");
        record_metrics(&job.kind, "lost", started);
        return;
    }
    match outcome {
        Some(spectra_api::providers::runner::RefreshOutcome::Registered(result)) => {
            match jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &result).await {
                Ok(Some(_)) => {
                    record_metrics(&job.kind, "succeeded", started);
                    info!(
                        job_id = %job.id,
                        version = %result.get("dataset_version_id").unwrap_or(&serde_json::Value::Null),
                        "dataset refresh version registered"
                    );
                }
                Ok(None) => warn!(job_id = %job.id, "refresh finished, but another worker had taken it over"),
                Err(e) => error!(job_id = %job.id, "could not record the refresh: {e}"),
            }
        }
        Some(spectra_api::providers::runner::RefreshOutcome::Cancelled) => {
            if !cancel_requested {
                warn!(job_id = %job.id, "refresh stopped without a cancel request");
            }
            record_metrics(&job.kind, "cancelled", started);
            if let Err(e) = jobs::mark_cancelled(&ctx.pool, job.id, &ctx.worker_id).await {
                error!(job_id = %job.id, "could not record the cancellation: {e}");
            }
        }
        Some(spectra_api::providers::runner::RefreshOutcome::Failed { message, retryable }) => {
            fail_job(&ctx, &job, started, &message, retryable).await;
        }
        None => fail_job(&ctx, &job, started, "The refresh job crashed.", true).await,
    }
}

/// Runs a Strategy Survival Lab report against the immutable inputs of a successful run.
#[allow(clippy::needless_pass_by_value)]
async fn run_survival_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = Instant::now();
    let parse_uuid = |key: &str| {
        job.payload.get(key).and_then(serde_json::Value::as_str).and_then(|v| Uuid::parse_str(v).ok())
    };
    let (Some(report_id), Some(run_id)) = (parse_uuid("report_id"), parse_uuid("run_id")) else {
        fail_job(&ctx, &job, started, "Survival job has invalid report or run id.", false).await;
        drop(permit);
        return;
    };
    let config_value = job.payload.get("config").cloned().unwrap_or(serde_json::Value::Null);
    let report_cfg = match spectra_research::SurvivalConfig::from_json(
        &Json::parse(&config_value.to_string()).unwrap_or(Json::Null),
    ) {
        Ok(config) => config,
        Err(error) => {
            let message = format!("Invalid survival config: {error}");
            let _ = sqlx::query("UPDATE survival_reports SET status='failed', error=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id).bind(report_id).bind(&message).execute(&ctx.pool).await;
            fail_job(&ctx, &job, started, &message, false).await;
            drop(permit);
            return;
        }
    };
    let row = match sqlx::query("SELECT r.strategy, r.params, r.config, j.payload FROM runs r JOIN jobs j ON j.id = r.job_id WHERE r.workspace_id = $1 AND r.id = $2 AND r.status = 'succeeded'")
        .bind(job.workspace_id).bind(run_id).fetch_optional(&ctx.pool).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            let message = "The source run is no longer available.";
            let _ = sqlx::query("UPDATE survival_reports SET status='failed', error=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id).bind(report_id).bind(message).execute(&ctx.pool).await;
            fail_job(&ctx, &job, started, message, false).await;
            drop(permit);
            return;
        }
        Err(error) => {
            let message = format!("Could not load source run: {error}");
            fail_job(&ctx, &job, started, &message, true).await;
            drop(permit);
            return;
        }
    };
    let original_payload: serde_json::Value = match row.try_get("payload") {
        Ok(value) => value,
        Err(error) => {
            fail_job(&ctx, &job, started, &format!("Could not load run inputs: {error}"), true).await;
            drop(permit);
            return;
        }
    };
    let strategy_from_run: String = row.try_get("strategy").unwrap_or_default();
    let strategy = if strategy_from_run.is_empty() {
        original_payload.get("strategy").and_then(serde_json::Value::as_str).unwrap_or("").to_owned()
    } else {
        strategy_from_run
    };
    let params_value = if row
        .try_get::<serde_json::Value, _>("params")
        .is_ok_and(|v| v != serde_json::json!({}))
    {
        row.try_get::<serde_json::Value, _>("params").unwrap_or_default()
    } else {
        original_payload.get("params").cloned().unwrap_or_else(|| {
            serde_json::json!({ "fast": original_payload.get("fast").and_then(serde_json::Value::as_f64).unwrap_or(5.0),
                "slow": original_payload.get("slow").and_then(serde_json::Value::as_f64).unwrap_or(20.0) })
        })
    };
    let config_value_run = row
        .try_get::<serde_json::Value, _>("config")
        .ok()
        .filter(|v| *v != serde_json::json!({}))
        .or_else(|| original_payload.get("config").cloned())
        .unwrap_or_else(|| serde_json::json!({}));
    let raw_params = Json::parse(&params_value.to_string()).unwrap_or(Json::Null);
    let params = match params_from_json(&raw_params) {
        Ok(v) => v,
        Err(error) => {
            let message = format!("Source run parameters are invalid: {error}");
            fail_job(&ctx, &job, started, &message, false).await;
            drop(permit);
            return;
        }
    };
    let backtest_config =
        match config_from_json(&Json::parse(&config_value_run.to_string()).unwrap_or(Json::Null)) {
            Ok(v) => v,
            Err(error) => {
                let message = format!("Source run config is invalid: {error}");
                fail_job(&ctx, &job, started, &message, false).await;
                drop(permit);
                return;
            }
        };
    let bars = if let Some(csv) = original_payload.get("csv").and_then(serde_json::Value::as_str) {
        match spectra_data::load_csv(csv.as_bytes(), &spectra_data::CsvOptions::default()) {
            Ok(dataset) => dataset.bars,
            Err(error) => {
                let message = format!("Could not reload source dataset: {error}");
                fail_job(&ctx, &job, started, &message, false).await;
                drop(permit);
                return;
            }
        }
    } else if let Some(version) = original_payload
        .get("dataset_version_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|v| Uuid::parse_str(v).ok())
    {
        let version = match datasets::resolve_version(
            &ctx.pool,
            job.workspace_id,
            version,
            Access::engine("worker-survival"),
        )
        .await
        {
            Ok(Resolved::Found(v)) => v,
            Ok(Resolved::Sealed { holdout }) => {
                let message = format!("The dataset version is sealed as holdout '{holdout}'; only a holdout evaluation may read it.");
                fail_job(&ctx, &job, started, &message, false).await;
                drop(permit);
                return;
            }
            Ok(Resolved::Missing) => {
                fail_job(&ctx, &job, started, "The source dataset version no longer exists.", false).await;
                drop(permit);
                return;
            }
            Err(error) => {
                fail_job(&ctx, &job, started, &format!("Could not load the source dataset: {error}"), true)
                    .await;
                drop(permit);
                return;
            }
        };
        if let Err((message, retryable)) =
            ingest::ensure_local(&ctx.store, &ctx.data_dir, &version.checksum, version.blob_sha256.as_deref())
                .await
        {
            fail_job(&ctx, &job, started, &message, retryable).await;
            drop(permit);
            return;
        }
        match spectra_backtest::load_bars(&ctx.data_dir, &version.checksum) {
            Ok(bars) => bars,
            Err(error) => {
                fail_job(&ctx, &job, started, &format!("Could not read the source bars: {error}"), true)
                    .await;
                drop(permit);
                return;
            }
        }
    } else {
        fail_job(&ctx, &job, started, "The source run does not identify reloadable dataset inputs.", false)
            .await;
        drop(permit);
        return;
    };
    if let Err(error) = sqlx::query(
        "UPDATE survival_reports SET status='running' WHERE workspace_id=$1 AND id=$2 AND status='queued'",
    )
    .bind(job.workspace_id)
    .bind(report_id)
    .execute(&ctx.pool)
    .await
    {
        fail_job(&ctx, &job, started, &format!("Could not start survival report: {error}"), true).await;
        drop(permit);
        return;
    }
    let factory = match spectra_backtest::builtin_factory(&strategy) {
        Ok(factory) => factory,
        Err(error) => {
            let message = format!("Unsupported source strategy: {error}");
            let _ = sqlx::query("UPDATE survival_reports SET status='failed', error=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id).bind(report_id).bind(&message).execute(&ctx.pool).await;
            fail_job(&ctx, &job, started, &message, false).await;
            drop(permit);
            return;
        }
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
    let mut work = {
        let (cancel, progress, bars, config, params, factory) =
            (Arc::clone(&cancel), Arc::clone(&progress), bars, backtest_config, params, factory);
        tokio::task::spawn_blocking(move || {
            let mut update = |p: SurvivalProgress| {
                let done = (f32::from(p.test_number.saturating_sub(1))
                    + p.step as f32 / p.total_steps.max(1) as f32)
                    / 9.0;
                progress.store(done.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            };
            let report = run_survival_lab_controlled(
                &bars,
                &factory,
                &config,
                &params,
                &report_cfg,
                Some(&cancel),
                Some(&mut update),
            )
            .map_err(|e| e.to_string())?;
            let json = report.to_json().map_err(|e| e.to_string())?;
            let encoded = engine::to_text(&json);
            let parsed: serde_json::Value = serde_json::from_str(&encoded).map_err(|e| e.to_string())?;
            let text = spectra_research::format_survival_report(&report);
            let verdicts =
                ["costs", "delay", "stability", "time", "trades", "sample", "regimes", "benchmark", "risk"]
                    .into_iter()
                    .filter_map(|key| {
                        parsed.get(key).and_then(|v| v.get("verdict")).map(|v| (key.to_owned(), v.clone()))
                    })
                    .collect::<serde_json::Map<_, _>>();
            let worst = match report.worst {
                Verdict::Pass => "pass",
                Verdict::Warn => "warn",
                Verdict::Insufficient => "insufficient",
                Verdict::Fail => "fail",
            }
            .to_owned();
            Ok::<_, String>((parsed, serde_json::Value::Object(verdicts), text, worst))
        })
    };
    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await;
    let mut cancel_requested = false;
    let result = loop {
        tokio::select! {
            work_result = &mut work => break match work_result { Ok(result) => result, Err(error) => Err(format!("Survival worker panicked: {error}")) },
            _ = beat.tick() => match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, Some(f32::from_bits(progress.load(Ordering::Relaxed)))).await {
                Ok(Beat::Continue) => {},
                Ok(Beat::CancelRequested) => { cancel_requested = true; cancel.store(true, Ordering::Relaxed); },
                Ok(Beat::Lost) => { cancel.store(true, Ordering::Relaxed); record_metrics(&job.kind, "lost", started); drop(permit); return; },
                Err(error) => warn!(job_id = %job.id, "survival heartbeat failed: {error}"),
            }
        }
    };
    drop(permit);
    match result {
        Ok((results, verdicts, text, worst)) => {
            let text_artifact = NewArtifact {
                workspace_id: job.workspace_id,
                run_id,
                kind: "survival_text",
                content_type: "text/plain; charset=utf-8",
                bytes: text.as_bytes(),
            };
            let stored_text = match store_for_run(&ctx.pool, &ctx.store, text_artifact, ctx.quota).await {
                Ok(stored) => stored,
                Err(error) => {
                    let message = format!("Could not store the survival text artifact: {error}");
                    let _ = sqlx::query("UPDATE survival_reports SET status='failed', error=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                        .bind(job.workspace_id).bind(report_id).bind(&message).execute(&ctx.pool).await;
                    fail_job(&ctx, &job, started, &message, true).await;
                    return;
                }
            };
            let saved = sqlx::query("UPDATE survival_reports SET status='succeeded', results=$3, verdicts=$4, worst_verdict=$5, text_sha256=$6, text_path=$7, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id).bind(report_id).bind(&results).bind(&verdicts).bind(&worst).bind(&stored_text.sha256).bind(&stored_text.path).execute(&ctx.pool).await;
            if let Err(error) = saved {
                fail_job(&ctx, &job, started, &format!("Could not save survival results: {error}"), true)
                    .await;
                return;
            }
            let registry = PgRegistry::new(ctx.pool.clone(), job.workspace_id);
            if let Err(error) = registry.record_survival_report_saved(report_id, run_id, &worst).await {
                error!(job_id = %job.id, report_id = %report_id, "could not append survival audit event: {error}");
            }
            let job_result =
                serde_json::json!({ "report_id": report_id, "run_id": run_id, "worst_verdict": worst });
            if let Err(error) = jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &job_result).await {
                error!(job_id = %job.id, "could not complete survival job: {error}");
            }
            record_metrics(&job.kind, "succeeded", started);
            info!(job_id = %job.id, report_id = %report_id, "survival report saved");
        }
        Err(message) if cancel_requested => {
            let _ = sqlx::query("UPDATE survival_reports SET status='cancelled', error=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id).bind(report_id).bind(&message).execute(&ctx.pool).await;
            record_metrics(&job.kind, "cancelled", started);
            if let Err(error) = jobs::mark_cancelled(&ctx.pool, job.id, &ctx.worker_id).await {
                error!(job_id = %job.id, "could not record survival cancellation: {error}");
            }
        }
        Err(message) => {
            let _ = sqlx::query("UPDATE survival_reports SET status='failed', error=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id).bind(report_id).bind(&message).execute(&ctx.pool).await;
            fail_job(&ctx, &job, started, &message, false).await;
        }
    }
}

fn parse_grid_from_json(val: &serde_json::Value) -> Result<spectra_research::ParamGrid, String> {
    let mut grid = spectra_research::ParamGrid::new();
    if let Some(obj) = val.as_object() {
        for (k, v) in obj {
            let mut nums = Vec::new();
            if let Some(arr) = v.as_array() {
                for item in arr {
                    if let Some(n) = item.as_f64() {
                        nums.push(n);
                    }
                }
            } else if let Some(n) = v.as_f64() {
                nums.push(n);
            }
            if !nums.is_empty() {
                grid = grid.add(k.clone(), nums).map_err(|e| e.to_string())?;
            }
        }
    }
    if grid.size() == 0 {
        grid = grid.add("fast", vec![5.0, 10.0, 15.0]).map_err(|e| e.to_string())?;
        grid = grid.add("slow", vec![20.0, 30.0, 40.0]).map_err(|e| e.to_string())?;
    }
    Ok(grid)
}

/// Runs a parameter sweep job.
#[allow(clippy::needless_pass_by_value)]
async fn run_sweep_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = Instant::now();
    let Some(sweep_id) = job
        .payload
        .get("sweep_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        fail_job(&ctx, &job, started, "Sweep job missing sweep_id.", false).await;
        drop(permit);
        return;
    };
    let Some(version_id) = job
        .payload
        .get("dataset_version_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        fail_job(&ctx, &job, started, "Sweep job missing dataset_version_id.", false).await;
        drop(permit);
        return;
    };
    let strategy_version_id = job
        .payload
        .get("strategy_version_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok());
    let experiment_id = job
        .payload
        .get("experiment_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok());

    let version = match datasets::resolve_version(
        &ctx.pool,
        job.workspace_id,
        version_id,
        Access::engine("worker-sweep"),
    )
    .await
    {
        Ok(Resolved::Found(v)) => v,
        Ok(Resolved::Sealed { holdout }) => {
            let message = format!("The dataset version is sealed as holdout '{holdout}'; only a holdout evaluation may read it.");
            fail_job(&ctx, &job, started, &message, false).await;
            drop(permit);
            return;
        }
        Ok(Resolved::Missing) => {
            fail_job(&ctx, &job, started, "Dataset version no longer exists in workspace.", false).await;
            drop(permit);
            return;
        }
        Err(e) => {
            fail_job(&ctx, &job, started, &format!("Could not look up dataset version: {e}"), true).await;
            drop(permit);
            return;
        }
    };

    if let Err((msg, retryable)) =
        ingest::ensure_local(&ctx.store, &ctx.data_dir, &version.checksum, version.blob_sha256.as_deref())
            .await
    {
        fail_job(&ctx, &job, started, &msg, retryable).await;
        drop(permit);
        return;
    }

    let bars = match spectra_backtest::load_bars(&ctx.data_dir, &version.checksum) {
        Ok(b) => b,
        Err(e) => {
            fail_job(&ctx, &job, started, &format!("Could not read dataset bars: {e}"), true).await;
            drop(permit);
            return;
        }
    };

    let _ = sqlx::query(
        "UPDATE optimization_sweeps SET status='running' WHERE workspace_id=$1 AND id=$2 AND status='queued'",
    )
    .bind(job.workspace_id)
    .bind(sweep_id)
    .execute(&ctx.pool)
    .await;

    let param_grid_val = job.payload.get("param_grid").cloned().unwrap_or_else(|| serde_json::json!({}));
    let method = job.payload.get("method").and_then(serde_json::Value::as_str).unwrap_or("grid");
    let max_trials = job.payload.get("max_trials").and_then(serde_json::Value::as_u64).map(|n| n as usize);
    let seed = job.payload.get("seed").and_then(serde_json::Value::as_u64).unwrap_or(42);
    let objective =
        job.payload.get("objective").and_then(serde_json::Value::as_str).unwrap_or("sharpe").to_string();
    let dsl_code = job.payload.get("dsl_code").and_then(serde_json::Value::as_str).map(ToOwned::to_owned);
    let strategy =
        job.payload.get("strategy").and_then(serde_json::Value::as_str).unwrap_or("sma_cross").to_string();

    let grid = match parse_grid_from_json(&param_grid_val) {
        Ok(g) => g,
        Err(e) => {
            fail_job(&ctx, &job, started, &format!("Invalid parameter grid: {e}"), false).await;
            drop(permit);
            return;
        }
    };

    let param_sets = if method == "random" {
        grid.sample(max_trials.unwrap_or(20), seed)
    } else if let Some(limit) = max_trials {
        grid.all().into_iter().take(limit).collect()
    } else {
        grid.all()
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));

    let mut work = {
        let (bars, objective, dsl_code, strategy, param_sets) =
            (bars.clone(), objective.clone(), dsl_code.clone(), strategy.clone(), param_sets.clone());
        tokio::task::spawn_blocking(move || {
            let backtest_config = spectra_engine::BacktestConfig::default();
            let sweep_res = if let Some(code) = dsl_code {
                let program = spectra_engine::dsl::parse_and_validate(&code).map_err(|e| e.to_string())?;
                let factory = move |params: &spectra_research::ParamSet| -> Result<Box<dyn spectra_engine::Strategy>, spectra_core::Error> {
                    let mut overrides = std::collections::HashMap::new();
                    for (k, v) in &params.values {
                        overrides.insert(k.clone(), *v);
                    }
                    Ok(Box::new(spectra_engine::dsl::DslStrategy::new(program.clone(), &overrides)))
                };
                let obj_fn: Box<spectra_research::Objective<'_>> = if objective == "total_return" {
                    Box::new(spectra_research::objective_total_return)
                } else {
                    Box::new(spectra_research::objective_sharpe)
                };
                spectra_research::sweep::run_sweep(
                    &bars,
                    &factory,
                    &backtest_config,
                    &param_sets,
                    &*obj_fn,
                    spectra_research::SweepOptions { threads: 1 },
                    1,
                )
                .map_err(|e| e.to_string())?
            } else {
                let factory = spectra_backtest::builtin_factory(&strategy).map_err(|e| e.to_string())?;
                let obj_fn: Box<spectra_research::Objective<'_>> = if objective == "total_return" {
                    Box::new(spectra_research::objective_total_return)
                } else {
                    Box::new(spectra_research::objective_sharpe)
                };
                spectra_research::sweep::run_sweep(
                    &bars,
                    &factory,
                    &backtest_config,
                    &param_sets,
                    &*obj_fn,
                    spectra_research::SweepOptions { threads: 1 },
                    1,
                )
                .map_err(|e| e.to_string())?
            };
            Ok::<_, String>(sweep_res)
        })
    };

    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await;
    let mut cancel_requested = false;
    let outcome = loop {
        tokio::select! {
            res = &mut work => break match res {
                Ok(r) => r,
                Err(e) => Err(format!("Sweep worker panicked: {e}")),
            },
            _ = beat.tick() => {
                let p = f32::from_bits(progress.load(Ordering::Relaxed));
                match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, Some(p)).await {
                    Ok(Beat::Continue) => {}
                    Ok(Beat::CancelRequested) => {
                        cancel_requested = true;
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(Beat::Lost) => {
                        cancel.store(true, Ordering::Relaxed);
                        record_metrics(&job.kind, "lost", started);
                        drop(permit);
                        return;
                    }
                    Err(e) => warn!(job_id = %job.id, "sweep heartbeat failed: {e}"),
                }
            }
        }
    };
    drop(permit);

    match outcome {
        Ok(sweep_res) => {
            // Record trials
            for (idx, trial) in sweep_res.trials.iter().enumerate() {
                let mut param_map = serde_json::Map::new();
                for (k, v) in &trial.params.values {
                    param_map.insert(k.clone(), serde_json::json!(v));
                }
                let params_json = serde_json::Value::Object(param_map);
                let metrics_json = serde_json::json!({
                    "sharpe": trial.metrics.sharpe,
                    "total_return": trial.metrics.total_return,
                    "annualized_return": trial.metrics.annualized_return,
                    "annualized_volatility": trial.metrics.annualized_volatility,
                    "max_drawdown": trial.metrics.max_drawdown,
                });
                let (ret_mean, ret_vol) = if trial.returns.is_empty() {
                    (None, None)
                } else {
                    let m = spectra_research::moments(&trial.returns);
                    (Some(m.mean), Some(m.stdev))
                };
                let _ = optimization::insert_trial(
                    &ctx.pool,
                    sweep_id,
                    (idx + 1) as i32,
                    &params_json,
                    &metrics_json,
                    trial.objective,
                    ret_mean,
                    ret_vol,
                )
                .await;

                // Also record as a child run in runs table (Task 2)
                let run_id = spectra_db::new_id();
                let _ = sqlx::query(
                    "INSERT INTO runs (id, workspace_id, experiment_id, strategy_version_id, dataset_version_id, job_id, kind, status, params, metrics, counts_as_trial, strategy, dataset_id, sharpe_per_period, n_obs, finished_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, 'sweep_trial', 'succeeded', $7, $8, true, $9, $10, $11, $12, now())"
                )
                .bind(run_id)
                .bind(job.workspace_id)
                .bind(experiment_id)
                .bind(strategy_version_id)
                .bind(version_id)
                .bind(job.id)
                .bind(&params_json)
                .bind(&metrics_json)
                .bind(&strategy)
                .bind(&version.checksum)
                .bind(ret_mean.unwrap_or(0.0))
                .bind(trial.returns.len() as i64)
                .execute(&ctx.pool)
                .await;
            }

            // Compute statistical overfitting metrics
            let returns_matrix: Vec<Vec<f64>> = sweep_res.trials.iter().map(|t| t.returns.clone()).collect();
            let sharpes: Vec<f64> = sweep_res.trials.iter().map(|t| t.metrics.sharpe).collect();
            let best_trial = sweep_res.best_trial();

            let (dsr, psr, min_trl, pbo, best_params_json, best_metrics_json) = if let Some(best) = best_trial
            {
                let m = spectra_research::moments(&best.returns);
                let sr_mom = spectra_research::moments(&sharpes);
                let sr_var = sr_mom.stdev * sr_mom.stdev;
                let psr =
                    spectra_research::probabilistic_sharpe(best.metrics.sharpe, 0.0, m.n, m.skew, m.kurt);
                let dsr = spectra_research::deflated_sharpe(&best.returns, sweep_res.trials.len(), sr_var);
                let min_trl =
                    spectra_research::min_track_record_length(best.metrics.sharpe, 0.0, m.skew, m.kurt, 0.95);
                let pbo = if returns_matrix.len() >= 2 && returns_matrix.first().is_some_and(|r| r.len() >= 8)
                {
                    spectra_research::cscv_pbo(&returns_matrix, 16).map_or(0.0, |p| p.pbo)
                } else {
                    0.0
                };
                let mut b_params = serde_json::Map::new();
                for (k, v) in &best.params.values {
                    b_params.insert(k.clone(), serde_json::json!(v));
                }
                let b_metrics = serde_json::json!({
                    "sharpe": best.metrics.sharpe,
                    "total_return": best.metrics.total_return,
                    "annualized_return": best.metrics.annualized_return,
                    "annualized_volatility": best.metrics.annualized_volatility,
                    "max_drawdown": best.metrics.max_drawdown,
                });
                (
                    Some(if dsr.is_nan() { 0.0 } else { dsr }),
                    Some(if psr.is_nan() { 0.0 } else { psr }),
                    Some(if min_trl.is_infinite() || min_trl.is_nan() { 0.0 } else { min_trl }),
                    Some(pbo),
                    Some(serde_json::Value::Object(b_params)),
                    Some(b_metrics),
                )
            } else {
                (None, None, None, None, None, None)
            };

            let _ = optimization::update_sweep(
                &ctx.pool,
                sweep_id,
                "succeeded",
                sweep_res.trials.len() as i32,
                Some((sweep_res.best + 1) as i32),
                best_params_json.as_ref(),
                best_metrics_json.as_ref(),
                best_trial.map(|t| t.objective),
                dsr,
                psr,
                min_trl,
                pbo,
            )
            .await;

            let job_result = serde_json::json!({
                "sweep_id": sweep_id,
                "trials_count": sweep_res.trials.len(),
                "best_trial_num": sweep_res.best + 1,
                "best_objective": best_trial.map(|t| t.objective),
                "dsr": dsr,
                "psr": psr,
                "pbo": pbo,
            });
            let _ = jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &job_result).await;
            record_metrics(&job.kind, "succeeded", started);
            info!(job_id = %job.id, sweep_id = %sweep_id, "sweep completed successfully");
        }
        Err(_msg) if cancel_requested => {
            let _ = sqlx::query("UPDATE optimization_sweeps SET status='cancelled', finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id)
                .bind(sweep_id)
                .execute(&ctx.pool)
                .await;
            record_metrics(&job.kind, "cancelled", started);
            let _ = jobs::mark_cancelled(&ctx.pool, job.id, &ctx.worker_id).await;
        }
        Err(msg) => {
            let attempted_trials = param_sets.len() as i32;
            let _ = sqlx::query("UPDATE optimization_sweeps SET status='failed', trial_count=$3, finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id)
                .bind(sweep_id)
                .bind(attempted_trials)
                .execute(&ctx.pool)
                .await;
            if let Some(exp_id) = experiment_id {
                for pset in &param_sets {
                    let mut p_map = serde_json::Map::new();
                    for (k, v) in &pset.values {
                        p_map.insert(k.clone(), serde_json::json!(v));
                    }
                    let run_id = spectra_db::new_id();
                    let _ = sqlx::query(
                        "INSERT INTO runs (id, workspace_id, experiment_id, strategy_version_id, dataset_version_id, job_id, kind, status, params, counts_as_trial, strategy, error, finished_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, 'sweep_trial', 'failed', $7, true, $8, $9, now())"
                    )
                    .bind(run_id)
                    .bind(job.workspace_id)
                    .bind(exp_id)
                    .bind(strategy_version_id)
                    .bind(version_id)
                    .bind(job.id)
                    .bind(serde_json::Value::Object(p_map))
                    .bind(&strategy)
                    .bind(&msg)
                    .execute(&ctx.pool)
                    .await;
                }
            }
            fail_job(&ctx, &job, started, &msg, false).await;
        }
    }
}

/// Runs a walk-forward validation job.
#[allow(clippy::needless_pass_by_value)]
async fn run_walk_forward_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = Instant::now();
    let Some(report_id) = job
        .payload
        .get("report_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        fail_job(&ctx, &job, started, "Walk-forward job missing report_id.", false).await;
        drop(permit);
        return;
    };
    let Some(version_id) = job
        .payload
        .get("dataset_version_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        fail_job(&ctx, &job, started, "Walk-forward job missing dataset_version_id.", false).await;
        drop(permit);
        return;
    };

    let version = match datasets::resolve_version(
        &ctx.pool,
        job.workspace_id,
        version_id,
        Access::engine("worker-walkforward"),
    )
    .await
    {
        Ok(Resolved::Found(v)) => v,
        Ok(Resolved::Sealed { holdout }) => {
            let message = format!("The dataset version is sealed as holdout '{holdout}'; only a holdout evaluation may read it.");
            fail_job(&ctx, &job, started, &message, false).await;
            drop(permit);
            return;
        }
        Ok(Resolved::Missing) => {
            fail_job(&ctx, &job, started, "Dataset version no longer exists in workspace.", false).await;
            drop(permit);
            return;
        }
        Err(e) => {
            fail_job(&ctx, &job, started, &format!("Could not look up dataset version: {e}"), true).await;
            drop(permit);
            return;
        }
    };

    if let Err((msg, retryable)) =
        ingest::ensure_local(&ctx.store, &ctx.data_dir, &version.checksum, version.blob_sha256.as_deref())
            .await
    {
        fail_job(&ctx, &job, started, &msg, retryable).await;
        drop(permit);
        return;
    }

    let bars = match spectra_backtest::load_bars(&ctx.data_dir, &version.checksum) {
        Ok(b) => b,
        Err(e) => {
            fail_job(&ctx, &job, started, &format!("Could not read dataset bars: {e}"), true).await;
            drop(permit);
            return;
        }
    };

    let _ = sqlx::query("UPDATE walk_forward_reports SET status='running' WHERE workspace_id=$1 AND id=$2 AND status='queued'")
        .bind(job.workspace_id)
        .bind(report_id)
        .execute(&ctx.pool)
        .await;

    let param_grid_val = job.payload.get("param_grid").cloned().unwrap_or_else(|| serde_json::json!({}));
    let mode_str = job.payload.get("mode").and_then(serde_json::Value::as_str).unwrap_or("rolling");
    let train_size =
        job.payload.get("train_size").and_then(serde_json::Value::as_u64).unwrap_or(200) as usize;
    let test_size = job.payload.get("test_size").and_then(serde_json::Value::as_u64).unwrap_or(50) as usize;
    let step_size =
        job.payload.get("step_size").and_then(serde_json::Value::as_u64).unwrap_or(test_size as u64) as usize;
    let embargo = job.payload.get("embargo").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
    let objective =
        job.payload.get("objective").and_then(serde_json::Value::as_str).unwrap_or("sharpe").to_string();
    let dsl_code = job.payload.get("dsl_code").and_then(serde_json::Value::as_str).map(ToOwned::to_owned);
    let strategy =
        job.payload.get("strategy").and_then(serde_json::Value::as_str).unwrap_or("sma_cross").to_string();

    let grid = match parse_grid_from_json(&param_grid_val) {
        Ok(g) => g,
        Err(e) => {
            fail_job(&ctx, &job, started, &format!("Invalid parameter grid: {e}"), false).await;
            drop(permit);
            return;
        }
    };
    let param_sets = grid.all();

    let wf_mode = if mode_str == "anchored" {
        spectra_research::WalkForwardMode::Anchored
    } else {
        spectra_research::WalkForwardMode::Rolling
    };
    let wf_config = spectra_research::WalkForwardConfig {
        train: train_size,
        test: test_size,
        step: step_size,
        embargo,
        mode: wf_mode,
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));

    let mut work = {
        let (bars, objective, dsl_code, strategy, param_sets, wf_config) = (
            bars.clone(),
            objective.clone(),
            dsl_code.clone(),
            strategy.clone(),
            param_sets.clone(),
            wf_config,
        );
        tokio::task::spawn_blocking(move || {
            let backtest_config = spectra_engine::BacktestConfig::default();
            let wf_res = if let Some(code) = dsl_code {
                let program = spectra_engine::dsl::parse_and_validate(&code).map_err(|e| e.to_string())?;
                let factory = move |params: &spectra_research::ParamSet| -> Result<Box<dyn spectra_engine::Strategy>, spectra_core::Error> {
                    let mut overrides = std::collections::HashMap::new();
                    for (k, v) in &params.values {
                        overrides.insert(k.clone(), *v);
                    }
                    Ok(Box::new(spectra_engine::dsl::DslStrategy::new(program.clone(), &overrides)))
                };
                let obj_fn: Box<spectra_research::Objective<'_>> = if objective == "total_return" {
                    Box::new(spectra_research::objective_total_return)
                } else {
                    Box::new(spectra_research::objective_sharpe)
                };
                spectra_research::walkforward::run_walk_forward(
                    &bars,
                    &factory,
                    &backtest_config,
                    &param_sets,
                    &wf_config,
                    &*obj_fn,
                    spectra_research::SweepOptions { threads: 1 },
                    None,
                )
                .map_err(|e| e.to_string())?
            } else {
                let factory = spectra_backtest::builtin_factory(&strategy).map_err(|e| e.to_string())?;
                let obj_fn: Box<spectra_research::Objective<'_>> = if objective == "total_return" {
                    Box::new(spectra_research::objective_total_return)
                } else {
                    Box::new(spectra_research::objective_sharpe)
                };
                spectra_research::walkforward::run_walk_forward(
                    &bars,
                    &factory,
                    &backtest_config,
                    &param_sets,
                    &wf_config,
                    &*obj_fn,
                    spectra_research::SweepOptions { threads: 1 },
                    None,
                )
                .map_err(|e| e.to_string())?
            };
            Ok::<_, String>(wf_res)
        })
    };

    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await;
    let mut cancel_requested = false;
    let outcome = loop {
        tokio::select! {
            res = &mut work => break match res {
                Ok(r) => r,
                Err(e) => Err(format!("Walk-forward worker panicked: {e}")),
            },
            _ = beat.tick() => {
                let p = f32::from_bits(progress.load(Ordering::Relaxed));
                match jobs::heartbeat(&ctx.pool, job.id, &ctx.worker_id, Some(p)).await {
                    Ok(Beat::Continue) => {}
                    Ok(Beat::CancelRequested) => {
                        cancel_requested = true;
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(Beat::Lost) => {
                        cancel.store(true, Ordering::Relaxed);
                        record_metrics(&job.kind, "lost", started);
                        drop(permit);
                        return;
                    }
                    Err(e) => warn!(job_id = %job.id, "walk-forward heartbeat failed: {e}"),
                }
            }
        }
    };
    drop(permit);

    match outcome {
        Ok(wf_res) => {
            let eff_ratio = if wf_res.mean_in_sample_objective.abs() > 1e-9 {
                wf_res.mean_out_of_sample_objective / wf_res.mean_in_sample_objective
            } else {
                0.0
            };

            let folds_vec: Vec<serde_json::Value> = wf_res
                .folds
                .iter()
                .enumerate()
                .map(|(idx, f)| {
                    let mut param_map = serde_json::Map::new();
                    for (k, v) in &f.best_params.values {
                        param_map.insert(k.clone(), serde_json::json!(v));
                    }
                    serde_json::json!({
                        "fold": idx + 1,
                        "train_begin": f.fold.train_begin,
                        "train_end": f.fold.train_end,
                        "test_begin": f.fold.test_begin,
                        "test_end": f.fold.test_end,
                        "in_sample_objective": f.in_sample_objective,
                        "out_of_sample_objective": f.out_of_sample_objective,
                        "best_params": serde_json::Value::Object(param_map),
                        "oos_sharpe": f.oos_metrics.sharpe,
                        "oos_return": f.oos_metrics.total_return,
                        "oos_drawdown": f.oos_metrics.max_drawdown,
                    })
                })
                .collect();
            let folds_json = serde_json::Value::Array(folds_vec);

            let _ = optimization::update_walk_forward(
                &ctx.pool,
                report_id,
                "succeeded",
                &folds_json,
                Some(eff_ratio),
                Some(wf_res.mean_in_sample_objective),
                Some(wf_res.mean_out_of_sample_objective),
            )
            .await;

            let job_result = serde_json::json!({
                "report_id": report_id,
                "folds_count": wf_res.folds.len(),
                "efficiency_ratio": eff_ratio,
                "in_sample_sharpe": wf_res.mean_in_sample_objective,
                "out_of_sample_sharpe": wf_res.mean_out_of_sample_objective,
            });
            let _ = jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &job_result).await;
            record_metrics(&job.kind, "succeeded", started);
            info!(job_id = %job.id, report_id = %report_id, "walk-forward completed successfully");
        }
        Err(_msg) if cancel_requested => {
            let _ = sqlx::query("UPDATE walk_forward_reports SET status='cancelled', finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id)
                .bind(report_id)
                .execute(&ctx.pool)
                .await;
            record_metrics(&job.kind, "cancelled", started);
            let _ = jobs::mark_cancelled(&ctx.pool, job.id, &ctx.worker_id).await;
        }
        Err(msg) => {
            let _ = sqlx::query("UPDATE walk_forward_reports SET status='failed', finished_at=now() WHERE workspace_id=$1 AND id=$2")
                .bind(job.workspace_id)
                .bind(report_id)
                .execute(&ctx.pool)
                .await;
            fail_job(&ctx, &job, started, &msg, false).await;
        }
    }
}
