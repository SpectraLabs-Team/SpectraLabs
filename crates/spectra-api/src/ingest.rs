//! The `ingest` job (M1 task 5): an uploaded file becomes a dataset version.
//!
//! 1. Read the upload (workspace-scoped; it may have expired) from the
//!    artifact store, which re-verifies its SHA-256.
//! 2. Parse it with the job's settings: exactly `spectra-ingest`'s rules,
//!    reporting progress by rows read and stopping when cancelled.
//! 3. Store the dataset's `bars.qfb` in the artifact store (so every worker
//!    can read it) and in this worker's local dataset store, register it as
//!    the next version of the named dataset (the same content changes
//!    nothing), keep the upload as its source, and write the audit log.
//!
//! A file the rules refuse (a bad row without "skip bad rows", no valid rows,
//! a missing column) is not a crash: the job fails for good with the full
//! quality report of the file in its result, every bad row counted.
//!
//! [`ensure_local`] is the other half: before a backtest, a worker copies a
//! version's `bars.qfb` from the artifact store into its local store.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use spectra_core::Json;
use spectra_data::{CsvOptions, DataStore, Dataset};
use spectra_db::artifacts::ArtifactBackend;
use spectra_db::jobs::Job;
use spectra_db::PgPool;
use uuid::Uuid;

/// What an ingest needs from its worker.
#[derive(Debug, Clone)]
pub struct IngestEnv {
    /// The database.
    pub pool: PgPool,
    /// The artifact store (uploads in, dataset files out).
    pub store: ArtifactBackend,
    /// This worker's local dataset store (`SPECTRA_DATA_DIR`).
    pub data_dir: PathBuf,
    /// Default storage quota per workspace.
    pub quota: i64,
}

/// How an ingest ended.
#[derive(Debug)]
pub enum IngestOutcome {
    /// Registered (or the same content was already a version): the job's result.
    Registered(serde_json::Value),
    /// The file was refused under the job's settings: the reason, and a
    /// result carrying the file's quality report. Never retried.
    Rejected {
        /// For the job's `error`.
        message: String,
        /// For the job's `result`.
        result: serde_json::Value,
    },
    /// Something else went wrong.
    Failed {
        /// For the job's `error`.
        message: String,
        /// Worth another attempt (a database or storage hiccup).
        retryable: bool,
    },
    /// Stopped on request.
    Cancelled,
}

/// An ingest job's payload, as `POST /api/v1/datasets/ingest` queues it.
#[derive(Debug, Clone)]
pub struct IngestPayload {
    /// The upload to read.
    pub upload_id: Uuid,
    /// The dataset to add a version to.
    pub dataset_name: String,
    /// Parsed settings.
    pub options: CsvOptions,
    /// The settings as given (canonical JSON).
    pub settings: serde_json::Value,
    /// The request that queued it, for the audit log.
    pub request_id: String,
}

impl IngestPayload {
    /// Reads a job payload.
    ///
    /// # Errors
    /// A message naming what is missing or invalid.
    pub fn from_json(payload: &serde_json::Value) -> Result<Self, String> {
        let upload_id = payload
            .get("upload_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("upload_id must be a UUID")?;
        let dataset_name = payload
            .get("dataset_name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("dataset_name is required")?
            .to_string();
        let settings = payload.get("settings").cloned().ok_or("settings are required")?;
        let doc = Json::parse(&settings.to_string()).map_err(|e| format!("settings: {e}"))?;
        let options = spectra_data::parse_settings(&doc).map_err(|errors| {
            let parts: Vec<String> =
                errors.iter().map(|e| format!("settings.{}: {}", e.field, e.message)).collect();
            parts.join("; ")
        })?;
        let request_id =
            payload.get("request_id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        Ok(Self { upload_id, dataset_name, options, settings, request_id })
    }
}

fn to_value(j: &Json) -> serde_json::Value {
    serde_json::from_str(&j.dump()).unwrap_or(serde_json::Value::Null)
}

fn failed(message: impl Into<String>, retryable: bool) -> IngestOutcome {
    IngestOutcome::Failed { message: message.into(), retryable }
}

/// Runs one ingest. `progress` receives the fraction done (as `f32` bits);
/// setting `cancel` stops it at the next progress report.
#[allow(clippy::too_many_lines)] // one linear flow
pub async fn run_ingest(
    env: &IngestEnv,
    job: &Job,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
) -> IngestOutcome {
    let set_progress = |p: f32| progress.store(p.to_bits(), Ordering::Relaxed);
    let payload = match IngestPayload::from_json(&job.payload) {
        Ok(p) => p,
        Err(m) => return failed(format!("Invalid ingest payload: {m}"), false),
    };
    let ws = job.workspace_id;

    // 1. The upload's bytes.
    let upload = match spectra_db::uploads::get(&env.pool, ws, payload.upload_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return failed("The upload has expired or was removed; upload the file again.", false),
        Err(e) => return failed(format!("Could not look up the upload: {e}"), true),
    };
    // Read the upload as a stream from the store (a download only for S3): the file is never
    // held in memory, and hashing it while parsing verifies it.
    let staging = env.store.staging_dir();
    let file = match env.store.local_file(&upload.sha256, &staging).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return failed("The uploaded file is no longer stored; upload it again.", false);
        }
        Err(e) => return failed(format!("Could not read the uploaded file: {e}"), true),
    };
    set_progress(0.05);

    // 2. Parse it (CPU-bound: on the blocking pool), reporting the share of bytes read.
    let (opt, cancel_flag, progress_atomic) =
        (payload.options.clone(), Arc::clone(&cancel), Arc::clone(&progress));
    let (path, size, expected) = (file.path.clone(), upload.size_bytes.max(1) as f32, upload.sha256.clone());
    let parsed = tokio::task::spawn_blocking(move || {
        let open = || std::fs::File::open(&path).map(|f| std::io::BufReader::with_capacity(1 << 20, f));
        let is_parquet = {
            let mut magic = [0u8; 4];
            let mut f = open().map_err(|e| (format!("cannot open the uploaded file: {e}"), None))?;
            use std::io::Read;
            f.read_exact(&mut magic).is_ok() && &magic == b"PAR1"
        };
        if is_parquet {
            let raw = std::fs::File::open(&path)
                .map_err(|e| (format!("cannot open the uploaded file: {e}"), None))?;
            let mut reader = Hashing { inner: raw, hash: spectra_core::Sha256::new() };
            let intact = std::io::copy(&mut reader, &mut std::io::sink()).is_ok()
                && reader.hash.clone().hex_digest() == expected;
            if !intact {
                return Err(("the stored upload does not match its SHA-256".to_string(), None));
            }

            let total_rows = spectra_parquet::parquet_total_rows(&path).unwrap_or(0);
            let loaded = spectra_parquet::load_parquet_file(
                &path,
                &opt,
                Some(&mut |rows_read| {
                    let frac = if total_rows > 0 {
                        0.05 + 0.80 * (rows_read as f32 / total_rows as f32).min(1.0)
                    } else {
                        0.5
                    };
                    progress_atomic.store(frac.to_bits(), Ordering::Relaxed);
                    !cancel_flag.load(Ordering::Relaxed)
                }),
            );
            match loaded {
                Ok(d) => Ok(d),
                Err(e) if e.to_string() == "cancelled" => Err((e.to_string(), None)),
                Err(e) => {
                    let report = spectra_parquet::report_parquet_file(&path, &opt)
                        .ok()
                        .map(|d| spectra_data::quality_report_json(&d, &opt));
                    Err((e.to_string(), report))
                }
            }
        } else {
            let file = open().map_err(|e| (format!("cannot open the uploaded file: {e}"), None))?;
            // Every byte is hashed on its way to the parser, and the rest of the file after it
            // stops, so a corrupt upload is reported as such whatever the parser made of it.
            let mut reader = Hashing { inner: file, hash: spectra_core::Sha256::new() };
            let loaded = spectra_data::load_csv_reader(
                &mut reader,
                &opt,
                Some(&mut |read| {
                    let frac = 0.05 + 0.80 * (read as f32 / size).min(1.0);
                    progress_atomic.store(frac.to_bits(), Ordering::Relaxed);
                    !cancel_flag.load(Ordering::Relaxed)
                }),
            );
            let cancelled = matches!(&loaded, Err(e) if e.to_string() == "cancelled");
            if !cancelled {
                let intact = std::io::copy(&mut reader, &mut std::io::sink()).is_ok()
                    && reader.hash.clone().hex_digest() == expected;
                if !intact {
                    return Err(("the stored upload does not match its SHA-256".to_string(), None));
                }
            }
            match loaded {
                Ok(d) => Ok(d),
                Err(e) if e.to_string() == "cancelled" => Err((e.to_string(), None)),
                Err(e) => {
                    // Refused: read the whole file once more without stopping, so the report
                    // lists every bad row, not just the first. Also streamed.
                    let report = open()
                        .ok()
                        .and_then(|r| spectra_data::report_csv_reader(r, &opt).ok())
                        .map(|d| spectra_data::quality_report_json(&d, &opt));
                    Err((e.to_string(), report))
                }
            }
        }
    })
    .await;
    if file.temporary {
        let _ = tokio::fs::remove_file(&file.path).await;
    }
    let dataset: Dataset = match parsed {
        Ok(Ok(d)) => d,
        Ok(Err((message, _))) if message == "cancelled" && cancel.load(Ordering::Relaxed) => {
            return IngestOutcome::Cancelled;
        }
        Ok(Err((message, _))) if message == "the stored upload does not match its SHA-256" => {
            return failed("The stored upload is corrupt; upload the file again.", false);
        }
        Ok(Err((message, report))) => {
            let result = serde_json::json!({
                "rejected": true,
                "error": message,
                "upload_id": upload.id,
                "filename": upload.filename,
                "settings": payload.settings,
                "quality_report": report.as_ref().map_or(serde_json::Value::Null, to_value),
            });
            return IngestOutcome::Rejected {
                message: format!("The file was not ingested: {message}"),
                result,
            };
        }
        Err(_) => return failed("The parser crashed while reading this file.", false),
    };
    if cancel.load(Ordering::Relaxed) {
        return IngestOutcome::Cancelled;
    }
    set_progress(0.88);

    // 3. Store the bars where every worker can read them, and locally.
    let encoded = spectra_data::datastore::encode_dataset(&dataset);
    let bars_sha = spectra_db::artifacts::sha256_hex(&encoded);
    let needed = i64::try_from(encoded.len()).unwrap_or(i64::MAX);
    match spectra_db::artifacts::quota(&env.pool, ws, &bars_sha, env.quota).await {
        Ok(Some(q)) if !q.present && q.used + needed > q.quota => {
            return failed(
                format!(
                    "The dataset needs {needed} bytes of storage, and this workspace has used {} of its {} bytes.",
                    q.used, q.quota
                ),
                false,
            );
        }
        Ok(Some(_)) => {}
        Ok(None) => return failed("The workspace no longer exists.", false),
        Err(e) => return failed(format!("Could not check the storage quota: {e}"), true),
    }
    let stored = match env.store.put(encoded).await {
        Ok(s) => s,
        Err(e) => return failed(format!("Could not store the dataset: {e}"), true),
    };
    let (data_dir, local) = (env.data_dir.clone(), dataset.clone());
    if let Ok(Err(e)) =
        tokio::task::spawn_blocking(move || DataStore::new(&data_dir).and_then(|s| s.put(&local))).await
    {
        // Not fatal: backtests fetch it from the artifact store.
        tracing::warn!(error = %e, "dataset not written to the local store");
    }
    set_progress(0.95);

    let quality = to_value(&spectra_data::quality_report_json(&dataset, &payload.options));
    let source = serde_json::json!({
        "upload_id": upload.id,
        "file_sha256": upload.sha256,
        "filename": upload.filename,
        "settings": payload.settings,
    });
    let registered = spectra_db::datasets::register_version(
        &env.pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: ws,
            dataset_name: &payload.dataset_name,
            checksum: &dataset.info.id,
            row_count: i64::try_from(dataset.bars.len()).unwrap_or(i64::MAX),
            byte_size: i64::try_from(stored.size).unwrap_or(i64::MAX),
            quality_report: quality.clone(),
            created_by: job.created_by,
            blob_sha256: Some(&stored.sha256),
            source: Some(source),
        },
    )
    .await;
    let version = match registered {
        Ok(v) => v,
        Err(e) => return failed(format!("Could not register the dataset version: {e}"), true),
    };
    if let Err(e) = spectra_db::uploads::mark_ingested(&env.pool, ws, upload.id).await {
        tracing::warn!(error = %e, upload_id = %upload.id, "upload not marked ingested");
    }
    let audit = spectra_db::uploads::audit(
        &env.pool,
        &spectra_db::uploads::AuditEvent {
            workspace_id: ws,
            actor_id: job.created_by,
            action: if version.existed { "dataset.version_unchanged" } else { "dataset.version_created" },
            target_type: "dataset_version",
            target_id: version.id.to_string(),
            request_id: &payload.request_id,
            details: serde_json::json!({
                "dataset_name": payload.dataset_name,
                "version": version.version,
                "checksum": dataset.info.id,
                "upload_id": upload.id,
                "filename": upload.filename,
                "job_id": job.id,
            }),
        },
    )
    .await;
    if let Err(e) = audit {
        tracing::error!(error = %e, "ingest not written to the audit log");
    }

    IngestOutcome::Registered(serde_json::json!({
        "dataset_version_id": version.id,
        "dataset_name": payload.dataset_name,
        "version": version.version,
        "existed": version.existed,
        "checksum": dataset.info.id,
        "rows": dataset.bars.len(),
        "symbols": dataset.symbols.symbols,
        "upload_id": upload.id,
        "quality_report": quality,
    }))
}

/// Makes sure `checksum` is in the local dataset store at `data_dir`,
/// copying it from the artifact store (`blob_sha256`) if needed. Both copies
/// are verified: the artifact by its SHA-256, the dataset by its id.
///
/// # Errors
/// A message for the job's error, with whether a retry could help.
pub async fn ensure_local(
    store: &ArtifactBackend,
    data_dir: &Path,
    checksum: &str,
    blob_sha256: Option<&str>,
) -> Result<(), (String, bool)> {
    let local = DataStore::new(data_dir).map_err(|e| (e.to_string(), true))?;
    if local.contains(checksum) {
        return Ok(());
    }
    let Some(blob) = blob_sha256 else {
        return Err((format!("Dataset {checksum} is not in this worker's dataset store."), false));
    };
    let raw = store.get(blob).await.map_err(|e| (format!("Could not read the dataset file: {e}"), true))?;
    let checksum = checksum.to_string();
    tokio::task::spawn_blocking(move || {
        let d =
            spectra_data::datastore::decode_dataset(&raw, &checksum).map_err(|e| (e.to_string(), false))?;
        local.put(&d).map(|_| ()).map_err(|e| (e.to_string(), true))
    })
    .await
    .map_err(|_| ("Copying the dataset crashed.".to_string(), true))?
}

/// A reader that hashes what passes through it.
struct Hashing<R> {
    inner: R,
    hash: spectra_core::Sha256,
}

impl<R: std::io::Read> std::io::Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
}
