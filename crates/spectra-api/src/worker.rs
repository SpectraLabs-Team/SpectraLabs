//! Worker logic for running engine jobs (backtests, survival, etc.).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use spectra_backtest::{run_job_from_store, JobFailure, JobProducts};
use spectra_core::Json;
use spectra_db::artifacts::{store_for_run, ArtifactBackend, NewArtifact};
use spectra_db::datasets::{self, Access, NewBacktestRun, Resolved};
use spectra_db::jobs::{self, Beat, Job};
use spectra_db::PgPool;
use spectra_engine::{Progress, RunControl, RunLimits};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{engine, ingest, observability};

/// Heartbeat interval for running jobs.
pub const HEARTBEAT: Duration = Duration::from_secs(5);

/// Environment needed to run a backtest job.
#[derive(Debug, Clone)]
pub struct BacktestWorkerEnv {
    pub pool: PgPool,
    pub store: ArtifactBackend,
    pub data_dir: PathBuf,
    pub quota: i64,
    pub backtest_max_bars: usize,
    pub backtest_max_instruments: usize,
}

impl BacktestWorkerEnv {
    #[must_use]
    pub fn new(pool: PgPool, store: ArtifactBackend, data_dir: PathBuf, quota: i64) -> Self {
        Self { pool, store, data_dir, quota, backtest_max_bars: 50_000_000, backtest_max_instruments: 10_000 }
    }
}

/// The strategy name recorded on a run: the built-in's name, or for several
/// strategies on one account their names joined with `+`.
#[must_use]
pub fn run_strategy_name(req: &spectra_backtest::BacktestRequest) -> String {
    if req.dsl_code.is_some() {
        "dsl".to_string()
    } else if req.is_multi() {
        req.strategies.iter().map(|a| a.strategy.name).collect::<Vec<_>>().join("+")
    } else {
        req.strategy.name.to_string()
    }
}

/// Converts an engine JSON document to the database's JSON type.
#[must_use]
pub fn to_db_json(j: &Json) -> serde_json::Value {
    serde_json::from_str(&engine::to_text(j)).unwrap_or(serde_json::Value::Null)
}

/// How a backtest's blocking part ended.
#[allow(clippy::large_enum_variant)]
enum BacktestEnd {
    Done(Result<JobProducts, JobFailure>),
    Panicked,
    Overran(Duration),
}

/// Record job metrics on completion.
pub fn record_metrics(kind: &str, status: &str, started: Instant) {
    observability::JobMetrics::job_completed(kind, status);
    observability::JobMetrics::job_duration(kind, status, started.elapsed().as_secs_f64());
}

/// Fails a job (retryable failures go back to the queue) and records metrics.
pub async fn fail_backtest_job(
    env: &BacktestWorkerEnv,
    job: &Job,
    worker_id: &str,
    started: Instant,
    message: &str,
    retryable: bool,
) {
    record_metrics(&job.kind, if retryable { "failed_retryable" } else { "failed_permanent" }, started);
    if let Err(e) = jobs::fail(&env.pool, job.id, worker_id, message, retryable).await {
        error!(job_id = %job.id, "could not record the failure: {e}");
    }
}

/// Records the run and stores every output file, then completes the job.
///
/// # Errors
/// Fails if the run could not be written to the database.
pub async fn finish_backtest(
    env: &BacktestWorkerEnv,
    job: &Job,
    worker_id: &str,
    bt: &spectra_backtest::BacktestJob,
    version: &datasets::DatasetVersion,
    products: &JobProducts,
) -> Result<Uuid, spectra_db::Error> {
    let summary = to_db_json(&products.summary);
    let run_id = datasets::insert_backtest_run(
        &env.pool,
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
        match store_for_run(&env.pool, &env.store, artifact, env.quota).await {
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
    if jobs::complete(&env.pool, job.id, worker_id, &result).await?.is_none() {
        warn!(job_id = %job.id, "backtest finished, but another worker had taken it over");
    }
    info!(job_id = %job.id, run_id = %run_id, result_sha256 = %products.run.result_sha256, "backtest recorded");
    Ok(run_id)
}

/// Runs a claimed backtest job to completion.
///
/// # Errors
/// Returns an error message and whether it is retryable if the backtest job failed.
pub async fn run_backtest_job(
    env: &BacktestWorkerEnv,
    job: &Job,
    worker_id: &str,
) -> Result<Uuid, (String, bool)> {
    let started = Instant::now();
    let pool = &env.pool;

    let parsed = Json::parse(&job.payload.to_string())
        .map_err(|e| format!("Invalid payload: {e}"))
        .and_then(|p| engine::backtest_job(&p).map_err(|(_, m)| m));
    let bt = match parsed {
        Ok(j) => j,
        Err(msg) => {
            fail_backtest_job(env, job, worker_id, started, &msg, false).await;
            return Err((msg, false));
        }
    };
    let Ok(version_id) = Uuid::parse_str(&bt.dataset_version_id) else {
        let msg = "dataset_version_id must be a UUID.";
        fail_backtest_job(env, job, worker_id, started, msg, false).await;
        return Err((msg.to_string(), false));
    };
    let version = match datasets::resolve_version(
        pool,
        job.workspace_id,
        version_id,
        Access::engine("worker-backtest"),
    )
    .await
    {
        Ok(Resolved::Found(v)) => v,
        // A job queued before its data was sealed, or one that bypassed the API check.
        Ok(Resolved::Sealed { holdout }) => {
            let msg = format!("The dataset version is sealed as holdout '{holdout}'; only a holdout evaluation may read it.");
            fail_backtest_job(env, job, worker_id, started, &msg, false).await;
            return Err((msg, false));
        }
        Ok(Resolved::Missing) => {
            let msg = "The dataset version no longer exists in this workspace.";
            fail_backtest_job(env, job, worker_id, started, msg, false).await;
            return Err((msg.to_string(), false));
        }
        Err(e) => {
            let msg = format!("Could not look up the dataset version: {e}");
            fail_backtest_job(env, job, worker_id, started, &msg, true).await;
            return Err((msg, true));
        }
    };

    if let Err((msg, retryable)) =
        ingest::ensure_local(&env.store, &env.data_dir, &version.checksum, version.blob_sha256.as_deref())
            .await
    {
        fail_backtest_job(env, job, worker_id, started, &msg, retryable).await;
        return Err((msg, retryable));
    }

    let timeout = engine::job_timeout(&job.kind);
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
    let mut work = {
        let (cancel, progress) = (Arc::clone(&cancel), Arc::clone(&progress));
        let (data_dir, checksum, bt) = (env.data_dir.clone(), version.checksum.clone(), bt.clone());
        let limits = RunLimits {
            max_bars: Some(env.backtest_max_bars),
            max_instruments: Some(env.backtest_max_instruments),
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
    beat.tick().await;
    let (mut cancel_requested, mut lost) = (false, false);
    let end = loop {
        tokio::select! {
            res = &mut work => break match res {
                Ok(r) => BacktestEnd::Done(r),
                Err(_) => BacktestEnd::Panicked,
            },
            _ = beat.tick() => {
                let p = f32::from_bits(progress.load(Ordering::Relaxed));
                match jobs::heartbeat(pool, job.id, worker_id, Some(p)).await {
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

    if lost {
        warn!(job_id = %job.id, "job was taken over after a missed heartbeat; dropping it");
        record_metrics(&job.kind, "lost", started);
        return Err(("lost".into(), false));
    }

    match end {
        BacktestEnd::Done(Ok(products)) => {
            match finish_backtest(env, job, worker_id, &bt, &version, &products).await {
                Ok(run_id) => {
                    record_metrics(&job.kind, "succeeded", started);
                    Ok(run_id)
                }
                Err(e) => {
                    error!(job_id = %job.id, "could not record the backtest: {e}");
                    Err((e.to_string(), true))
                }
            }
        }
        BacktestEnd::Done(Err(JobFailure::Cancelled)) => {
            if !cancel_requested {
                warn!(job_id = %job.id, "backtest stopped without a cancel request");
            }
            record_metrics(&job.kind, "cancelled", started);
            if let Err(e) = jobs::mark_cancelled(pool, job.id, worker_id).await {
                error!(job_id = %job.id, "could not record the cancellation: {e}");
            }
            Err(("cancelled".to_string(), false))
        }
        BacktestEnd::Done(Err(JobFailure::Failed { retryable, message })) => {
            fail_backtest_job(env, job, worker_id, started, &message, retryable).await;
            Err((message, retryable))
        }
        BacktestEnd::Panicked => {
            let msg = "The backtest worker encountered an unexpected panic during execution.";
            fail_backtest_job(env, job, worker_id, started, msg, false).await;
            Err((msg.to_string(), false))
        }
        BacktestEnd::Overran(t) => {
            let msg = format!("The backtest ran longer than its {} s limit.", t.as_secs());
            fail_backtest_job(env, job, worker_id, started, &msg, false).await;
            Err((msg, false))
        }
    }
}
