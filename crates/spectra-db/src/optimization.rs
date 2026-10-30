//! Module 4: Parameter Sweeps, Walk-Forward Validation, Overfitting and Holdout Vault persistence.
#![allow(clippy::missing_errors_doc)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{Error, PgPool};

/// A parameter sweep record (parent of optimization trials).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct OptimizationSweep {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub job_id: Option<Uuid>,
    pub experiment_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,
    pub strategy_version_id: Option<Uuid>,
    pub dataset_version_id: Uuid,
    pub status: String,
    pub method: String,
    pub objective: String,
    pub param_grid: JsonValue,
    pub trial_count: i32,
    pub best_trial_num: Option<i32>,
    pub best_params: Option<JsonValue>,
    pub best_metrics: Option<JsonValue>,
    pub best_objective: Option<f64>,
    pub dsr: Option<f64>,
    pub psr: Option<f64>,
    pub min_trl: Option<f64>,
    pub pbo: Option<f64>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// An individual trial within a sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct OptimizationTrial {
    pub id: Uuid,
    pub sweep_id: Uuid,
    pub trial_num: i32,
    pub params: JsonValue,
    pub metrics: JsonValue,
    pub objective_value: f64,
    pub returns_mean: Option<f64>,
    pub returns_vol: Option<f64>,
    pub created_at: DateTime<Utc>,
}

/// A walk-forward validation report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct WalkForwardReport {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub job_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,
    pub strategy_version_id: Option<Uuid>,
    pub dataset_version_id: Uuid,
    pub status: String,
    pub mode: String,
    pub train_size: i32,
    pub test_size: i32,
    pub step_size: i32,
    pub embargo: i32,
    pub objective: String,
    pub param_grid: JsonValue,
    pub folds: JsonValue,
    pub efficiency_ratio: Option<f64>,
    pub in_sample_sharpe: Option<f64>,
    pub out_of_sample_sharpe: Option<f64>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A sealed holdout dataset in the holdout vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct HoldoutVault {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub dataset_version_id: Uuid,
    pub budget: i32,
    pub evaluations_used: i32,
    pub notes: String,
    pub sealed_by: Option<Uuid>,
    pub sealed_at: DateTime<Utc>,
}

/// An audited evaluation against a sealed holdout dataset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct HoldoutEvaluation {
    pub id: Uuid,
    pub holdout_id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_version_id: Option<Uuid>,
    pub evaluation_num: i32,
    pub metrics: JsonValue,
    pub dsr: Option<f64>,
    pub warnings: JsonValue,
    pub evaluated_by: Option<Uuid>,
    pub evaluated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------- Sweeps

/// Insert a new queued optimization sweep.
#[allow(clippy::too_many_arguments)]
pub async fn insert_sweep(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    job_id: Option<Uuid>,
    experiment_id: Option<Uuid>,
    strategy_id: Option<Uuid>,
    strategy_version_id: Option<Uuid>,
    dataset_version_id: Uuid,
    method: &str,
    objective: &str,
    param_grid: &JsonValue,
    created_by: Option<Uuid>,
) -> Result<OptimizationSweep, Error> {
    sqlx::query_as(include_str!("../queries/sweep_insert.sql"))
        .bind(id)
        .bind(workspace_id)
        .bind(job_id)
        .bind(experiment_id)
        .bind(strategy_id)
        .bind(strategy_version_id)
        .bind(dataset_version_id)
        .bind(method)
        .bind(objective)
        .bind(param_grid)
        .bind(created_by)
        .fetch_one(pool)
        .await
}

/// Fetch a sweep by ID.
pub async fn get_sweep(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<OptimizationSweep>, Error> {
    sqlx::query_as(include_str!("../queries/sweep_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// List sweeps in a workspace.
pub async fn list_sweeps(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: i32,
) -> Result<Vec<OptimizationSweep>, Error> {
    sqlx::query_as(include_str!("../queries/sweep_list.sql"))
        .bind(workspace_id)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Update a sweep's status and computed overfitting statistics.
#[allow(clippy::too_many_arguments)]
pub async fn update_sweep(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    trial_count: i32,
    best_trial_num: Option<i32>,
    best_params: Option<&JsonValue>,
    best_metrics: Option<&JsonValue>,
    best_objective: Option<f64>,
    dsr: Option<f64>,
    psr: Option<f64>,
    min_trl: Option<f64>,
    pbo: Option<f64>,
) -> Result<OptimizationSweep, Error> {
    sqlx::query_as(include_str!("../queries/sweep_update.sql"))
        .bind(id)
        .bind(status)
        .bind(trial_count)
        .bind(best_trial_num)
        .bind(best_params)
        .bind(best_metrics)
        .bind(best_objective)
        .bind(dsr)
        .bind(psr)
        .bind(min_trl)
        .bind(pbo)
        .fetch_one(pool)
        .await
}

/// Insert a completed trial for a sweep.
#[allow(clippy::too_many_arguments)]
pub async fn insert_trial(
    pool: &PgPool,
    sweep_id: Uuid,
    trial_num: i32,
    params: &JsonValue,
    metrics: &JsonValue,
    objective_value: f64,
    returns_mean: Option<f64>,
    returns_vol: Option<f64>,
) -> Result<OptimizationTrial, Error> {
    let id = crate::new_id();
    sqlx::query_as(include_str!("../queries/sweep_trial_insert.sql"))
        .bind(id)
        .bind(sweep_id)
        .bind(trial_num)
        .bind(params)
        .bind(metrics)
        .bind(objective_value)
        .bind(returns_mean)
        .bind(returns_vol)
        .fetch_one(pool)
        .await
}

/// List all trials belonging to a sweep.
pub async fn trials_of_sweep(pool: &PgPool, sweep_id: Uuid) -> Result<Vec<OptimizationTrial>, Error> {
    sqlx::query_as(include_str!("../queries/sweep_trials_of.sql")).bind(sweep_id).fetch_all(pool).await
}

// ---------------------------------------------------------------- Walk-Forward

/// Insert a new queued walk-forward report.
#[allow(clippy::too_many_arguments)]
pub async fn insert_walk_forward(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    job_id: Option<Uuid>,
    strategy_id: Option<Uuid>,
    strategy_version_id: Option<Uuid>,
    dataset_version_id: Uuid,
    mode: &str,
    train_size: i32,
    test_size: i32,
    step_size: i32,
    embargo: i32,
    objective: &str,
    param_grid: &JsonValue,
    created_by: Option<Uuid>,
) -> Result<WalkForwardReport, Error> {
    sqlx::query_as(include_str!("../queries/walk_forward_insert.sql"))
        .bind(id)
        .bind(workspace_id)
        .bind(job_id)
        .bind(strategy_id)
        .bind(strategy_version_id)
        .bind(dataset_version_id)
        .bind(mode)
        .bind(train_size)
        .bind(test_size)
        .bind(step_size)
        .bind(embargo)
        .bind(objective)
        .bind(param_grid)
        .bind(created_by)
        .fetch_one(pool)
        .await
}

/// Get a walk-forward report by ID.
pub async fn get_walk_forward(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<WalkForwardReport>, Error> {
    sqlx::query_as(include_str!("../queries/walk_forward_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// List walk-forward reports in a workspace.
pub async fn list_walk_forwards(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: i32,
) -> Result<Vec<WalkForwardReport>, Error> {
    sqlx::query_as(include_str!("../queries/walk_forward_list.sql"))
        .bind(workspace_id)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Update walk-forward status, fold results and efficiency ratio.
pub async fn update_walk_forward(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    folds: &JsonValue,
    efficiency_ratio: Option<f64>,
    in_sample_sharpe: Option<f64>,
    out_of_sample_sharpe: Option<f64>,
) -> Result<WalkForwardReport, Error> {
    sqlx::query_as(include_str!("../queries/walk_forward_update.sql"))
        .bind(id)
        .bind(status)
        .bind(folds)
        .bind(efficiency_ratio)
        .bind(in_sample_sharpe)
        .bind(out_of_sample_sharpe)
        .fetch_one(pool)
        .await
}

// ---------------------------------------------------------------- Holdout Vault

/// Seal a dataset version in the holdout vault with an evaluation budget.
pub async fn insert_holdout(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    name: &str,
    dataset_version_id: Uuid,
    budget: i32,
    notes: &str,
    sealed_by: Option<Uuid>,
) -> Result<HoldoutVault, Error> {
    sqlx::query_as(include_str!("../queries/holdout_insert.sql"))
        .bind(id)
        .bind(workspace_id)
        .bind(name)
        .bind(dataset_version_id)
        .bind(budget)
        .bind(notes)
        .bind(sealed_by)
        .fetch_one(pool)
        .await
}

/// Get a holdout vault entry by ID.
pub async fn get_holdout(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<HoldoutVault>, Error> {
    sqlx::query_as(include_str!("../queries/holdout_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Get a holdout vault entry by name.
pub async fn get_holdout_by_name(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
) -> Result<Option<HoldoutVault>, Error> {
    sqlx::query_as(include_str!("../queries/holdout_get_by_name.sql"))
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(pool)
        .await
}

/// List all sealed holdouts in a workspace.
pub async fn list_holdouts(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<HoldoutVault>, Error> {
    sqlx::query_as(include_str!("../queries/holdout_list.sql")).bind(workspace_id).fetch_all(pool).await
}

/// A granted holdout evaluation: one unit of budget is spent and the grant is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldoutGrant {
    /// Which evaluation this is (1, 2, ...), the number the result is recorded under.
    pub evaluation_num: i32,
    /// The holdout's total budget.
    pub budget: i32,
    /// The sealed data. Its checksum is what unlocks the bars.
    pub data: crate::datasets::DatasetVersion,
}

/// Spends one evaluation of holdout `holdout_id` and returns its sealed data.
///
/// This is the only query that returns a sealed holdout's checksum. The database spends the
/// budget and writes the grant to `holdout_access_log` before returning it, under the holdout's
/// row lock, so concurrent evaluations cannot overspend. `None`: no such holdout in the workspace,
/// or its budget is spent (logged as `exhausted`).
pub async fn grant_holdout_evaluation(
    pool: &PgPool,
    workspace_id: Uuid,
    holdout_id: Uuid,
    purpose: &str,
    user: Option<Uuid>,
) -> Result<Option<HoldoutGrant>, Error> {
    type Row = (i32, i32, Uuid, String, i64, String, i32, Option<String>);
    let row: Option<Row> = sqlx::query_as(include_str!("../queries/holdout_grant.sql"))
        .bind(workspace_id)
        .bind(holdout_id)
        .bind(purpose)
        .bind(user)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(evaluation_num, budget, id, checksum, row_count, dataset_name, version, blob_sha256)| {
        HoldoutGrant {
            evaluation_num,
            budget,
            data: crate::datasets::DatasetVersion {
                id,
                checksum,
                row_count,
                dataset_name,
                version,
                blob_sha256,
            },
        }
    }))
}

/// One entry of a holdout's access log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct HoldoutAccess {
    pub id: Uuid,
    pub holdout_id: Uuid,
    /// `denied`, `granted` or `exhausted`.
    pub action: String,
    pub purpose: String,
    pub evaluation_num: Option<i32>,
    pub user_id: Option<Uuid>,
    pub at: DateTime<Utc>,
}

/// A holdout's access log, newest first (at most `limit`).
pub async fn holdout_access_log(
    pool: &PgPool,
    workspace_id: Uuid,
    holdout_id: Uuid,
    limit: i64,
) -> Result<Vec<HoldoutAccess>, Error> {
    sqlx::query_as(include_str!("../queries/holdout_access_log.sql"))
        .bind(workspace_id)
        .bind(holdout_id)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Records the result of evaluation `evaluation_num`, which [`grant_holdout_evaluation`] granted
/// (and already counted against the budget).
#[allow(clippy::too_many_arguments)]
pub async fn insert_holdout_evaluation(
    pool: &PgPool,
    holdout_id: Uuid,
    workspace_id: Uuid,
    strategy_version_id: Option<Uuid>,
    evaluation_num: i32,
    metrics: &JsonValue,
    dsr: Option<f64>,
    warnings: &JsonValue,
    evaluated_by: Option<Uuid>,
) -> Result<HoldoutEvaluation, Error> {
    let mut tx = pool.begin().await?;

    let id = crate::new_id();
    let eval: HoldoutEvaluation = sqlx::query_as(include_str!("../queries/holdout_eval_insert.sql"))
        .bind(id)
        .bind(holdout_id)
        .bind(workspace_id)
        .bind(strategy_version_id)
        .bind(evaluation_num)
        .bind(metrics)
        .bind(dsr)
        .bind(warnings)
        .bind(evaluated_by)
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(eval)
}

/// List all past evaluations of a sealed holdout.
pub async fn evaluations_of_holdout(
    pool: &PgPool,
    holdout_id: Uuid,
) -> Result<Vec<HoldoutEvaluation>, Error> {
    sqlx::query_as(include_str!("../queries/holdout_evals_of.sql")).bind(holdout_id).fetch_all(pool).await
}

/// Check if a dataset version is sealed in the holdout vault for this workspace.
pub async fn is_holdout_sealed(
    pool: &PgPool,
    workspace_id: Uuid,
    dataset_version_id: Uuid,
) -> Result<bool, Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM holdout_vault WHERE workspace_id = $1 AND dataset_version_id = $2)",
    )
    .bind(workspace_id)
    .bind(dataset_version_id)
    .fetch_one(pool)
    .await
}

/// Find holdout evaluations associated with a workspace and optionally strategy version or dataset version.
pub async fn evaluations_for_run_target(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_version_id: Option<Uuid>,
    dataset_version_id: Option<Uuid>,
) -> Result<Vec<HoldoutEvaluation>, Error> {
    sqlx::query_as(
        "SELECT e.id, e.holdout_id, e.workspace_id, e.strategy_version_id, e.evaluation_num, e.metrics, e.dsr, e.warnings, e.evaluated_by, e.evaluated_at \
         FROM holdout_evaluations e \
         JOIN holdout_vault v ON v.id = e.holdout_id \
         WHERE e.workspace_id = $1 \
           AND ($2::uuid IS NULL OR e.strategy_version_id = $2) \
           AND ($3::uuid IS NULL OR v.dataset_version_id = $3) \
         ORDER BY e.evaluated_at DESC LIMIT 20",
    )
    .bind(workspace_id)
    .bind(strategy_version_id)
    .bind(dataset_version_id)
    .fetch_all(pool)
    .await
}
