//! Postgres-backed experiment registry.
//!
//! The normalized tables are projections for filtering and screens. Every
//! mutation appends the byte-compatible event payload in the same transaction;
//! per-workspace locking serializes writers, and [`PgRegistry::verify`] checks
//! the resulting chain with the file registry's verifier.

use std::fmt;

use spectra_core::{sha256_hex, Json};
use spectra_db::PgPool;
use spectra_engine::{config_fingerprint, Metrics};
use spectra_research::sharpe_per_period;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::registry::{
    config_to_json, event_hash, metrics_from_json, metrics_to_json, params_from_json, params_to_json,
    system_clock_ns, ArtifactInfo, Event, ExperimentInfo, RunComparison, RunFilter, RunInfo, RunInput,
    VerifyReport, GENESIS,
};

/// A database registry operation failed.
#[derive(Debug)]
pub enum PgRegistryError {
    /// Postgres rejected or could not complete the operation.
    Database(sqlx::Error),
    /// An event payload or hash chain was malformed.
    Integrity(String),
    /// A caller supplied an invalid registry value.
    Invalid(String),
    /// The shared registry JSON implementation rejected a value.
    Registry(spectra_core::Error),
}

impl fmt::Display for PgRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database registry operation failed: {error}"),
            Self::Integrity(message) | Self::Invalid(message) => f.write_str(message),
            Self::Registry(error) => write!(f, "registry operation failed: {error}"),
        }
    }
}

impl std::error::Error for PgRegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::Integrity(_) | Self::Invalid(_) => None,
        }
    }
}

impl From<sqlx::Error> for PgRegistryError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<spectra_core::Error> for PgRegistryError {
    fn from(error: spectra_core::Error) -> Self {
        Self::Registry(error)
    }
}

/// An artifact whose bytes are already in the artifact store, to be linked to a
/// run and recorded in the chain.
#[derive(Debug, Clone)]
pub struct StoredArtifact {
    /// e.g. `report`, `equity_curve`, `trades`, `returns`.
    pub kind: String,
    /// Lower-case hex SHA-256 of the bytes.
    pub sha256: String,
    /// Location in the artifact store.
    pub uri: String,
    /// Byte count.
    pub size_bytes: u64,
    /// MIME type.
    pub content_type: String,
    /// Small summary recorded in the event.
    pub summary: Json,
}

/// Paging and status options for [`PgRegistry::runs_page`].
#[derive(Debug, Clone, Default)]
pub struct RunPage {
    /// Only runs in this status (`queued`, `running`, `succeeded`, `failed`, `cancelled`).
    pub status: Option<String>,
    /// The `next` cursor from the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 through 1,000.
    pub limit: u32,
}

/// An artifact linked to a run.
#[derive(Debug, Clone)]
pub struct RunArtifact {
    /// Registry id.
    pub id: String,
    /// e.g. `report`, `equity_curve`.
    pub kind: String,
    /// SHA-256 of the bytes.
    pub sha256: String,
    /// Location in the artifact store.
    pub uri: String,
    /// Byte count (0 for artifacts imported from a file registry).
    pub size_bytes: u64,
    /// MIME type.
    pub content_type: String,
    /// Summary recorded in the event.
    pub summary: Json,
}

/// Run lifecycle statuses.
pub const RUN_STATUSES: [&str; 5] = ["queued", "running", "succeeded", "failed", "cancelled"];

/// Columns read by [`row_to_run`], in order.
const RUN_COLUMNS: &str = "COALESCE(r.registry_id, r.id::text), e.registry_id, r.kind, r.dataset_id, \
     r.dataset_bars_sha256, r.strategy, r.strategy_sha256, r.params::text, r.config::text, \
     r.config_sha256, r.result_sha256, r.engine_version, r.seed, r.bar_count, r.metrics::text, \
     r.sharpe_per_period, r.n_obs, r.counts_as_trial, \
     floor(extract(epoch FROM r.created_at) * 1000000000)::bigint, r.status, \
     COALESCE(r.job_id::text, ''), COALESCE(r.error, ''), \
     ARRAY(SELECT n.body FROM run_notes n WHERE n.workspace_id = r.workspace_id AND n.run_id = r.id \
           ORDER BY n.created_at, n.id)";
const RUN_COLUMN_COUNT: usize = 23;

fn parse_cursor(cursor: &str) -> Result<(i64, Uuid), PgRegistryError> {
    let invalid = || PgRegistryError::Invalid("invalid page cursor".into());
    let (micros, id) = cursor.split_once('~').ok_or_else(invalid)?;
    Ok((micros.parse().map_err(|_| invalid())?, Uuid::parse_str(id).map_err(|_| invalid())?))
}

/// A Postgres registry scoped to exactly one workspace.
#[derive(Debug, Clone)]
pub struct PgRegistry {
    pool: PgPool,
    workspace_id: Uuid,
}

// Every fallible method uses the public, documented error variants above.
#[allow(clippy::missing_errors_doc)]
impl PgRegistry {
    /// Create a workspace-scoped registry handle.
    pub fn new(pool: PgPool, workspace_id: Uuid) -> Self {
        Self { pool, workspace_id }
    }

    /// The workspace this registry handle can access.
    pub fn workspace_id(&self) -> Uuid {
        self.workspace_id
    }

    /// Create a root experiment or a child linked to `parent_id`.
    ///
    /// `parent_id` accepts the registry id exposed by this API or the UUID
    /// primary key used by internal jobs. The event payload always contains
    /// the stable registry id.
    pub async fn create_experiment(
        &self,
        name: &str,
        hypothesis: &str,
        tags: &[&str],
        parent_id: &str,
        created_by: Option<Uuid>,
    ) -> Result<String, PgRegistryError> {
        if name.trim().is_empty() {
            return Err(PgRegistryError::Invalid("experiment name must not be empty".into()));
        }
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let id = create_experiment_tx(
            &mut tx,
            self.workspace_id,
            name.trim(),
            hypothesis,
            tags,
            parent_id,
            created_by,
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// The workspace's default experiment ("Unsorted", tagged `default`),
    /// created on first use. Backtests started without an experiment go here.
    pub async fn default_experiment(&self, created_by: Option<Uuid>) -> Result<String, PgRegistryError> {
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT registry_id FROM experiments WHERE workspace_id = $1 AND name = $2 \
             AND tags @> ARRAY['default']::text[] AND parent_id IS NULL AND archived_at IS NULL \
             ORDER BY created_at, id LIMIT 1",
        )
        .bind(self.workspace_id)
        .bind(DEFAULT_EXPERIMENT_NAME)
        .fetch_optional(&mut *tx)
        .await?;
        let id = match existing {
            Some(id) => id,
            None => {
                create_experiment_tx(
                    &mut tx,
                    self.workspace_id,
                    DEFAULT_EXPERIMENT_NAME,
                    "Runs started without choosing an experiment.",
                    &["default"],
                    "",
                    created_by,
                )
                .await?
            }
        };
        tx.commit().await?;
        Ok(id)
    }

    /// Create a child experiment from an existing experiment.
    pub async fn fork_experiment(
        &self,
        parent_id: &str,
        name: &str,
        hypothesis: &str,
        tags: &[&str],
        created_by: Option<Uuid>,
    ) -> Result<String, PgRegistryError> {
        if parent_id.is_empty() {
            return Err(PgRegistryError::Invalid("fork_experiment requires a parent experiment id".into()));
        }
        self.create_experiment(name, hypothesis, tags, parent_id, created_by).await
    }

    /// List active experiments in this workspace, newest first.
    pub async fn experiments(&self) -> Result<Vec<ExperimentInfo>, PgRegistryError> {
        let rows = sqlx::query(
            "SELECT e.registry_id, e.name, e.hypothesis, e.tags, COALESCE(p.registry_id, ''), \
             floor(extract(epoch FROM e.created_at) * 1000000000)::bigint, \
             ARRAY(SELECT n.body FROM experiment_notes n WHERE n.workspace_id = e.workspace_id \
                   AND n.experiment_id = e.id ORDER BY n.created_at, n.id) \
             FROM experiments e LEFT JOIN experiments p \
               ON p.workspace_id = e.workspace_id AND p.id = e.parent_id \
             WHERE e.workspace_id = $1 AND e.archived_at IS NULL \
             ORDER BY e.created_at DESC, e.id DESC",
        )
        .bind(self.workspace_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ExperimentInfo {
                    id: row.try_get(0)?,
                    name: row.try_get(1)?,
                    hypothesis: row.try_get(2)?,
                    tags: row.try_get(3)?,
                    parent_id: row.try_get(4)?,
                    created_ts: row.try_get(5)?,
                    notes: row.try_get(6)?,
                })
            })
            .collect()
    }

    /// Fetch one experiment by registry id or database UUID.
    pub async fn experiment(&self, id: &str) -> Result<Option<ExperimentInfo>, PgRegistryError> {
        let row = sqlx::query(
            "SELECT e.registry_id, e.name, e.hypothesis, e.tags, COALESCE(p.registry_id, ''), \
             floor(extract(epoch FROM e.created_at) * 1000000000)::bigint, \
             ARRAY(SELECT n.body FROM experiment_notes n WHERE n.workspace_id = e.workspace_id \
                   AND n.experiment_id = e.id ORDER BY n.created_at, n.id) \
             FROM experiments e LEFT JOIN experiments p \
               ON p.workspace_id = e.workspace_id AND p.id = e.parent_id \
             WHERE e.workspace_id = $1 AND (e.registry_id = $2 OR e.id::text = $2)",
        )
        .bind(self.workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ExperimentInfo {
                id: row.try_get(0)?,
                name: row.try_get(1)?,
                hypothesis: row.try_get(2)?,
                tags: row.try_get(3)?,
                parent_id: row.try_get(4)?,
                created_ts: row.try_get(5)?,
                notes: row.try_get(6)?,
            })
        })
        .transpose()
    }

    /// Root-first experiment lineage, bounded to prevent cycles in corrupted data.
    pub async fn lineage(&self, id: &str) -> Result<Vec<String>, PgRegistryError> {
        let mut lineage = Vec::new();
        let mut current = id.to_string();
        for _ in 0..10_000 {
            let experiment = self
                .experiment(&current)
                .await?
                .ok_or_else(|| PgRegistryError::Invalid(format!("unknown experiment: {current}")))?;
            lineage.push(experiment.id);
            if experiment.parent_id.is_empty() {
                lineage.reverse();
                return Ok(lineage);
            }
            current = experiment.parent_id;
        }
        Err(PgRegistryError::Integrity("experiment lineage exceeds 10,000 ancestors".into()))
    }

    /// Insert a queued run before starting its associated job.
    ///
    /// The returned UUID is the database primary key. The public registry id
    /// is assigned only when the successful run event is committed.
    pub async fn create_queued_run(
        &self,
        input: &RunInput,
        job_id: Option<Uuid>,
        created_by: Option<Uuid>,
    ) -> Result<Uuid, PgRegistryError> {
        validate_run_input(input)?;
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let experiment_id =
            find_experiment_id(&mut tx, self.workspace_id, &input.experiment_id).await?.ok_or_else(|| {
                PgRegistryError::Invalid(format!("unknown experiment: {}", input.experiment_id))
            })?;
        let id = spectra_db::new_id();
        let params = params_to_json(&input.params)?.dump();
        let config = config_to_json(&input.config)?.dump();
        let seed = i64::try_from(input.run.seed).map_err(|_| {
            PgRegistryError::Invalid("seed exceeds the database's signed 64-bit range".into())
        })?;
        sqlx::query(
            "INSERT INTO runs (id, workspace_id, experiment_id, job_id, kind, status, dataset_id, strategy, \
             params, config, seed, engine_version, created_by) \
             VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8::jsonb, $9::jsonb, $10, $11, $12)",
        )
        .bind(id)
        .bind(self.workspace_id)
        .bind(experiment_id)
        .bind(job_id)
        .bind(&input.kind)
        .bind(&input.dataset_id)
        .bind(&input.strategy)
        .bind(params)
        .bind(config)
        .bind(seed)
        .bind(&input.run.engine_version)
        .bind(created_by)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Atomically queue a backtest job and its linked queued run row.
    pub async fn enqueue_run(
        &self,
        input: &RunInput,
        job: &spectra_db::jobs::NewJob,
    ) -> Result<(spectra_db::jobs::Job, Uuid), PgRegistryError> {
        validate_run_input(input)?;
        if job.workspace_id != self.workspace_id || job.kind != "backtest" {
            return Err(PgRegistryError::Invalid(
                "registry runs require a backtest job in the same workspace".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let id = spectra_db::new_id();
        let mut job_payload = job.payload.clone();
        let Some(object) = job_payload.as_object_mut() else {
            return Err(PgRegistryError::Invalid("backtest payload must be a JSON object".into()));
        };
        object.insert("registry_run_id".into(), sqlx::types::JsonValue::String(id.to_string()));
        let experiment_id =
            find_experiment_id(&mut tx, self.workspace_id, &input.experiment_id).await?.ok_or_else(|| {
                PgRegistryError::Invalid(format!("unknown experiment: {}", input.experiment_id))
            })?;
        let job_row = sqlx::query_as::<_, spectra_db::jobs::Job>(
            "WITH j AS (\
                 INSERT INTO jobs (workspace_id, kind, payload, priority, max_attempts, created_by) \
                 VALUES ($1, $2, $3, $4, $5, $6) RETURNING *\
             ), ev AS (\
                 INSERT INTO job_events (job_id, workspace_id, kind) SELECT id, workspace_id, 'queued' FROM j\
             ) SELECT j.* FROM j",
        )
        .bind(job.workspace_id)
        .bind(&job.kind)
        .bind(&job_payload)
        .bind(job.priority)
        .bind(job.max_attempts)
        .bind(job.created_by)
        .fetch_one(&mut *tx)
        .await?;
        let params = params_to_json(&input.params)?.dump();
        let config = config_to_json(&input.config)?.dump();
        let seed = i64::try_from(input.run.seed).map_err(|_| {
            PgRegistryError::Invalid("seed exceeds the database's signed 64-bit range".into())
        })?;
        sqlx::query(
            "INSERT INTO runs (id, workspace_id, experiment_id, job_id, kind, status, dataset_id, strategy, \
             params, config, seed, engine_version, created_by) \
             VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8::jsonb, $9::jsonb, $10, $11, $12)",
        )
        .bind(id)
        .bind(self.workspace_id)
        .bind(experiment_id)
        .bind(job_row.id)
        .bind(&input.kind)
        .bind(&input.dataset_id)
        .bind(&input.strategy)
        .bind(params)
        .bind(config)
        .bind(seed)
        .bind(&input.run.engine_version)
        .bind(job.created_by)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((job_row, id))
    }

    /// Transition a queued run to running. Repeated or cross-workspace starts fail.
    pub async fn start_run(&self, run_id: Uuid) -> Result<(), PgRegistryError> {
        let result = sqlx::query(
            "UPDATE runs SET status = 'running', started_at = now() \
             WHERE workspace_id = $1 AND id = $2 AND status = 'queued'",
        )
        .bind(self.workspace_id)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            let status: Option<String> =
                sqlx::query_scalar("SELECT status FROM runs WHERE workspace_id = $1 AND id = $2")
                    .bind(self.workspace_id)
                    .bind(run_id)
                    .fetch_optional(&self.pool)
                    .await?;
            if status.as_deref() != Some("running") {
                return Err(PgRegistryError::Invalid(format!(
                    "run {run_id} is missing or cannot start from its current status"
                )));
            }
        }
        Ok(())
    }

    /// Finalize a running run and append its event atomically.
    pub async fn complete_run(&self, run_id: Uuid, input: &RunInput) -> Result<String, PgRegistryError> {
        self.complete_run_with_artifacts(run_id, input, &[]).await
    }

    /// Finalize a running run, link its stored artifacts and append the
    /// `run.recorded` event followed by one `artifact.recorded` event per
    /// artifact, all in one transaction: either the run is recorded with every
    /// artifact or nothing is.
    pub async fn complete_run_with_artifacts(
        &self,
        run_id: Uuid,
        input: &RunInput,
        artifacts: &[StoredArtifact],
    ) -> Result<String, PgRegistryError> {
        validate_run_input(input)?;
        for artifact in artifacts {
            validate_stored_artifact(artifact)?;
        }
        let params = params_to_json(&input.params)?;
        let config = config_to_json(&input.config)?;
        let metrics = metrics_to_json(&input.metrics)?;
        let sharpe_period = sharpe_per_period(&input.returns);
        if !sharpe_period.is_finite() {
            return Err(PgRegistryError::Invalid("non-finite per-period Sharpe cannot be recorded".into()));
        }
        let seed = i64::try_from(input.run.seed).map_err(|_| {
            PgRegistryError::Invalid("seed exceeds the database's signed 64-bit range".into())
        })?;
        let bars = i64::try_from(input.run.bar_count)
            .map_err(|_| PgRegistryError::Invalid("bar count exceeds the database range".into()))?;
        let n_obs = i64::try_from(input.returns.len())
            .map_err(|_| PgRegistryError::Invalid("observation count exceeds the database range".into()))?;
        let exp_registry_id: String;
        let registry_id: String;
        let ts_ns = (system_clock_ns())();
        let payload: Json;
        {
            let mut tx = self.pool.begin().await?;
            lock_workspace(&mut tx, self.workspace_id).await?;
            let run = sqlx::query(
                "SELECT r.experiment_id, e.registry_id, r.status FROM runs r \
                 JOIN experiments e ON e.workspace_id = r.workspace_id AND e.id = r.experiment_id \
                 WHERE r.workspace_id = $1 AND r.id = $2 FOR UPDATE OF r",
            )
            .bind(self.workspace_id)
            .bind(run_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| PgRegistryError::Invalid(format!("unknown run: {run_id}")))?;
            exp_registry_id = run.try_get(1)?;
            let status: String = run.try_get(2)?;
            if status != "running" {
                return Err(PgRegistryError::Invalid(format!("run {run_id} is not running")));
            }
            registry_id = run_id.to_string();
            let run_db_experiment_id: Uuid = run.try_get(0)?;
            let expected_experiment_id =
                find_experiment_id(&mut tx, self.workspace_id, &input.experiment_id).await?.ok_or_else(
                    || PgRegistryError::Invalid(format!("unknown experiment: {}", input.experiment_id)),
                )?;
            if run_db_experiment_id != expected_experiment_id {
                return Err(PgRegistryError::Invalid("run belongs to a different experiment".into()));
            }
            payload = Json::object()
                .with("id", Json::string(registry_id.clone()))
                .with("experiment_id", Json::string(exp_registry_id.clone()))
                .with("kind", Json::string(input.kind.clone()))
                .with("dataset_id", Json::string(input.dataset_id.clone()))
                .with("dataset_bars_sha256", Json::string(input.run.dataset_sha256.clone()))
                .with("strategy", Json::string(input.strategy.clone()))
                .with("strategy_sha256", Json::string(input.strategy_sha256.clone()))
                .with("params", params.clone())
                .with("config", config.clone())
                .with("config_sha256", Json::string(input.run.config_sha256.clone()))
                .with("result_sha256", Json::string(input.run.result_sha256.clone()))
                .with("engine_version", Json::string(input.run.engine_version.clone()))
                .with("seed", Json::string(input.run.seed.to_string()))
                .with("bar_count", Json::integer(bars))
                .with("metrics", metrics.clone())
                .with("sharpe_per_period", Json::number(sharpe_period)?)
                .with("n_obs", Json::integer(n_obs))
                .with("counts_as_trial", Json::boolean(input.counts_as_trial))
                .with("note", Json::string(input.note.clone()));

            sqlx::query(
                "UPDATE runs SET status = 'succeeded', finished_at = now(), dataset_bars_sha256 = $3, \
                 strategy = $4, strategy_sha256 = $5, params = $6::jsonb, config = $7::jsonb, \
                 config_sha256 = $8, result_sha256 = $9, engine_version = $10, seed = $11, \
                 bar_count = $12, n_obs = $13, sharpe_per_period = $14, metrics = $15::jsonb, \
                 counts_as_trial = $16, registry_id = $17 \
                 WHERE workspace_id = $1 AND id = $2 AND status = 'running'",
            )
            .bind(self.workspace_id)
            .bind(run_id)
            .bind(&input.run.dataset_sha256)
            .bind(&input.strategy)
            .bind(&input.strategy_sha256)
            .bind(params.dump())
            .bind(config.dump())
            .bind(input.run.config_sha256.clone())
            .bind(input.run.result_sha256.clone())
            .bind(&input.run.engine_version)
            .bind(seed)
            .bind(bars)
            .bind(n_obs)
            .bind(sharpe_period)
            .bind(metrics.dump())
            .bind(input.counts_as_trial)
            .bind(&registry_id)
            .execute(&mut *tx)
            .await?;
            append_event(&mut tx, self.workspace_id, "run.recorded", ts_ns, payload.clone()).await?;
            for artifact in artifacts {
                link_artifact(
                    &mut tx,
                    self.workspace_id,
                    run_id,
                    &exp_registry_id,
                    &registry_id,
                    artifact,
                    ts_ns,
                )
                .await?;
            }
            tx.commit().await?;
        }
        Ok(registry_id)
    }

    /// Record a run whose computation has already completed in memory.
    pub async fn record_run(
        &self,
        input: &RunInput,
        created_by: Option<Uuid>,
    ) -> Result<String, PgRegistryError> {
        let run_id = self.create_queued_run(input, None, created_by).await?;
        self.start_run(run_id).await?;
        self.complete_run(run_id, input).await
    }

    /// Mark an unfinished run failed. Failures do not create `run.recorded` events.
    pub async fn fail_run(&self, run_id: Uuid, message: &str) -> Result<(), PgRegistryError> {
        let result = sqlx::query(
            "UPDATE runs SET status = 'failed', error = $3, finished_at = now() \
             WHERE workspace_id = $1 AND id = $2 AND status IN ('queued', 'running')",
        )
        .bind(self.workspace_id)
        .bind(run_id)
        .bind(message)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(PgRegistryError::Invalid(format!("run {run_id} is missing or already finished")));
        }
        Ok(())
    }

    /// Mark an unfinished run cancelled. Cancellations do not create `run.recorded` events.
    pub async fn cancel_run(&self, run_id: Uuid) -> Result<(), PgRegistryError> {
        let result = sqlx::query(
            "UPDATE runs SET status = 'cancelled', finished_at = now() \
             WHERE workspace_id = $1 AND id = $2 AND status IN ('queued', 'running')",
        )
        .bind(self.workspace_id)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(PgRegistryError::Invalid(format!("run {run_id} is missing or already finished")));
        }
        Ok(())
    }

    /// List runs using the file registry's filters. Results are newest first
    /// (at most 1,000; use [`PgRegistry::runs_page`] to page through more).
    pub async fn runs(&self, filter: &RunFilter) -> Result<Vec<RunInfo>, PgRegistryError> {
        Ok(self.runs_page(filter, &RunPage { limit: 1000, ..RunPage::default() }).await?.0)
    }

    /// One page of runs, newest first, plus the cursor for the next page
    /// (`None` on the last page).
    pub async fn runs_page(
        &self,
        filter: &RunFilter,
        page: &RunPage,
    ) -> Result<(Vec<RunInfo>, Option<String>), PgRegistryError> {
        if filter.min_sharpe.is_some_and(|value| !value.is_finite()) {
            return Err(PgRegistryError::Invalid("minimum Sharpe must be finite".into()));
        }
        if !(1..=1000).contains(&page.limit) {
            return Err(PgRegistryError::Invalid("page size must be between 1 and 1000".into()));
        }
        if let Some(status) = page.status.as_deref() {
            if !RUN_STATUSES.contains(&status) {
                return Err(PgRegistryError::Invalid(format!("unknown run status: {status}")));
            }
        }
        let cursor = page.cursor.as_deref().map(parse_cursor).transpose()?;
        let experiment_id = if let Some(id) = filter.experiment_id.as_deref() {
            let found: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM experiments WHERE workspace_id = $1 AND (registry_id = $2 OR id::text = $2)",
            )
            .bind(self.workspace_id)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
            found
        } else {
            None
        };
        if filter.experiment_id.is_some() && experiment_id.is_none() {
            return Ok((Vec::new(), None));
        }
        let created_by = filter
            .created_by
            .as_deref()
            .map(|s| {
                Uuid::parse_str(s)
                    .map_err(|_| PgRegistryError::Invalid(format!("invalid created_by UUID: {s}")))
            })
            .transpose()?;
        let hidden_for = filter
            .hidden_for
            .as_deref()
            .map(|s| {
                Uuid::parse_str(s)
                    .map_err(|_| PgRegistryError::Invalid(format!("invalid hidden_for UUID: {s}")))
            })
            .transpose()?;
        let rows = sqlx::query(&format!(
            "SELECT {RUN_COLUMNS}, (extract(epoch FROM r.created_at) * 1000000)::bigint, r.id \
             FROM runs r LEFT JOIN experiments e ON e.workspace_id = r.workspace_id AND e.id = r.experiment_id \
             WHERE r.workspace_id = $1 AND ($2::uuid IS NULL OR r.experiment_id = $2) \
               AND ($3::text IS NULL OR r.dataset_id = $3) \
               AND ($4::text IS NULL OR r.strategy = $4) \
               AND ($5::text IS NULL OR r.kind = $5) \
               AND (NOT $6 OR r.counts_as_trial) \
               AND ($7::float8 IS NULL OR COALESCE((r.metrics->>'sharpe')::float8, -1.0e300) >= $7) \
               AND ($8::text IS NULL OR r.status = $8) \
               AND ($9::uuid IS NULL OR r.created_by = $9) \
               AND ($10::uuid IS NULL OR NOT EXISTS ( \
                    SELECT 1 FROM user_run_hidden h \
                    WHERE h.workspace_id = r.workspace_id AND h.user_id = $10 AND h.run_id = r.id \
               )) \
               AND ($11::bigint IS NULL OR (r.created_at, r.id) < \
                    (timestamptz 'epoch' + $11 * interval '1 microsecond', $12)) \
             ORDER BY r.created_at DESC, r.id DESC LIMIT $13"
        ))
        .bind(self.workspace_id)
        .bind(experiment_id)
        .bind(filter.dataset_id.as_deref())
        .bind(filter.strategy.as_deref())
        .bind(filter.kind.as_deref())
        .bind(filter.only_trials)
        .bind(filter.min_sharpe)
        .bind(page.status.as_deref())
        .bind(created_by)
        .bind(hidden_for)
        .bind(cursor.map(|(micros, _)| micros))
        .bind(cursor.map(|(_, id)| id))
        .bind(i64::from(page.limit) + 1)
        .fetch_all(&self.pool)
        .await?;
        let limit = usize::try_from(page.limit).unwrap_or(usize::MAX);
        let next = if rows.len() > limit {
            let last = &rows[limit - 1];
            let micros: i64 = last.try_get(RUN_COLUMN_COUNT)?;
            let id: Uuid = last.try_get(RUN_COLUMN_COUNT + 1)?;
            Some(format!("{micros}~{id}"))
        } else {
            None
        };
        let runs = rows.iter().take(limit).map(row_to_run).collect::<Result<Vec<_>, _>>()?;
        Ok((runs, next))
    }

    /// Run counts per experiment registry id: `(all runs, succeeded runs)`.
    pub async fn run_counts(&self) -> Result<std::collections::HashMap<String, (u64, u64)>, PgRegistryError> {
        let rows = sqlx::query(
            "SELECT e.registry_id, count(r.id), count(r.id) FILTER (WHERE r.status = 'succeeded') \
             FROM experiments e LEFT JOIN runs r ON r.workspace_id = e.workspace_id AND r.experiment_id = e.id \
             WHERE e.workspace_id = $1 GROUP BY e.registry_id",
        )
        .bind(self.workspace_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let all: i64 = row.try_get(1)?;
                let ok: i64 = row.try_get(2)?;
                Ok((row.try_get(0)?, (u64::try_from(all).unwrap_or(0), u64::try_from(ok).unwrap_or(0))))
            })
            .collect()
    }

    /// Artifacts attached to one run, oldest first.
    pub async fn run_artifacts(&self, run_id: &str) -> Result<Vec<RunArtifact>, PgRegistryError> {
        let rows = sqlx::query(
            "SELECT a.registry_id, a.kind, a.sha256, a.path, a.size_bytes, a.content_type, a.summary::text \
             FROM run_artifacts a JOIN runs r ON r.workspace_id = a.workspace_id AND r.id = a.run_id \
             WHERE a.workspace_id = $1 AND (r.registry_id = $2 OR r.id::text = $2) \
             ORDER BY a.created_at, a.id",
        )
        .bind(self.workspace_id)
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let summary: String = row.try_get(6)?;
                Ok(RunArtifact {
                    id: row.try_get(0)?,
                    kind: row.try_get(1)?,
                    sha256: row.try_get(2)?,
                    uri: row.try_get(3)?,
                    size_bytes: u64::try_from(row.try_get::<i64, _>(4)?).unwrap_or(0),
                    content_type: row.try_get(5)?,
                    summary: Json::parse(&summary).map_err(PgRegistryError::Registry)?,
                })
            })
            .collect()
    }

    /// Fetch a run by registry id or UUID primary key.
    pub async fn run(&self, id: &str) -> Result<Option<RunInfo>, PgRegistryError> {
        let row = sqlx::query(&format!(
            "SELECT {RUN_COLUMNS} \
             FROM runs r LEFT JOIN experiments e ON e.workspace_id = r.workspace_id AND e.id = r.experiment_id \
             WHERE r.workspace_id = $1 AND (r.registry_id = $2 OR r.id::text = $2)"
        ))
        .bind(self.workspace_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_run(&row)).transpose()
    }

    /// Hide a run from a specific user's personal run list.
    ///
    /// The run record remains untouched in the registry. Hiding is idempotent.
    pub async fn hide_run_for_user(&self, run_id: &str, user_id: Uuid) -> Result<(), PgRegistryError> {
        let run_uuid = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM runs WHERE workspace_id = $1 AND (registry_id = $2 OR id::text = $2)",
        )
        .bind(self.workspace_id)
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(run_uuid) = run_uuid else {
            return Err(PgRegistryError::Invalid(format!("run {run_id} not found")));
        };
        sqlx::query(
            "INSERT INTO user_run_hidden (workspace_id, user_id, run_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(self.workspace_id)
        .bind(user_id)
        .bind(run_uuid)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// List artifacts attached to an experiment.
    pub async fn artifacts(&self, experiment_id: &str) -> Result<Vec<ArtifactInfo>, PgRegistryError> {
        let experiment = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM experiments WHERE workspace_id = $1 AND (registry_id = $2 OR id::text = $2)",
        )
        .bind(self.workspace_id)
        .bind(experiment_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(experiment) = experiment else { return Ok(Vec::new()) };
        let rows = sqlx::query(
            "SELECT a.registry_id, e.registry_id, COALESCE(r.registry_id, r.id::text), a.kind, \
             a.sha256, a.path, a.summary::text FROM run_artifacts a \
             JOIN runs r ON r.workspace_id = a.workspace_id AND r.id = a.run_id \
             JOIN experiments e ON e.workspace_id = r.workspace_id AND e.id = r.experiment_id \
             WHERE a.workspace_id = $1 AND e.id = $2 ORDER BY a.created_at, a.id",
        )
        .bind(self.workspace_id)
        .bind(experiment)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let summary: String = row.try_get(6)?;
                Ok(ArtifactInfo {
                    id: row.try_get(0)?,
                    experiment_id: row.try_get(1)?,
                    run_id: row.try_get(2)?,
                    kind: row.try_get(3)?,
                    sha256: row.try_get(4)?,
                    uri: row.try_get(5)?,
                    summary: Json::parse(&summary).map_err(PgRegistryError::Registry)?,
                })
            })
            .collect()
    }

    /// Number of completed runs that count as statistical trials.
    pub async fn trial_count(&self, experiment_id: Option<&str>) -> Result<usize, PgRegistryError> {
        let experiment = if let Some(id) = experiment_id {
            Some(
                sqlx::query_scalar::<_, Uuid>(
                    "SELECT id FROM experiments WHERE workspace_id = $1 AND (registry_id = $2 OR id::text = $2)",
                )
                .bind(self.workspace_id)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| PgRegistryError::Invalid(format!("unknown experiment: {id}")))?,
            )
        } else {
            None
        };
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM runs WHERE workspace_id = $1 \
             AND counts_as_trial AND ($2::uuid IS NULL OR experiment_id = $2)",
        )
        .bind(self.workspace_id)
        .bind(experiment)
        .fetch_one(&self.pool)
        .await?;
        usize::try_from(count).map_err(|_| PgRegistryError::Integrity("negative trial count".into()))
    }

    /// Compare two completed runs by their stored fingerprints and metrics.
    pub async fn compare(&self, left_id: &str, right_id: &str) -> Result<RunComparison, PgRegistryError> {
        let left = self
            .run(left_id)
            .await?
            .ok_or_else(|| PgRegistryError::Invalid(format!("unknown run: {left_id}")))?;
        let right = self
            .run(right_id)
            .await?
            .ok_or_else(|| PgRegistryError::Invalid(format!("unknown run: {right_id}")))?;
        let mut differences = Vec::new();
        let same_dataset = left.dataset_bars_sha256 == right.dataset_bars_sha256;
        let same_strategy_source =
            left.strategy == right.strategy && left.strategy_sha256 == right.strategy_sha256;
        let same_params = left.params == right.params;
        let same_config = left.config_sha256 == right.config_sha256;
        let same_engine = left.engine_version == right.engine_version;
        let same_result = left.result_sha256 == right.result_sha256;
        for (same, label) in [
            (same_dataset, "dataset"),
            (same_strategy_source, "strategy source"),
            (same_params, "parameters"),
            (same_config, "configuration"),
            (same_engine, "engine version"),
            (same_result, "result"),
        ] {
            if !same {
                differences.push(format!("different {label}"));
            }
        }
        Ok(RunComparison {
            same_dataset,
            same_strategy_source,
            same_params,
            same_config,
            same_engine,
            same_result,
            sharpe_delta: right.metrics.sharpe - left.metrics.sharpe,
            total_return_delta: right.metrics.total_return - left.metrics.total_return,
            differences,
        })
    }

    /// Record an artifact reference and event atomically after verifying its bytes' SHA-256.
    pub async fn record_artifact(
        &self,
        experiment_id: &str,
        run_id: &str,
        kind: &str,
        bytes: &[u8],
        uri: &str,
        summary: Json,
    ) -> Result<String, PgRegistryError> {
        if kind.trim().is_empty() || uri.trim().is_empty() {
            return Err(PgRegistryError::Invalid("artifact kind and URI must not be empty".into()));
        }
        let artifact = StoredArtifact {
            kind: kind.to_owned(),
            sha256: sha256_hex(bytes),
            uri: uri.to_owned(),
            size_bytes: bytes.len() as u64,
            content_type: "application/octet-stream".into(),
            summary,
        };
        validate_stored_artifact(&artifact)?;
        let ts_ns = (system_clock_ns())();
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let experiment_db_id = find_experiment_id(&mut tx, self.workspace_id, experiment_id)
            .await?
            .ok_or_else(|| PgRegistryError::Invalid(format!("unknown experiment: {experiment_id}")))?;
        let run_db_id = find_run_id(&mut tx, self.workspace_id, run_id)
            .await?
            .ok_or_else(|| PgRegistryError::Invalid(format!("unknown run: {run_id}")))?;
        let belongs: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id = $1 AND id = $2 AND experiment_id = $3)",
        )
        .bind(self.workspace_id)
        .bind(run_db_id)
        .bind(experiment_db_id)
        .fetch_one(&mut *tx)
        .await?;
        if !belongs {
            return Err(PgRegistryError::Invalid("run belongs to a different experiment".into()));
        }
        let registry_id =
            link_artifact(&mut tx, self.workspace_id, run_db_id, experiment_id, run_id, &artifact, ts_ns)
                .await?;
        tx.commit().await?;
        Ok(registry_id)
    }

    /// Append a note to an experiment or run; the target itself remains immutable.
    pub async fn add_note(
        &self,
        target_id: &str,
        text: &str,
        author_id: Option<Uuid>,
    ) -> Result<(), PgRegistryError> {
        if text.trim().is_empty() {
            return Err(PgRegistryError::Invalid("note must not be empty".into()));
        }
        let ts_ns = (system_clock_ns())();
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let experiment = find_experiment_id(&mut tx, self.workspace_id, target_id).await?;
        let run = if experiment.is_none() {
            find_run_id(&mut tx, self.workspace_id, target_id).await?
        } else {
            None
        };
        if let Some(id) = experiment {
            sqlx::query("INSERT INTO experiment_notes (workspace_id, experiment_id, author_id, body) VALUES ($1, $2, $3, $4)")
                .bind(self.workspace_id)
                .bind(id)
                .bind(author_id)
                .bind(text)
                .execute(&mut *tx)
                .await?;
        } else if let Some(id) = run {
            sqlx::query(
                "INSERT INTO run_notes (workspace_id, run_id, author_id, body) VALUES ($1, $2, $3, $4)",
            )
            .bind(self.workspace_id)
            .bind(id)
            .bind(author_id)
            .bind(text)
            .execute(&mut *tx)
            .await?;
        } else {
            return Err(PgRegistryError::Invalid(format!("unknown note target: {target_id}")));
        }
        let payload =
            Json::object().with("target_id", Json::string(target_id)).with("text", Json::string(text));
        append_event(&mut tx, self.workspace_id, "note.added", ts_ns, payload).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Append tamper-evident evidence that a survival report was saved.
    pub async fn record_survival_report_started(
        &self,
        report_id: Uuid,
        run_id: Uuid,
        actor_id: Option<Uuid>,
    ) -> Result<(), PgRegistryError> {
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM survival_reports WHERE workspace_id=$1 AND id=$2 AND run_id=$3)",
        )
        .bind(self.workspace_id)
        .bind(report_id)
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(PgRegistryError::Invalid("survival report is not present".into()));
        }
        let payload = Json::object()
            .with("report_id", Json::string(report_id.to_string()))
            .with("run_id", Json::string(run_id.to_string()))
            .with("actor_id", actor_id.map_or(Json::Null, |id| Json::string(id.to_string())));
        append_event(&mut tx, self.workspace_id, "survival.started", (system_clock_ns())(), payload).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Append tamper-evident evidence that a survival report was saved.
    pub async fn record_survival_report_saved(
        &self,
        report_id: Uuid,
        run_id: Uuid,
        verdict: &str,
    ) -> Result<(), PgRegistryError> {
        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM survival_reports WHERE workspace_id=$1 AND id=$2 AND run_id=$3 AND status='succeeded')",
        )
        .bind(self.workspace_id)
        .bind(report_id)
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(PgRegistryError::Invalid("survival report is not saved".into()));
        }
        let payload = Json::object()
            .with("report_id", Json::string(report_id.to_string()))
            .with("run_id", Json::string(run_id.to_string()))
            .with("worst_verdict", Json::string(verdict));
        append_event(&mut tx, self.workspace_id, "survival.saved", (system_clock_ns())(), payload).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Read and verify the workspace event chain.
    pub async fn verify(&self) -> Result<VerifyReport, PgRegistryError> {
        let events = self.events().await?;
        let mut previous = GENESIS.to_string();
        for (index, event) in events.iter().enumerate() {
            if event.seq != index as u64 {
                return Ok(VerifyReport {
                    ok: false,
                    events: index,
                    last_hash: previous,
                    detail: format!("unexpected sequence number at event {index}"),
                });
            }
            if event.prev != previous {
                return Ok(VerifyReport {
                    ok: false,
                    events: index,
                    last_hash: previous,
                    detail: format!("previous-hash mismatch at event {index}"),
                });
            }
            let expected = event_hash(&event.prev, event.seq, &event.kind, event.ts_ns, &event.payload);
            if event.hash != expected {
                return Ok(VerifyReport {
                    ok: false,
                    events: index,
                    last_hash: previous,
                    detail: format!("event content does not match its hash at event {index}"),
                });
            }
            previous.clone_from(&event.hash);
        }
        Ok(VerifyReport { ok: true, events: events.len(), last_hash: previous, detail: String::new() })
    }

    /// Export the exact event lines used by the file registry and C++ reference.
    pub async fn export_jsonl(&self) -> Result<String, PgRegistryError> {
        let mut output = String::new();
        for event in self.events().await? {
            output.push_str(
                &Json::object()
                    .with("seq", Json::integer(event.seq as i64))
                    .with("type", Json::string(event.kind.clone()))
                    .with("ts_ns", Json::integer(event.ts_ns))
                    .with("payload", event.payload.clone())
                    .with("prev", Json::string(event.prev.clone()))
                    .with("hash", Json::string(event.hash.clone()))
                    .dump(),
            );
            output.push('\n');
        }
        Ok(output)
    }

    /// Import a verified JSONL chain into an empty workspace in one transaction.
    ///
    /// The original event sequence, payloads and hashes are preserved. The
    /// queryable tables are rebuilt from supported registry events as part of
    /// the same transaction. Imports are deliberately restricted to empty
    /// workspaces so an import can never silently overwrite or interleave a
    /// workspace's history.
    pub async fn import_jsonl(&self, input: &str) -> Result<usize, PgRegistryError> {
        const MAX_IMPORT_BYTES: usize = 50 * 1024 * 1024;
        const MAX_IMPORT_EVENTS: usize = 100_000;
        if input.len() > MAX_IMPORT_BYTES {
            return Err(PgRegistryError::Invalid("registry import exceeds the 50 MiB limit".into()));
        }
        if input.is_empty() || !input.ends_with('\n') {
            return Err(PgRegistryError::Invalid(
                "registry import must contain complete newline-terminated events".into(),
            ));
        }
        let mut events = Vec::new();
        let mut previous = GENESIS.to_string();
        for (index, line) in input.lines().enumerate() {
            if events.len() >= MAX_IMPORT_EVENTS {
                return Err(PgRegistryError::Invalid(
                    "registry import exceeds the 100,000 event limit".into(),
                ));
            }
            let value = Json::parse(line)
                .map_err(|e| PgRegistryError::Invalid(format!("invalid event at line {}: {e}", index + 1)))?;
            let seq = u64::try_from(value.at("seq")?.as_int()?).map_err(|_| {
                PgRegistryError::Integrity(format!("negative sequence at line {}", index + 1))
            })?;
            if seq != index as u64 {
                return Err(PgRegistryError::Integrity(format!(
                    "unexpected sequence number at line {}",
                    index + 1
                )));
            }
            let kind = value.at("type")?.as_str()?.to_owned();
            let ts_ns = value.at("ts_ns")?.as_int()?;
            let payload = value.at("payload")?.clone();
            let prev = value.at("prev")?.as_str()?.to_owned();
            let hash = value.at("hash")?.as_str()?.to_owned();
            if prev != previous {
                return Err(PgRegistryError::Integrity(format!(
                    "previous-hash mismatch at line {}",
                    index + 1
                )));
            }
            let expected = event_hash(&prev, seq, &kind, ts_ns, &payload);
            if hash != expected {
                return Err(PgRegistryError::Integrity(format!("event hash mismatch at line {}", index + 1)));
            }
            previous.clone_from(&hash);
            events.push(Event { seq, kind, ts_ns, payload, prev, hash });
        }
        if events.is_empty() {
            return Err(PgRegistryError::Invalid("registry import contains no events".into()));
        }

        let mut tx = self.pool.begin().await?;
        lock_workspace(&mut tx, self.workspace_id).await?;
        let existing: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM registry_events WHERE workspace_id = $1) OR \
             EXISTS (SELECT 1 FROM experiments WHERE workspace_id = $1) OR \
             EXISTS (SELECT 1 FROM runs WHERE workspace_id = $1)",
        )
        .bind(self.workspace_id)
        .fetch_one(&mut *tx)
        .await?;
        if existing {
            return Err(PgRegistryError::Invalid("registry import requires an empty workspace".into()));
        }

        let mut experiments = std::collections::HashMap::<String, Uuid>::new();
        let mut runs = std::collections::HashMap::<String, Uuid>::new();
        for event in &events {
            project_imported_event(&mut tx, self.workspace_id, event, &mut experiments, &mut runs).await?;
            sqlx::query(
                "INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash) \
                 VALUES ($1, $2, $3, $4, $5::json, $6, $7)",
            )
            .bind(self.workspace_id)
            .bind(
                i64::try_from(event.seq)
                    .map_err(|_| PgRegistryError::Integrity("registry sequence overflow".into()))?,
            )
            .bind(&event.kind)
            .bind(event.ts_ns)
            .bind(event.payload.dump())
            .bind(&event.prev)
            .bind(&event.hash)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(events.len())
    }

    /// Return all events in append order.
    pub async fn events(&self) -> Result<Vec<Event>, PgRegistryError> {
        let rows = sqlx::query(
            "SELECT seq, type, ts_ns, payload::text, prev_hash, hash FROM registry_events \
             WHERE workspace_id = $1 ORDER BY seq",
        )
        .bind(self.workspace_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let payload_text: String = row.try_get(3)?;
                let payload = Json::parse(&payload_text).map_err(PgRegistryError::Registry)?;
                Ok(Event {
                    seq: u64::try_from(row.try_get::<i64, _>(0)?)
                        .map_err(|_| PgRegistryError::Integrity("negative registry sequence".into()))?,
                    kind: row.try_get(1)?,
                    ts_ns: row.try_get(2)?,
                    payload,
                    prev: row.try_get(4)?,
                    hash: row.try_get(5)?,
                })
            })
            .collect()
    }
}

/// Name of the per-workspace default experiment.
pub const DEFAULT_EXPERIMENT_NAME: &str = "Unsorted";

/// Insert an experiment and its event. The caller holds the workspace lock.
async fn create_experiment_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    name: &str,
    hypothesis: &str,
    tags: &[&str],
    parent_id: &str,
    created_by: Option<Uuid>,
) -> Result<String, PgRegistryError> {
    let parent = if parent_id.is_empty() {
        None
    } else {
        Some(
            find_experiment_id(tx, workspace_id, parent_id)
                .await?
                .ok_or_else(|| PgRegistryError::Invalid(format!("unknown parent experiment: {parent_id}")))?,
        )
    };
    let parent_registry_id = if let Some(id) = parent {
        Some(
            sqlx::query_scalar::<_, String>(
                "SELECT registry_id FROM experiments WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?,
        )
    } else {
        None
    };

    let id = spectra_db::new_id();
    let registry_id = id.to_string();
    let created_ts = (system_clock_ns())();
    let payload = Json::object()
        .with("id", Json::string(registry_id.clone()))
        .with("name", Json::string(name))
        .with("hypothesis", Json::string(hypothesis))
        .with("tags", Json::Array(tags.iter().map(|tag| Json::string(*tag)).collect()))
        .with("parent_id", Json::string(parent_registry_id.as_deref().unwrap_or("")));

    sqlx::query(
            "INSERT INTO experiments (id, workspace_id, name, hypothesis, tags, parent_id, created_by, registry_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(name)
        .bind(hypothesis)
        .bind(tags)
        .bind(parent)
        .bind(created_by)
        .bind(&registry_id)
        .execute(&mut **tx)
        .await?;

    append_event(tx, workspace_id, "experiment.created", created_ts, payload).await?;
    Ok(registry_id)
}

async fn find_experiment_id(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    id: &str,
) -> Result<Option<Uuid>, PgRegistryError> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM experiments WHERE workspace_id = $1 AND (registry_id = $2 OR id::text = $2)",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn find_run_id(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    id: &str,
) -> Result<Option<Uuid>, PgRegistryError> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM runs WHERE workspace_id = $1 AND (registry_id = $2 OR id::text = $2)",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn project_imported_event(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    event: &Event,
    experiments: &mut std::collections::HashMap<String, Uuid>,
    runs: &mut std::collections::HashMap<String, Uuid>,
) -> Result<(), PgRegistryError> {
    let payload = &event.payload;
    let text = |key: &str| -> Result<String, PgRegistryError> { Ok(payload.at(key)?.as_str()?.to_owned()) };
    let timestamp = event.ts_ns as f64 / 1_000_000_000.0;
    match event.kind.as_str() {
        "experiment.created" => {
            let registry_id = text("id")?;
            if registry_id.trim().is_empty() || experiments.contains_key(&registry_id) {
                return Err(PgRegistryError::Integrity("duplicate or empty experiment id in import".into()));
            }
            let parent_registry_id = text("parent_id").unwrap_or_default();
            let parent_id = if parent_registry_id.is_empty() {
                None
            } else {
                Some(*experiments.get(&parent_registry_id).ok_or_else(|| {
                    PgRegistryError::Integrity(format!(
                        "experiment parent {parent_registry_id:?} must precede its child"
                    ))
                })?)
            };
            let tags: Vec<String> = payload
                .at("tags")?
                .items()?
                .iter()
                .map(|tag| tag.as_str().map(str::to_owned).map_err(PgRegistryError::Registry))
                .collect::<Result<_, _>>()?;
            let id = spectra_db::new_id();
            sqlx::query(
                "INSERT INTO experiments (id, workspace_id, name, hypothesis, tags, parent_id, registry_id, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8))",
            )
            .bind(id)
            .bind(workspace_id)
            .bind(text("name")?)
            .bind(payload.find("hypothesis").map(|v| v.as_str()).transpose()?.unwrap_or("").to_owned())
            .bind(tags)
            .bind(parent_id)
            .bind(&registry_id)
            .bind(timestamp)
            .execute(&mut **tx)
            .await?;
            experiments.insert(registry_id, id);
        }
        "run.recorded" => {
            let registry_id = text("id")?;
            if registry_id.trim().is_empty() || runs.contains_key(&registry_id) {
                return Err(PgRegistryError::Integrity("duplicate or empty run id in import".into()));
            }
            let experiment_registry_id = text("experiment_id")?;
            let experiment_id = *experiments.get(&experiment_registry_id).ok_or_else(|| {
                PgRegistryError::Integrity(format!(
                    "run references unknown experiment {experiment_registry_id:?}"
                ))
            })?;
            let id = spectra_db::new_id();
            sqlx::query(
                "INSERT INTO runs (id, workspace_id, experiment_id, kind, status, created_at) \
                 VALUES ($1, $2, $3, $4, 'queued', to_timestamp($5))",
            )
            .bind(id)
            .bind(workspace_id)
            .bind(experiment_id)
            .bind(text("kind")?)
            .bind(timestamp)
            .execute(&mut **tx)
            .await?;
            sqlx::query("UPDATE runs SET status = 'running', started_at = to_timestamp($2) WHERE workspace_id = $1 AND id = $3")
                .bind(workspace_id).bind(timestamp).bind(id).execute(&mut **tx).await?;

            let params = payload.at("params")?.dump();
            let config = payload.at("config")?.dump();
            let metrics = payload.at("metrics")?.dump();
            let seed = text("seed")?
                .parse::<i64>()
                .map_err(|_| PgRegistryError::Integrity("run seed is outside signed 64-bit range".into()))?;
            let bar_count = payload.at("bar_count")?.as_int()?;
            let n_obs = payload.at("n_obs")?.as_int()?;
            if bar_count < 0 || n_obs < 0 {
                return Err(PgRegistryError::Integrity("negative run bar or observation count".into()));
            }
            sqlx::query(
                "UPDATE runs SET status = 'succeeded', finished_at = to_timestamp($3), \
                 dataset_id = $4, dataset_bars_sha256 = $5, strategy = $6, strategy_sha256 = $7, \
                 params = $8::jsonb, config = $9::jsonb, config_sha256 = $10, result_sha256 = $11, \
                 engine_version = $12, seed = $13, bar_count = $14, metrics = $15::jsonb, \
                 sharpe_per_period = $16, n_obs = $17, counts_as_trial = $18, registry_id = $19 \
                 WHERE workspace_id = $1 AND id = $2",
            )
            .bind(workspace_id)
            .bind(id)
            .bind(timestamp)
            .bind(payload.find("dataset_id").map(|v| v.as_str()).transpose()?.unwrap_or("").to_owned())
            .bind(text("dataset_bars_sha256")?)
            .bind(text("strategy")?)
            .bind(payload.find("strategy_sha256").map(|v| v.as_str()).transpose()?.unwrap_or("").to_owned())
            .bind(params)
            .bind(config)
            .bind(text("config_sha256")?)
            .bind(text("result_sha256")?)
            .bind(text("engine_version")?)
            .bind(seed)
            .bind(bar_count)
            .bind(metrics)
            .bind(payload.at("sharpe_per_period")?.as_double()?)
            .bind(n_obs)
            .bind(payload.at("counts_as_trial")?.as_bool()?)
            .bind(&registry_id)
            .execute(&mut **tx)
            .await?;
            runs.insert(registry_id.clone(), id);
            let note = payload.find("note").map(|v| v.as_str()).transpose()?.unwrap_or("");
            if !note.trim().is_empty() {
                sqlx::query("INSERT INTO run_notes (workspace_id, run_id, body, created_at) VALUES ($1, $2, $3, to_timestamp($4))")
                    .bind(workspace_id).bind(id).bind(note).bind(timestamp).execute(&mut **tx).await?;
            }
        }
        "artifact.recorded" => {
            let experiment_id = text("experiment_id")?;
            let Some(experiment_id) = experiments.get(&experiment_id).copied() else {
                return Err(PgRegistryError::Integrity("artifact references an unknown experiment".into()));
            };
            let run_registry_id = text("run_id").unwrap_or_default();
            if !run_registry_id.is_empty() {
                let run_id = *runs.get(&run_registry_id).ok_or_else(|| {
                    PgRegistryError::Integrity("artifact references a run that has not been imported".into())
                })?;
                let uri = payload.find("uri").map(|v| v.as_str()).transpose()?.unwrap_or("");
                let summary = payload.find("summary").map_or_else(|| "{}".to_owned(), Json::dump);
                sqlx::query(
                    "INSERT INTO run_artifacts (id, workspace_id, run_id, kind, sha256, path, size_bytes, \
                     content_type, registry_id, summary, created_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, 0, 'application/octet-stream', $7, $8::jsonb, to_timestamp($9))",
                )
                .bind(spectra_db::new_id()).bind(workspace_id).bind(run_id).bind(text("kind")?)
                .bind(text("sha256")?).bind(uri).bind(text("id")?).bind(summary).bind(timestamp)
                .execute(&mut **tx).await?;
            }
            let _ = experiment_id;
        }
        "note.added" => {
            let target = text("target_id")?;
            let body = text("text")?;
            if let Some(id) = experiments.get(&target) {
                sqlx::query("INSERT INTO experiment_notes (workspace_id, experiment_id, body, created_at) VALUES ($1, $2, $3, to_timestamp($4))")
                    .bind(workspace_id).bind(id).bind(body).bind(timestamp).execute(&mut **tx).await?;
            } else if let Some(id) = runs.get(&target) {
                sqlx::query("INSERT INTO run_notes (workspace_id, run_id, body, created_at) VALUES ($1, $2, $3, to_timestamp($4))")
                    .bind(workspace_id).bind(id).bind(body).bind(timestamp).execute(&mut **tx).await?;
            } else {
                return Err(PgRegistryError::Integrity(format!("note references unknown target {target:?}")));
            }
        }
        // Holdout vault and future registry events stay intact in the canonical
        // event log. Their relational projections belong to their own modules.
        _ => {}
    }
    Ok(())
}

fn row_to_run(row: &sqlx::postgres::PgRow) -> Result<RunInfo, PgRegistryError> {
    let params_text: String = row.try_get(7)?;
    let config_text: String = row.try_get(8)?;
    let metrics_text: String = row.try_get(14)?;
    let status: String = row.try_get(19)?;
    let metrics = if metrics_text == "{}" {
        Metrics::default()
    } else {
        metrics_from_json(&Json::parse(&metrics_text).map_err(PgRegistryError::Registry)?)?
    };
    let params = params_from_json(&Json::parse(&params_text).map_err(PgRegistryError::Registry)?)?;
    let config = Json::parse(&config_text).map_err(PgRegistryError::Registry)?;
    Ok(RunInfo {
        id: row.try_get(0)?,
        experiment_id: row.try_get::<Option<String>, _>(1)?.unwrap_or_default(),
        kind: row.try_get(2)?,
        dataset_id: row.try_get(3)?,
        dataset_bars_sha256: row.try_get(4)?,
        strategy: row.try_get(5)?,
        strategy_sha256: row.try_get(6)?,
        params,
        config,
        config_sha256: row.try_get(9)?,
        result_sha256: row.try_get(10)?,
        engine_version: row.try_get(11)?,
        seed: row
            .try_get::<Option<i64>, _>(12)?
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or_default(),
        bar_count: row
            .try_get::<i64, _>(13)?
            .try_into()
            .map_err(|_| PgRegistryError::Integrity("negative run bar count".into()))?,
        metrics,
        sharpe_per_period: row.try_get::<Option<f64>, _>(15)?.unwrap_or_default(),
        n_obs: row
            .try_get::<i64, _>(16)?
            .try_into()
            .map_err(|_| PgRegistryError::Integrity("negative run observation count".into()))?,
        counts_as_trial: row.try_get(17)?,
        recorded_ts: row.try_get(18)?,
        status,
        job_id: row.try_get(20)?,
        error: row.try_get(21)?,
        notes: row.try_get(22)?,
    })
}

fn validate_run_input(input: &RunInput) -> Result<(), PgRegistryError> {
    if input.kind.trim().is_empty() || input.strategy.trim().is_empty() {
        return Err(PgRegistryError::Invalid("run kind and strategy must not be empty".into()));
    }
    for (name, hash) in [
        ("dataset bars", input.run.dataset_sha256.as_str()),
        ("strategy source", input.strategy_sha256.as_str()),
        ("configuration", input.run.config_sha256.as_str()),
        ("result", input.run.result_sha256.as_str()),
    ] {
        if hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(PgRegistryError::Invalid(format!(
                "{name} fingerprint must be 64 lowercase hex characters"
            )));
        }
    }
    if config_fingerprint(&input.config) != input.run.config_sha256 {
        return Err(PgRegistryError::Invalid(
            "configuration fingerprint does not match the supplied config".into(),
        ));
    }
    if input.returns.iter().any(|value| !value.is_finite()) {
        return Err(PgRegistryError::Invalid("run returns must all be finite".into()));
    }
    metrics_to_json(&input.metrics)?;
    config_to_json(&input.config)?;
    params_to_json(&input.params)?;
    Ok(())
}

fn validate_stored_artifact(artifact: &StoredArtifact) -> Result<(), PgRegistryError> {
    if artifact.kind.trim().is_empty() || artifact.uri.trim().is_empty() {
        return Err(PgRegistryError::Invalid("artifact kind and URI must not be empty".into()));
    }
    if artifact.sha256.len() != 64
        || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(PgRegistryError::Invalid("artifact SHA-256 must be 64 lowercase hex characters".into()));
    }
    i64::try_from(artifact.size_bytes)
        .map_err(|_| PgRegistryError::Invalid("artifact is too large".into()))?;
    Ok(())
}

/// Insert (or adopt) the `run_artifacts` row and append its event. The
/// artifact store may already have recorded the row when it stored the bytes;
/// that row is adopted rather than duplicated.
async fn link_artifact(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    run_db_id: Uuid,
    experiment_registry_id: &str,
    run_registry_id: &str,
    artifact: &StoredArtifact,
    ts_ns: i64,
) -> Result<String, PgRegistryError> {
    let id = spectra_db::new_id();
    let registry_id: String = sqlx::query_scalar(
        "INSERT INTO run_artifacts (id, workspace_id, run_id, kind, sha256, path, size_bytes, \
         content_type, registry_id, summary) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb) \
         ON CONFLICT (run_id, kind, sha256) DO UPDATE SET summary = EXCLUDED.summary, \
           registry_id = EXCLUDED.registry_id \
         RETURNING registry_id",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(run_db_id)
    .bind(&artifact.kind)
    .bind(&artifact.sha256)
    .bind(&artifact.uri)
    .bind(i64::try_from(artifact.size_bytes).unwrap_or(i64::MAX))
    .bind(&artifact.content_type)
    .bind(id.to_string())
    .bind(artifact.summary.dump())
    .fetch_one(&mut **tx)
    .await?;
    let payload = Json::object()
        .with("id", Json::string(registry_id.clone()))
        .with("experiment_id", Json::string(experiment_registry_id))
        .with("run_id", Json::string(run_registry_id))
        .with("kind", Json::string(artifact.kind.clone()))
        .with("sha256", Json::string(artifact.sha256.clone()))
        .with("uri", Json::string(artifact.uri.clone()))
        .with("summary", artifact.summary.clone());
    append_event(tx, workspace_id, "artifact.recorded", ts_ns, payload).await?;
    Ok(registry_id)
}

async fn lock_workspace(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
) -> Result<(), PgRegistryError> {
    let locked = sqlx::query_scalar::<_, Uuid>("SELECT id FROM workspaces WHERE id = $1 FOR NO KEY UPDATE")
        .bind(workspace_id)
        .fetch_optional(&mut **tx)
        .await?;
    if locked.is_none() {
        return Err(PgRegistryError::Invalid(format!("unknown workspace: {workspace_id}")));
    }
    Ok(())
}

async fn append_event(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    kind: &str,
    ts_ns: i64,
    payload: Json,
) -> Result<String, PgRegistryError> {
    // Callers lock the workspace row before reading the tail. The DB trigger
    // takes the same lock as a second line of defense for other writers.
    let previous = sqlx::query(
        "SELECT seq, hash FROM registry_events WHERE workspace_id = $1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(workspace_id)
    .fetch_optional(&mut **tx)
    .await?;
    let (seq, prev_hash) = if let Some(row) = previous {
        let last_seq: i64 = row.try_get(0)?;
        let seq = last_seq
            .checked_add(1)
            .ok_or_else(|| PgRegistryError::Integrity("registry sequence overflow".into()))?;
        (seq, row.try_get::<String, _>(1)?)
    } else {
        (0, GENESIS.to_string())
    };
    let seq_u64 =
        u64::try_from(seq).map_err(|_| PgRegistryError::Integrity("negative registry sequence".into()))?;
    let hash = event_hash(&prev_hash, seq_u64, kind, ts_ns, &payload);
    let payload_text = payload.dump();
    sqlx::query(
        "INSERT INTO registry_events (workspace_id, seq, type, ts_ns, payload, prev_hash, hash) \
         VALUES ($1, $2, $3, $4, $5::json, $6, $7)",
    )
    .bind(workspace_id)
    .bind(seq)
    .bind(kind)
    .bind(ts_ns)
    .bind(payload_text)
    .bind(prev_hash)
    .bind(&hash)
    .execute(&mut **tx)
    .await?;
    Ok(hash)
}
