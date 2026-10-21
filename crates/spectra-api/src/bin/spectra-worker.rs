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
use spectra_backtest::{run_job_from_store, JobFailure, JobProducts};
use spectra_core::{Json, ENGINE_VERSION};
use spectra_db::artifacts::{store_for_run, ArtifactBackend, NewArtifact};
use spectra_db::datasets::{self, NewBacktestRun};
use spectra_db::jobs::{self, Beat, Job, NewRun, WorkerInfo};
use spectra_db::{PgPool, PoolConfig};
use spectra_engine::RunRecord;
use spectra_engine::{Progress, RunControl, RunLimits};
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
    let housekeeping = tokio::spawn(async move {
        let mut beat = tokio::time::interval(HEARTBEAT);
        let mut sweep = tokio::time::interval(SWEEP_EVERY);
        let mut expire = tokio::time::interval(EXPIRE_UPLOADS_EVERY);
        let mut sample = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = hk_stop.changed() => break,
                _ = beat.tick() => {
                    if let Err(e) = jobs::worker_heartbeat(&hk_pool, &hk_info).await {
                        error!("heartbeat failed: {e}");
                    }
                }
                _ = expire.tick() => expire_uploads(&hk_pool, &hk_store).await,
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
    if job.kind == spectra_backtest::JOB_KIND {
        run_backtest_job(ctx, job, permit).await;
        return;
    }
    if job.kind == engine::INGEST_JOB_KIND {
        run_ingest_job(ctx, job, permit).await;
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

/// Converts an engine JSON document to the database's JSON type.
fn to_db_json(j: &Json) -> serde_json::Value {
    serde_json::from_str(&engine::to_text(j)).unwrap_or(serde_json::Value::Null)
}

/// How a backtest's blocking part ended.
#[allow(clippy::large_enum_variant)] // one value per job, moved once
enum BacktestEnd {
    Done(Result<JobProducts, JobFailure>),
    Panicked,
    /// The run ignored cancellation past its deadline; its result is discarded.
    Overran(Duration),
}

/// A `backtest` job (M3 task 5). Unlike the other kinds, the engine reports
/// progress (sent with each heartbeat) and stops cleanly when cancelled or out
/// of time, so the slot is freed as soon as the job ends.
#[allow(clippy::needless_pass_by_value)] // the spawned task owns its job and permit
async fn run_backtest_job(ctx: Arc<Ctx>, job: Job, permit: tokio::sync::OwnedSemaphorePermit) {
    let started = Instant::now();
    let pool = &ctx.pool;
    let wid = ctx.worker_id.as_str();

    // The payload was validated when queued; check again, it may predate a change.
    let parsed = Json::parse(&job.payload.to_string())
        .map_err(|e| format!("Invalid payload: {e}"))
        .and_then(|p| engine::backtest_job(&p).map_err(|(_, m)| m));
    let bt = match parsed {
        Ok(j) => j,
        Err(msg) => return fail_job(&ctx, &job, started, &msg, false).await,
    };
    let Ok(version_id) = Uuid::parse_str(&bt.dataset_version_id) else {
        return fail_job(&ctx, &job, started, "dataset_version_id must be a UUID.", false).await;
    };
    let version = match datasets::resolve_version(pool, job.workspace_id, version_id).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            let msg = "The dataset version no longer exists in this workspace.";
            return fail_job(&ctx, &job, started, msg, false).await;
        }
        Err(e) => {
            let msg = format!("Could not look up the dataset version: {e}");
            return fail_job(&ctx, &job, started, &msg, true).await;
        }
    };

    // Datasets ingested through the Studio live in the artifact store; copy this one into the
    // local store the engine reads, unless it is there already.
    if let Err((msg, retryable)) =
        ingest::ensure_local(&ctx.store, &ctx.data_dir, &version.checksum, version.blob_sha256.as_deref())
            .await
    {
        return fail_job(&ctx, &job, started, &msg, retryable).await;
    }

    // The engine stops itself shortly before the job's limit; the deadline below is a backstop.
    let timeout = engine::job_timeout(&job.kind);
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
    let mut work = {
        let (cancel, progress) = (Arc::clone(&cancel), Arc::clone(&progress));
        let (data_dir, checksum, bt) = (ctx.data_dir.clone(), version.checksum.clone(), bt.clone());
        let limits = RunLimits {
            max_bars: Some(ctx.backtest_max_bars),
            max_instruments: Some(ctx.backtest_max_instruments),
            max_wall_time: Some(timeout.saturating_sub(Duration::from_secs(15))),
        };
        tokio::task::spawn_blocking(move || {
            let mut report = |p: Progress| progress.store((p.fraction() as f32).to_bits(), Ordering::Relaxed);
            let mut ctl = RunControl {
                limits,
                cancel: Some(&*cancel),
                progress: Some(&mut report),
                progress_every: 1000,
            };
            run_job_from_store(&data_dir, &checksum, &bt, &mut ctl)
        })
    };

    let deadline = tokio::time::Instant::now() + timeout + Duration::from_secs(30);
    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.tick().await; // the first tick is immediate; the claim was the first heartbeat
    let (mut cancel_requested, mut lost) = (false, false);
    let end = loop {
        tokio::select! {
            res = &mut work => break match res {
                Ok(r) => BacktestEnd::Done(r),
                Err(_) => BacktestEnd::Panicked,
            },
            _ = beat.tick() => {
                let p = f32::from_bits(progress.load(Ordering::Relaxed));
                match jobs::heartbeat(pool, job.id, wid, Some(p)).await {
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
                break BacktestEnd::Overran(timeout);
            }
        }
    };

    if matches!(end, BacktestEnd::Overran(_)) {
        // Keep the slot until the computation really returns.
        tokio::spawn(async move {
            let _ = work.await;
            drop(permit);
        });
    } else {
        drop(permit);
    }

    if lost {
        warn!(job_id = %job.id, "job was taken over after a missed heartbeat; dropping it");
        record_metrics(&job.kind, "lost", started);
        return;
    }
    match end {
        BacktestEnd::Done(Ok(products)) => {
            match finish_backtest(&ctx, &job, &bt, &version, &products).await {
                Ok(()) => record_metrics(&job.kind, "succeeded", started),
                // The job stays running without heartbeats, so the stale sweep requeues it.
                Err(e) => error!(job_id = %job.id, "could not record the backtest: {e}"),
            }
        }
        BacktestEnd::Done(Err(JobFailure::Cancelled)) => {
            if !cancel_requested {
                warn!(job_id = %job.id, "backtest stopped without a cancel request");
            }
            record_metrics(&job.kind, "cancelled", started);
            if let Err(e) = jobs::mark_cancelled(pool, job.id, wid).await {
                error!(job_id = %job.id, "could not record the cancellation: {e}");
            }
        }
        BacktestEnd::Done(Err(JobFailure::Failed { retryable, message })) => {
            fail_job(&ctx, &job, started, &message, retryable).await;
        }
        BacktestEnd::Panicked => {
            fail_job(&ctx, &job, started, "The engine crashed while running this backtest.", true).await;
        }
        BacktestEnd::Overran(t) => {
            let msg = format!("The backtest ran longer than its {} s limit.", t.as_secs());
            fail_job(&ctx, &job, started, &msg, false).await;
        }
    }
}

/// The strategy name recorded on a run: the built-in's name, or for several
/// strategies on one account their names joined with `+`.
fn run_strategy_name(req: &spectra_backtest::BacktestRequest) -> String {
    if req.is_multi() {
        req.strategies.iter().map(|a| a.strategy.name).collect::<Vec<_>>().join("+")
    } else {
        req.strategy.name.to_string()
    }
}

/// Records the run and stores every output file, then completes the job.
async fn finish_backtest(
    ctx: &Ctx,
    job: &Job,
    bt: &spectra_backtest::BacktestJob,
    version: &datasets::DatasetVersion,
    products: &JobProducts,
) -> Result<(), spectra_db::Error> {
    let summary = to_db_json(&products.summary);
    let run_id = datasets::insert_backtest_run(
        &ctx.pool,
        &NewBacktestRun {
            id: spectra_db::new_id(),
            workspace_id: job.workspace_id,
            job_id: job.id,
            params: to_db_json(&products.params),
            metrics: summary.clone(),
            seed: i64::try_from(bt.request.config.seed).ok(),
            engine_version: products.run.engine_version.clone(),
            created_by: job.created_by,
            started_at: job.started_at.unwrap_or_else(chrono::Utc::now),
            dataset_version_id: version.id,
            strategy_version_id: bt.strategy_version_id.as_deref().and_then(|v| Uuid::parse_str(v).ok()),
            strategy: run_strategy_name(&bt.request),
            strategy_sha256: bt.request.strategy_sha256(),
        },
    )
    .await?;

    let mut stored = Vec::new();
    let mut problems = Vec::new();
    for f in &products.files {
        let artifact = NewArtifact {
            workspace_id: job.workspace_id,
            run_id,
            kind: f.kind,
            content_type: f.media_type,
            bytes: &f.bytes,
        };
        match store_for_run(&ctx.pool, &ctx.store, artifact, ctx.quota).await {
            Ok(s) => stored.push(serde_json::json!({
                "kind": f.kind,
                "file_name": f.file_name,
                "content_type": f.media_type,
                "sha256": s.sha256,
                "size": s.size,
            })),
            Err(e) => {
                error!(job_id = %job.id, kind = f.kind, "artifact not stored: {e}");
                problems.push(serde_json::json!({ "kind": f.kind, "error": e.to_string() }));
            }
        }
    }

    let result = serde_json::json!({
        "run_id": run_id,
        "dataset_version_id": version.id,
        "fingerprints": {
            "engine_version": products.run.engine_version,
            "dataset_sha256": products.run.dataset_sha256,
            "config_sha256": products.run.config_sha256,
            "result_sha256": products.run.result_sha256,
            "strategy_sha256": bt.request.strategy_sha256(),
        },
        "summary": summary,
        "artifacts": stored,
        "artifact_errors": problems,
    });
    if jobs::complete(&ctx.pool, job.id, &ctx.worker_id, &result).await?.is_none() {
        warn!(job_id = %job.id, "backtest finished, but another worker had taken it over");
    }
    info!(job_id = %job.id, run_id = %run_id, result_sha256 = %products.run.result_sha256, "backtest recorded");
    Ok(())
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
        let version = match datasets::resolve_version(&ctx.pool, job.workspace_id, version).await {
            Ok(Some(v)) => v,
            Ok(None) => {
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
