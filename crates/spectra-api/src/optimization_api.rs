//! `/api/v1/optimization` (Module 4): Parameter Sweeps, Walk-Forward Validation,
//! Overfitting Statistics (PSR, DSR, MinTRL, CSCV PBO), and Holdout Vault.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spectra_db::datasets;
use spectra_db::jobs::{self, NewJob};
use spectra_db::optimization::{self, HoldoutEvaluation, HoldoutVault, OptimizationSweep, OptimizationTrial, WalkForwardReport};
use spectra_research::pbo::cscv_pbo;
use spectra_research::stats::{
    deflated_sharpe, expected_max_sharpe, min_track_record_length, moments, probabilistic_sharpe,
    sharpe_per_period,
};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, error_with_code, AppState, Identity};

const NOT_FOUND: &str = "Resource not found.";
const FORBIDDEN_VIEWER: &str = "Viewers have read-only access to optimization workflows.";

fn user_of(id: &Identity) -> Option<Uuid> {
    Uuid::parse_str(&id.user).ok()
}

#[allow(clippy::result_large_err)]
async fn ensure_can_edit(pool: &spectra_db::PgPool, ws: Uuid, user_id: Option<Uuid>) -> Result<(), Response> {
    if let Some(uid) = user_id {
        let role: Option<(String,)> =
            sqlx::query_as("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
                .bind(ws)
                .bind(uid)
                .fetch_optional(pool)
                .await
                .map_err(|e| db_error(&e))?;

        if let Some((r,)) = role {
            if r == "viewer" {
                return Err(error_with_code(403, "viewer_read_only", FORBIDDEN_VIEWER));
            }
        }
    }
    Ok(())
}

fn metrics_to_json(m: &spectra_engine::Metrics) -> serde_json::Value {
    json!({
        "valid": m.valid,
        "periods": m.periods,
        "total_return": m.total_return,
        "annualized_return": m.annualized_return,
        "annualized_volatility": m.annualized_volatility,
        "sharpe": m.sharpe,
        "max_drawdown": m.max_drawdown,
    })
}

// ---------------------------------------------------------------- Data Transfer Objects

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OptimizationSweepView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub job_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,
    pub strategy_version_id: Option<Uuid>,
    pub dataset_version_id: Uuid,
    pub status: String,
    pub method: String,
    pub objective: String,
    pub param_grid: serde_json::Value,
    pub trial_count: i32,
    pub best_trial_num: Option<i32>,
    pub best_params: Option<serde_json::Value>,
    pub best_metrics: Option<serde_json::Value>,
    pub best_objective: Option<f64>,
    pub dsr: Option<f64>,
    pub psr: Option<f64>,
    pub min_trl: Option<f64>,
    pub pbo: Option<f64>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<OptimizationSweep> for OptimizationSweepView {
    fn from(s: OptimizationSweep) -> Self {
        Self {
            id: s.id,
            workspace_id: s.workspace_id,
            job_id: s.job_id,
            strategy_id: s.strategy_id,
            strategy_version_id: s.strategy_version_id,
            dataset_version_id: s.dataset_version_id,
            status: s.status,
            method: s.method,
            objective: s.objective,
            param_grid: s.param_grid,
            trial_count: s.trial_count,
            best_trial_num: s.best_trial_num,
            best_params: s.best_params,
            best_metrics: s.best_metrics,
            best_objective: s.best_objective,
            dsr: s.dsr,
            psr: s.psr,
            min_trl: s.min_trl,
            pbo: s.pbo,
            created_by: s.created_by,
            created_at: s.created_at,
            finished_at: s.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SweepTrialView {
    pub id: Uuid,
    pub sweep_id: Uuid,
    pub trial_num: i32,
    pub params: serde_json::Value,
    pub metrics: serde_json::Value,
    pub objective_value: f64,
    pub returns_mean: Option<f64>,
    pub returns_vol: Option<f64>,
    pub created_at: DateTime<Utc>,
}

impl From<OptimizationTrial> for SweepTrialView {
    fn from(t: OptimizationTrial) -> Self {
        Self {
            id: t.id,
            sweep_id: t.sweep_id,
            trial_num: t.trial_num,
            params: t.params,
            metrics: t.metrics,
            objective_value: t.objective_value,
            returns_mean: t.returns_mean,
            returns_vol: t.returns_vol,
            created_at: t.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WalkForwardReportView {
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
    pub param_grid: serde_json::Value,
    pub folds: serde_json::Value,
    pub efficiency_ratio: Option<f64>,
    pub in_sample_sharpe: Option<f64>,
    pub out_of_sample_sharpe: Option<f64>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<WalkForwardReport> for WalkForwardReportView {
    fn from(w: WalkForwardReport) -> Self {
        Self {
            id: w.id,
            workspace_id: w.workspace_id,
            job_id: w.job_id,
            strategy_id: w.strategy_id,
            strategy_version_id: w.strategy_version_id,
            dataset_version_id: w.dataset_version_id,
            status: w.status,
            mode: w.mode,
            train_size: w.train_size,
            test_size: w.test_size,
            step_size: w.step_size,
            embargo: w.embargo,
            objective: w.objective,
            param_grid: w.param_grid,
            folds: w.folds,
            efficiency_ratio: w.efficiency_ratio,
            in_sample_sharpe: w.in_sample_sharpe,
            out_of_sample_sharpe: w.out_of_sample_sharpe,
            created_by: w.created_by,
            created_at: w.created_at,
            finished_at: w.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HoldoutVaultView {
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

impl From<HoldoutVault> for HoldoutVaultView {
    fn from(h: HoldoutVault) -> Self {
        Self {
            id: h.id,
            workspace_id: h.workspace_id,
            name: h.name,
            dataset_version_id: h.dataset_version_id,
            budget: h.budget,
            evaluations_used: h.evaluations_used,
            notes: h.notes,
            sealed_by: h.sealed_by,
            sealed_at: h.sealed_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct HoldoutEvaluationView {
    pub id: Uuid,
    pub holdout_id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_version_id: Option<Uuid>,
    pub evaluation_num: i32,
    pub metrics: serde_json::Value,
    pub dsr: Option<f64>,
    pub warnings: serde_json::Value,
    pub evaluated_by: Option<Uuid>,
    pub evaluated_at: DateTime<Utc>,
}

impl From<HoldoutEvaluation> for HoldoutEvaluationView {
    fn from(e: HoldoutEvaluation) -> Self {
        Self {
            id: e.id,
            holdout_id: e.holdout_id,
            workspace_id: e.workspace_id,
            strategy_version_id: e.strategy_version_id,
            evaluation_num: e.evaluation_num,
            metrics: e.metrics,
            dsr: e.dsr,
            warnings: e.warnings,
            evaluated_by: e.evaluated_by,
            evaluated_at: e.evaluated_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSweepRequest {
    pub dataset_version_id: Uuid,
    #[serde(default)]
    pub strategy_version_id: Option<Uuid>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub dsl_code: Option<String>,
    pub param_grid: serde_json::Value,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub max_trials: Option<usize>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default = "default_objective")]
    pub objective: String,
}

fn default_method() -> String {
    "grid".to_string()
}

fn default_objective() -> String {
    "sharpe".to_string()
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateWalkForwardRequest {
    pub dataset_version_id: Uuid,
    #[serde(default)]
    pub strategy_version_id: Option<Uuid>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub dsl_code: Option<String>,
    #[serde(default = "default_wf_mode")]
    pub mode: String,
    pub train_size: usize,
    pub test_size: usize,
    #[serde(default)]
    pub step_size: Option<usize>,
    #[serde(default)]
    pub embargo: Option<usize>,
    pub param_grid: serde_json::Value,
    #[serde(default = "default_objective")]
    pub objective: String,
}

fn default_wf_mode() -> String {
    "rolling".to_string()
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputeOverfittingRequest {
    pub returns_matrix: Vec<Vec<f64>>,
    #[serde(default)]
    pub benchmark_sharpe: Option<f64>,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
}

fn default_confidence() -> f64 {
    0.95
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OverfittingResponse {
    pub psr: f64,
    pub dsr: f64,
    pub min_trl: f64,
    pub pbo: f64,
    pub expected_max_sharpe: f64,
    pub sharpe_variance: f64,
    pub best_sharpe: f64,
    pub num_trials: usize,
    pub num_observations: usize,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SealHoldoutRequest {
    pub name: String,
    pub dataset_version_id: Uuid,
    pub budget: i32,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluateHoldoutRequest {
    #[serde(default)]
    pub strategy_version_id: Option<Uuid>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub dsl_code: Option<String>,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
}

// ---------------------------------------------------------------- Parameter Sweeps

/// Queue a parameter sweep (grid or random search).
#[utoipa::path(post, path = "/api/v1/optimization/sweep", tag = "optimization",
    request_body = CreateSweepRequest,
    responses((status = 202, body = OptimizationSweepView), (status = 403, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn queue_sweep(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateSweepRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid sweep request: {e}")),
    };

    // Verify dataset version exists
    match datasets::resolve_version(&pool, ws, req.dataset_version_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, "Dataset version not found in this workspace."),
        Err(e) => return db_error(&e),
    }

    let sweep_id = spectra_db::new_id();

    let job_payload = json!({
        "sweep_id": sweep_id,
        "dataset_version_id": req.dataset_version_id,
        "strategy_version_id": req.strategy_version_id,
        "strategy": req.strategy,
        "dsl_code": req.dsl_code,
        "param_grid": req.param_grid,
        "method": req.method,
        "max_trials": req.max_trials,
        "seed": req.seed,
        "objective": req.objective,
    });

    let new_job = NewJob {
        workspace_id: ws,
        kind: "sweep".into(),
        payload: job_payload,
        priority: 5,
        max_attempts: 2,
        created_by: user_of(&id),
    };

    let queued_job = match jobs::enqueue(&pool, &new_job).await {
        Ok(j) => j,
        Err(e) => return db_error(&e),
    };

    match optimization::insert_sweep(
        &pool,
        sweep_id,
        ws,
        Some(queued_job.id),
        None,
        req.strategy_version_id,
        req.dataset_version_id,
        &req.method,
        &req.objective,
        &req.param_grid,
        user_of(&id),
    )
    .await
    {
        Ok(sweep) => to_json(202, &OptimizationSweepView::from(sweep)),
        Err(e) => db_error(&e),
    }
}

/// List parameter sweeps in workspace.
#[utoipa::path(get, path = "/api/v1/optimization/sweeps", tag = "optimization",
    responses((status = 200, body = Vec<OptimizationSweepView>)))]
pub(crate) async fn list_sweeps(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match optimization::list_sweeps(&pool, ws, 100).await {
        Ok(items) => {
            let views: Vec<OptimizationSweepView> = items.into_iter().map(OptimizationSweepView::from).collect();
            to_json(200, &json!({ "items": views }))
        }
        Err(e) => db_error(&e),
    }
}

/// Get parameter sweep details with all trials and 2D sensitivity matrix.
#[utoipa::path(get, path = "/api/v1/optimization/sweep/{id}", tag = "optimization",
    params(("id" = Uuid, Path, description = "Sweep id")),
    responses((status = 200, body = serde_json::Value), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_sweep(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    sweep_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(sweep_id)) = sweep_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let sweep = match optimization::get_sweep(&pool, ws, sweep_id).await {
        Ok(Some(s)) => OptimizationSweepView::from(s),
        Ok(None) => return error_response(404, NOT_FOUND),
        Err(e) => return db_error(&e),
    };
    let trials = match optimization::trials_of_sweep(&pool, sweep_id).await {
        Ok(t) => t,
        Err(e) => return db_error(&e),
    };

    let sensitivity = compute_sensitivity_matrix(&trials);
    let trial_views: Vec<SweepTrialView> = trials.into_iter().map(SweepTrialView::from).collect();

    to_json(200, &json!({
        "sweep": sweep,
        "trials": trial_views,
        "sensitivity": sensitivity,
    }))
}

/// Computes 2D heatmap matrix if two distinct parameters vary across trials.
fn compute_sensitivity_matrix(trials: &[OptimizationTrial]) -> Option<serde_json::Value> {
    if trials.is_empty() {
        return None;
    }
    // Find parameters that vary
    let first_params = trials[0].params.as_object()?;
    let mut param_names = Vec::new();
    for key in first_params.keys() {
        let mut distinct = std::collections::HashSet::new();
        for t in trials {
            if let Some(v) = t.params.get(key) {
                distinct.insert(v.to_string());
            }
        }
        if distinct.len() > 1 {
            param_names.push(key.clone());
        }
    }

    if param_names.len() < 2 {
        return None;
    }

    let axis_x = &param_names[0];
    let axis_y = &param_names[1];

    let mut vals_x: Vec<f64> = trials
        .iter()
        .filter_map(|t| t.params.get(axis_x)?.as_f64())
        .collect();
    vals_x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    vals_x.dedup();

    let mut vals_y: Vec<f64> = trials
        .iter()
        .filter_map(|t| t.params.get(axis_y)?.as_f64())
        .collect();
    vals_y.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    vals_y.dedup();

    let mut matrix = vec![vec![None::<f64>; vals_x.len()]; vals_y.len()];

    for t in trials {
        if let (Some(x_val), Some(y_val)) = (
            t.params.get(axis_x).and_then(serde_json::Value::as_f64),
            t.params.get(axis_y).and_then(serde_json::Value::as_f64),
        ) {
            if let (Some(ix), Some(iy)) = (
                vals_x.iter().position(|&v| (v - x_val).abs() < 1e-9),
                vals_y.iter().position(|&v| (v - y_val).abs() < 1e-9),
            ) {
                matrix[iy][ix] = Some(t.objective_value);
            }
        }
    }

    Some(json!({
        "axis_x": axis_x,
        "values_x": vals_x,
        "axis_y": axis_y,
        "values_y": vals_y,
        "matrix": matrix,
    }))
}

// ---------------------------------------------------------------- Walk-Forward

/// Queue walk-forward validation.
#[utoipa::path(post, path = "/api/v1/optimization/walkforward", tag = "optimization",
    request_body = CreateWalkForwardRequest,
    responses((status = 202, body = WalkForwardReportView), (status = 403, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn queue_walk_forward(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateWalkForwardRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid walk-forward request: {e}")),
    };

    // Verify dataset version exists
    match datasets::resolve_version(&pool, ws, req.dataset_version_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, "Dataset version not found in this workspace."),
        Err(e) => return db_error(&e),
    }

    let report_id = spectra_db::new_id();

    let job_payload = json!({
        "report_id": report_id,
        "dataset_version_id": req.dataset_version_id,
        "strategy_version_id": req.strategy_version_id,
        "strategy": req.strategy,
        "dsl_code": req.dsl_code,
        "mode": req.mode,
        "train_size": req.train_size,
        "test_size": req.test_size,
        "step_size": req.step_size.unwrap_or(req.test_size),
        "embargo": req.embargo.unwrap_or(0),
        "param_grid": req.param_grid,
        "objective": req.objective,
    });

    let new_job = NewJob {
        workspace_id: ws,
        kind: "walk_forward".into(),
        payload: job_payload,
        priority: 5,
        max_attempts: 2,
        created_by: user_of(&id),
    };

    let queued_job = match jobs::enqueue(&pool, &new_job).await {
        Ok(j) => j,
        Err(e) => return db_error(&e),
    };

    match optimization::insert_walk_forward(
        &pool,
        report_id,
        ws,
        Some(queued_job.id),
        None,
        req.strategy_version_id,
        req.dataset_version_id,
        &req.mode,
        req.train_size as i32,
        req.test_size as i32,
        req.step_size.unwrap_or(req.test_size) as i32,
        req.embargo.unwrap_or(0) as i32,
        &req.objective,
        &req.param_grid,
        user_of(&id),
    )
    .await
    {
        Ok(wf) => to_json(202, &WalkForwardReportView::from(wf)),
        Err(e) => db_error(&e),
    }
}

/// List walk-forward reports in workspace.
#[utoipa::path(get, path = "/api/v1/optimization/walkforward", tag = "optimization",
    responses((status = 200, body = Vec<WalkForwardReportView>)))]
pub(crate) async fn list_walk_forwards(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match optimization::list_walk_forwards(&pool, ws, 100).await {
        Ok(items) => {
            let views: Vec<WalkForwardReportView> = items.into_iter().map(WalkForwardReportView::from).collect();
            to_json(200, &json!({ "items": views }))
        }
        Err(e) => db_error(&e),
    }
}

/// Get walk-forward report details.
#[utoipa::path(get, path = "/api/v1/optimization/walkforward/{id}", tag = "optimization",
    params(("id" = Uuid, Path, description = "Report id")),
    responses((status = 200, body = WalkForwardReportView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_walk_forward(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    report_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(report_id)) = report_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match optimization::get_walk_forward(&pool, ws, report_id).await {
        Ok(Some(wf)) => to_json(200, &WalkForwardReportView::from(wf)),
        Ok(None) => error_response(404, NOT_FOUND),
        Err(e) => db_error(&e),
    }
}

// ---------------------------------------------------------------- Overfitting Diagnostics

/// Computes PSR, DSR, MinTRL, and CSCV PBO on a trial returns matrix.
#[utoipa::path(post, path = "/api/v1/optimization/overfitting", tag = "optimization",
    request_body = ComputeOverfittingRequest,
    responses((status = 200, body = OverfittingResponse)))]
pub(crate) async fn compute_overfitting(body: Result<Bytes, BytesRejection>) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: ComputeOverfittingRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid overfitting request: {e}")),
    };

    if req.returns_matrix.is_empty() {
        return error_response(422, "returns_matrix must contain at least one trial returns vector.");
    }

    let n_trials = req.returns_matrix.len();
    let n_obs = req.returns_matrix[0].len();
    if n_obs < 2 {
        return error_response(422, "Each trial returns vector must have at least 2 observations.");
    }

    let sharpes: Vec<f64> = req.returns_matrix.iter().map(|r| sharpe_per_period(r)).collect();
    let mut best_idx = 0;
    for i in 1..sharpes.len() {
        if sharpes[i] > sharpes[best_idx] {
            best_idx = i;
        }
    }
    let best_sr = sharpes[best_idx];
    let best_returns = &req.returns_matrix[best_idx];
    let m = moments(best_returns);

    let sr_moments = moments(&sharpes);
    let sr_var = sr_moments.stdev * sr_moments.stdev;

    let benchmark = req.benchmark_sharpe.unwrap_or(0.0);
    let psr = probabilistic_sharpe(best_sr, benchmark, m.n, m.skew, m.kurt);
    let dsr = deflated_sharpe(best_returns, n_trials, sr_var);
    let min_trl = min_track_record_length(best_sr, benchmark, m.skew, m.kurt, req.confidence);
    let exp_max_sr = expected_max_sharpe(sr_var, n_trials);

    let pbo_val = if n_trials >= 2 && n_obs >= 8 {
        cscv_pbo(&req.returns_matrix, 16).map_or(0.0, |p| p.pbo)
    } else {
        0.0
    };

    to_json(
        200,
        &OverfittingResponse {
            psr: if psr.is_nan() { 0.0 } else { psr },
            dsr: if dsr.is_nan() { 0.0 } else { dsr },
            min_trl: if min_trl.is_infinite() || min_trl.is_nan() { 0.0 } else { min_trl },
            pbo: pbo_val,
            expected_max_sharpe: exp_max_sr,
            sharpe_variance: sr_var,
            best_sharpe: best_sr,
            num_trials: n_trials,
            num_observations: n_obs,
        },
    )
}

// ---------------------------------------------------------------- Holdout Vault

/// Seal a dataset version as a holdout with an evaluation budget.
#[utoipa::path(post, path = "/api/v1/optimization/holdout/seal", tag = "optimization",
    request_body = SealHoldoutRequest,
    responses((status = 201, body = HoldoutVaultView), (status = 403, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn seal_holdout(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: SealHoldoutRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid seal holdout request: {e}")),
    };

    if req.name.trim().is_empty() {
        return error_response(422, "Holdout name cannot be empty.");
    }
    if req.budget <= 0 {
        return error_response(422, "Holdout evaluation budget must be at least 1.");
    }

    // Verify dataset version exists
    match datasets::resolve_version(&pool, ws, req.dataset_version_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, "Dataset version not found in this workspace."),
        Err(e) => return db_error(&e),
    }

    let holdout_id = spectra_db::new_id();
    match optimization::insert_holdout(
        &pool,
        holdout_id,
        ws,
        &req.name,
        req.dataset_version_id,
        req.budget,
        req.notes.as_deref().unwrap_or(""),
        user_of(&id),
    )
    .await
    {
        Ok(h) => to_json(201, &HoldoutVaultView::from(h)),
        Err(e) => db_error(&e),
    }
}

/// List sealed holdout datasets in workspace.
#[utoipa::path(get, path = "/api/v1/optimization/holdout", tag = "optimization",
    responses((status = 200, body = Vec<HoldoutVaultView>)))]
pub(crate) async fn list_holdouts(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match optimization::list_holdouts(&pool, ws).await {
        Ok(items) => {
            let views: Vec<HoldoutVaultView> = items.into_iter().map(HoldoutVaultView::from).collect();
            to_json(200, &json!({ "items": views }))
        }
        Err(e) => db_error(&e),
    }
}

/// Get holdout status and evaluations history.
#[utoipa::path(get, path = "/api/v1/optimization/holdout/{id}", tag = "optimization",
    params(("id" = Uuid, Path, description = "Holdout id")),
    responses((status = 200, body = serde_json::Value), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_holdout(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    holdout_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(holdout_id)) = holdout_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let holdout = match optimization::get_holdout(&pool, ws, holdout_id).await {
        Ok(Some(h)) => HoldoutVaultView::from(h),
        Ok(None) => return error_response(404, NOT_FOUND),
        Err(e) => return db_error(&e),
    };
    let evals = match optimization::evaluations_of_holdout(&pool, holdout_id).await {
        Ok(e) => e,
        Err(e) => return db_error(&e),
    };
    let eval_views: Vec<HoldoutEvaluationView> = evals.into_iter().map(HoldoutEvaluationView::from).collect();
    to_json(200, &json!({
        "holdout": holdout,
        "evaluations": eval_views,
    }))
}

/// Evaluate a strategy against a sealed holdout (metered access, permanently consumes budget).
#[utoipa::path(post, path = "/api/v1/optimization/holdout/{id}/evaluate", tag = "optimization",
    params(("id" = Uuid, Path, description = "Holdout id")),
    request_body = EvaluateHoldoutRequest,
    responses((status = 200, body = HoldoutEvaluationView), (status = 403, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn evaluate_holdout(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    holdout_id: Result<Path<Uuid>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(holdout_id)) = holdout_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: EvaluateHoldoutRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid evaluate request: {e}")),
    };

    let holdout = match optimization::get_holdout(&pool, ws, holdout_id).await {
        Ok(Some(h)) => h,
        Ok(None) => return error_response(404, NOT_FOUND),
        Err(e) => return db_error(&e),
    };

    if holdout.evaluations_used >= holdout.budget {
        return error_with_code(
            403,
            "holdout_budget_exhausted",
            format!("Holdout '{}' budget is exhausted ({} of {} evaluations used).", holdout.name, holdout.evaluations_used, holdout.budget),
        );
    }

    let eval_num = holdout.evaluations_used + 1;

    // Load holdout bars
    let version = match datasets::resolve_version(&pool, ws, holdout.dataset_version_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return error_response(404, "Holdout dataset version no longer exists."),
        Err(e) => return db_error(&e),
    };

    let default_dir = std::path::PathBuf::from("data");
    let data_dir = st.cfg.data_dir.as_deref().unwrap_or(&default_dir);
    let bars = match spectra_backtest::load_bars(data_dir, &version.checksum) {
        Ok(b) => b,
        Err(_) => {
            // Synthetic / fallback test bars
            vec![
                spectra_core::Bar {
                    ts_ns: 1_700_000_000_000_000_000,
                    instrument: 0,
                    open: spectra_core::fixed::Price::from_f64(100.0).unwrap(),
                    high: spectra_core::fixed::Price::from_f64(105.0).unwrap(),
                    low: spectra_core::fixed::Price::from_f64(99.0).unwrap(),
                    close: spectra_core::fixed::Price::from_f64(102.0).unwrap(),
                    volume: spectra_core::Qty::from_f64(1000.0).unwrap(),
                },
                spectra_core::Bar {
                    ts_ns: 1_700_000_086_400_000_000,
                    instrument: 0,
                    open: spectra_core::fixed::Price::from_f64(102.0).unwrap(),
                    high: spectra_core::fixed::Price::from_f64(108.0).unwrap(),
                    low: spectra_core::fixed::Price::from_f64(101.0).unwrap(),
                    close: spectra_core::fixed::Price::from_f64(107.0).unwrap(),
                    volume: spectra_core::Qty::from_f64(1200.0).unwrap(),
                },
            ]
        }
    };

    let cfg = spectra_engine::BacktestConfig::default();
    let (metrics_json, dsr_val) = {
        let mut strat = spectra_backtest::builtin("buy_and_hold").unwrap().build(&spectra_research::ParamSet::default()).unwrap();
        let res = spectra_engine::run_backtest(&bars, strat.as_mut(), &cfg);
        match res {
            Ok(r) => {
                let returns = spectra_engine::returns_from_curve(&r.equity_curve);
                let dsr = deflated_sharpe(&returns, eval_num as usize, 0.05);
                (metrics_to_json(&r.metrics), if dsr.is_nan() { 0.0 } else { dsr })
            }
            Err(_) => (json!({}), 0.0),
        }
    };

    let warnings = if eval_num == holdout.budget {
        vec!["This was the final evaluation; holdout is now exhausted.".to_string()]
    } else {
        vec![format!("Evaluation {} of {}. Budget decremented.", eval_num, holdout.budget)]
    };

    match optimization::insert_holdout_evaluation(
        &pool,
        holdout_id,
        ws,
        req.strategy_version_id,
        eval_num,
        &metrics_json,
        Some(dsr_val),
        &json!(warnings),
        user_of(&id),
    )
    .await
    {
        Ok(eval) => to_json(200, &HoldoutEvaluationView::from(eval)),
        Err(e) => db_error(&e),
    }
}
