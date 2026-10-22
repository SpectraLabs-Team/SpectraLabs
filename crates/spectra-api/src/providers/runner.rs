//! Dataset refresh execution runner for background worker jobs (M1b task 13).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use spectra_core::Json;
use spectra_data::{append_bars, datastore::encode_dataset, DataStore, Dataset, SymbolTable};
use spectra_db::artifacts::ArtifactBackend;
use spectra_db::jobs::Job;

use super::alpaca::AlpacaConnector;
use super::fake::FakeConnector;
use super::vault::resolve_alpaca_credentials;
use super::{BarResolution, FetchBarsRequest, MarketDataConnector, ProviderError};

/// Job kind identifier for provider-driven dataset refreshes.
pub const PROVIDER_REFRESH_JOB_KIND: &str = "provider_refresh";

/// Supported market data connectors.
#[derive(Debug, Clone)]
pub enum Connector {
    /// Deterministic mock connector.
    Fake(FakeConnector),
    /// Alpaca Market Data API connector.
    Alpaca(AlpacaConnector),
}

impl Connector {
    /// Fetches bars for the requested symbols and time range.
    ///
    /// # Errors
    /// Returns [`ProviderError`] if fetching fails or credentials/rates are invalid.
    pub async fn fetch_bars(&self, req: &FetchBarsRequest) -> Result<Vec<spectra_core::Bar>, ProviderError> {
        match self {
            Self::Fake(c) => c.fetch_bars(req).await,
            Self::Alpaca(c) => c.fetch_bars(req).await,
        }
    }
}

fn to_value(j: &Json) -> JsonValue {
    serde_json::from_str(&j.dump()).unwrap_or(JsonValue::Null)
}

/// Environment needed to execute dataset refresh jobs.
#[derive(Debug, Clone)]
pub struct RefreshEnv {
    /// Database pool.
    pub pool: PgPool,
    /// Artifact backend for content-addressed bars storage.
    pub store: ArtifactBackend,
    /// Local directory of content-addressed datasets.
    pub data_dir: std::path::PathBuf,
    /// Workspace artifact storage quota in bytes.
    pub quota: i64,
}

/// Input payload for a dataset refresh job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshJobPayload {
    /// Target dataset id.
    pub dataset_id: Uuid,
    /// Optional schedule id if enqueued by automated scheduler.
    pub schedule_id: Option<Uuid>,
    /// Provider configuration id.
    pub provider_config_id: Uuid,
    /// Symbols to refresh.
    pub symbols: Vec<String>,
    /// Number of bars to refetch backward for revision overlap detection.
    #[serde(default = "default_lookback")]
    pub lookback_bars: i32,
}

const fn default_lookback() -> i32 {
    5
}

/// The result of executing a dataset refresh.
#[derive(Debug)]
pub enum RefreshOutcome {
    /// New dataset version registered.
    Registered(JsonValue),
    /// Cancelled before completion.
    Cancelled,
    /// Refresh failed.
    Failed {
        /// Explanation of failure.
        message: String,
        /// True if job is retryable.
        retryable: bool,
    },
}

/// Executes a dataset refresh job to completion.
pub async fn run_refresh(
    env: &RefreshEnv,
    job: &Job,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
) -> RefreshOutcome {
    let set_progress = |p: f32| progress.store(p.to_bits(), Ordering::Relaxed);
    set_progress(0.05);

    let payload: RefreshJobPayload = match serde_json::from_value(job.payload.clone()) {
        Ok(p) => p,
        Err(e) => {
            return RefreshOutcome::Failed {
                message: format!("Invalid refresh payload: {e}"),
                retryable: false,
            };
        }
    };

    if cancel.load(Ordering::Relaxed) {
        return RefreshOutcome::Cancelled;
    }

    // 1. Resolve dataset
    let ds_row =
        match spectra_db::datasets::get_dataset(&env.pool, job.workspace_id, payload.dataset_id).await {
            Ok(Some(d)) => d,
            Ok(None) => {
                return RefreshOutcome::Failed {
                    message: format!("Dataset {} does not exist in this workspace", payload.dataset_id),
                    retryable: false,
                };
            }
            Err(e) => {
                return RefreshOutcome::Failed {
                    message: format!("Database error fetching dataset: {e}"),
                    retryable: true,
                };
            }
        };

    // 2. Resolve latest version
    let versions =
        match spectra_db::datasets::dataset_versions(&env.pool, job.workspace_id, payload.dataset_id).await {
            Ok(v) => v,
            Err(e) => {
                return RefreshOutcome::Failed {
                    message: format!("Database error fetching dataset versions: {e}"),
                    retryable: true,
                };
            }
        };
    let latest_version = versions.into_iter().max_by_key(|v| v.version);
    set_progress(0.15);

    // 3. Load base dataset from DataStore
    let base_dataset = match &latest_version {
        Some(v) => {
            let local_store = match DataStore::new(&env.data_dir) {
                Ok(s) => s,
                Err(e) => {
                    return RefreshOutcome::Failed {
                        message: format!("Cannot access local data store: {e}"),
                        retryable: true,
                    };
                }
            };
            match local_store.get(&v.checksum) {
                Ok(ds) => ds,
                Err(_) => {
                    // Fall back to artifact store if present
                    match env.store.get(&v.checksum).await {
                        Ok(bytes) => match spectra_data::datastore::decode_dataset(&bytes, &v.checksum) {
                            Ok(ds) => ds,
                            Err(e) => {
                                return RefreshOutcome::Failed {
                                    message: format!("Failed to decode dataset from artifact store: {e}"),
                                    retryable: false,
                                };
                            }
                        },
                        Err(_) => {
                            return RefreshOutcome::Failed {
                                message: format!("Base dataset version content {} not found", v.checksum),
                                retryable: false,
                            };
                        }
                    }
                }
            }
        }
        None => Dataset { symbols: SymbolTable::from_symbols(&payload.symbols), ..Default::default() },
    };

    if cancel.load(Ordering::Relaxed) {
        return RefreshOutcome::Cancelled;
    }
    set_progress(0.30);

    // 4. Resolve provider config
    let provider_cfg = match spectra_db::providers::get_provider_config(
        &env.pool,
        job.workspace_id,
        payload.provider_config_id,
    )
    .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return RefreshOutcome::Failed {
                message: format!("Provider configuration {} not found", payload.provider_config_id),
                retryable: false,
            };
        }
        Err(e) => {
            return RefreshOutcome::Failed {
                message: format!("Database error fetching provider configuration: {e}"),
                retryable: true,
            };
        }
    };

    // 5. Build connector
    let connector = match provider_cfg.provider.as_str() {
        "fake" => Connector::Fake(FakeConnector::new()),
        "alpaca" => {
            let (key_id, secret_key) = match resolve_alpaca_credentials(
                &env.pool,
                job.workspace_id,
                provider_cfg.secret_id,
                &provider_cfg.settings,
            )
            .await
            {
                Ok(creds) => creds,
                Err(err) => {
                    return RefreshOutcome::Failed {
                        message: format!("Failed to resolve Alpaca credentials: {err}"),
                        retryable: false,
                    };
                }
            };

            let base_url = provider_cfg
                .settings
                .get("base_url")
                .and_then(JsonValue::as_str)
                .unwrap_or(super::alpaca::ALPACA_DATA_URL);
            let feed = provider_cfg.settings.get("feed").and_then(JsonValue::as_str).unwrap_or("iex");

            Connector::Alpaca(AlpacaConnector::new(base_url, key_id, secret_key, feed))
        }
        unknown => {
            return RefreshOutcome::Failed {
                message: format!("Unsupported provider: {unknown}"),
                retryable: false,
            };
        }
    };

    // 6. Calculate fetch range
    let now_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let start_ns = if base_dataset.bars.is_empty() {
        now_ns - 30 * 86_400 * 1_000_000_000
    } else {
        let lookback_ns = i64::from(payload.lookback_bars.max(1)) * 86_400 * 1_000_000_000;
        base_dataset.info.last_ts - lookback_ns
    };
    let end_ns = now_ns;

    let fetch_req = FetchBarsRequest {
        symbols: payload.symbols.clone(),
        start_ns,
        end_ns,
        resolution: BarResolution::Day,
    };

    set_progress(0.50);

    // 7. Fetch bars
    let fetched_bars = match connector.fetch_bars(&fetch_req).await {
        Ok(b) => b,
        Err(e) => {
            return RefreshOutcome::Failed {
                message: format!("Provider fetch failed: {e}"),
                retryable: matches!(e, ProviderError::RateLimited { .. } | ProviderError::Http(_)),
            };
        }
    };

    if cancel.load(Ordering::Relaxed) {
        return RefreshOutcome::Cancelled;
    }
    set_progress(0.70);

    // 8. Append bars into new dataset version
    let new_symbols = SymbolTable::from_symbols(&payload.symbols);
    let note = format!("Refreshed from {} ({})", provider_cfg.name, provider_cfg.provider);
    let new_dataset = match append_bars(&base_dataset, &fetched_bars, &new_symbols, note) {
        Ok(ds) => ds,
        Err(e) => {
            return RefreshOutcome::Failed {
                message: format!("Failed to append refreshed bars: {e}"),
                retryable: false,
            };
        }
    };

    set_progress(0.85);

    // 9. If checksum matches existing latest version, bars were identical
    if let Some(latest) = &latest_version {
        if latest.checksum == new_dataset.info.id {
            if let Some(sched_id) = payload.schedule_id {
                let _ = spectra_db::providers::update_refresh_schedule_run(
                    &env.pool,
                    sched_id,
                    Utc::now(),
                    Utc::now() + chrono::Duration::seconds(86400),
                )
                .await;
            }
            return RefreshOutcome::Registered(serde_json::json!({
                "dataset_id": payload.dataset_id,
                "dataset_version_id": latest.id,
                "version": latest.version,
                "checksum": latest.checksum,
                "status": "up_to_date",
                "bars_count": new_dataset.bars.len(),
            }));
        }
    }

    // 10. Store in local DataStore and ArtifactBackend
    let encoded = encode_dataset(&new_dataset);
    let bars_sha = spectra_db::artifacts::sha256_hex(&encoded);
    let needed = i64::try_from(encoded.len()).unwrap_or(i64::MAX);

    match spectra_db::artifacts::quota(&env.pool, job.workspace_id, &bars_sha, env.quota).await {
        Ok(Some(q)) if !q.present && q.used + needed > q.quota => {
            return RefreshOutcome::Failed {
                message: format!(
                    "The refreshed dataset needs {needed} bytes of storage, and this workspace has used {} of its {} bytes.",
                    q.used, q.quota
                ),
                retryable: false,
            };
        }
        Ok(Some(_)) => {}
        Ok(None) => {
            return RefreshOutcome::Failed {
                message: "The workspace no longer exists.".into(),
                retryable: false,
            };
        }
        Err(e) => {
            return RefreshOutcome::Failed {
                message: format!("Could not check storage quota: {e}"),
                retryable: true,
            };
        }
    }

    if let Err(e) = env.store.put(encoded.clone()).await {
        return RefreshOutcome::Failed {
            message: format!("Could not store dataset artifact: {e}"),
            retryable: true,
        };
    }

    let (data_dir, local_ds) = (env.data_dir.clone(), new_dataset.clone());
    if let Ok(Err(e)) =
        tokio::task::spawn_blocking(move || DataStore::new(&data_dir).and_then(|s| s.put(&local_ds))).await
    {
        tracing::warn!(error = %e, "refreshed dataset not written to local store");
    }

    set_progress(0.95);

    // 11. Register new derived dataset version
    let quality_report =
        spectra_data::quality_report_json(&new_dataset, &spectra_data::CsvOptions::default());
    let quality_value: JsonValue = to_value(&quality_report);

    let derivation_params = serde_json::json!({
        "provider": provider_cfg.provider,
        "provider_config_id": provider_cfg.id,
        "symbols": payload.symbols,
        "fetched_bars": fetched_bars.len(),
        "total_bars": new_dataset.bars.len(),
    });

    let source = serde_json::json!({
        "provider": provider_cfg.provider,
        "provider_config_name": provider_cfg.name,
        "refresh_at": Utc::now().to_rfc3339(),
    });

    let reg = match spectra_db::datasets::register_derived_version(
        &env.pool,
        &spectra_db::datasets::NewDerivedDatasetVersion {
            workspace_id: job.workspace_id,
            dataset_name: &ds_row.name,
            checksum: &new_dataset.info.id,
            row_count: new_dataset.bars.len() as i64,
            byte_size: encoded.len() as i64,
            quality_report: quality_value,
            created_by: job.created_by,
            blob_sha256: Some(&bars_sha),
            source: Some(source),
            derived_from_id: latest_version.as_ref().map(|v| v.id),
            derivation_kind: Some("refresh"),
            derivation_params: Some(derivation_params),
        },
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return RefreshOutcome::Failed {
                message: format!("Could not register derived dataset version: {e}"),
                retryable: true,
            };
        }
    };

    // 12. Update schedule run state if triggered from a schedule
    if let Some(sched_id) = payload.schedule_id {
        let _ = spectra_db::providers::update_refresh_schedule_run(
            &env.pool,
            sched_id,
            Utc::now(),
            Utc::now() + chrono::Duration::seconds(86400),
        )
        .await;
    }

    set_progress(1.0);

    RefreshOutcome::Registered(serde_json::json!({
        "dataset_id": payload.dataset_id,
        "dataset_version_id": reg.id,
        "version": reg.version,
        "checksum": new_dataset.info.id,
        "status": "registered",
        "bars_count": new_dataset.bars.len(),
        "existed": reg.existed,
    }))
}

/// Sweeps due dataset refresh schedules and enqueues worker refresh jobs.
pub async fn sweep_due_refresh_schedules(pool: &PgPool) {
    let due = match spectra_db::providers::list_due_refresh_schedules(pool, Utc::now(), 50).await {
        Ok(list) => list,
        Err(e) => {
            error!("failed to query due refresh schedules: {e}");
            return;
        }
    };

    for sched in due {
        let next_run = Utc::now() + chrono::Duration::seconds(sched.interval_seconds);
        if let Err(e) =
            spectra_db::providers::update_refresh_schedule_run(pool, sched.id, sched.next_run_at, next_run)
                .await
        {
            error!(schedule_id = %sched.id, "failed to advance schedule next_run_at: {e}");
            continue;
        }

        let payload = serde_json::json!({
            "dataset_id": sched.dataset_id,
            "schedule_id": sched.id,
            "provider_config_id": sched.provider_config_id,
            "symbols": sched.symbols,
            "lookback_bars": sched.lookback_bars,
        });

        let new_job = spectra_db::jobs::NewJob {
            workspace_id: sched.workspace_id,
            kind: PROVIDER_REFRESH_JOB_KIND.to_string(),
            payload,
            priority: 5,
            max_attempts: 3,
            created_by: sched.created_by,
        };

        if let Err(e) = spectra_db::jobs::enqueue(pool, &new_job).await {
            error!(schedule_id = %sched.id, "failed to enqueue refresh job: {e}");
        } else {
            info!(
                schedule_id = %sched.id,
                dataset_id = %sched.dataset_id,
                "enqueued automated dataset refresh job"
            );
        }
    }
}
