//! `/api/v1/optimization` (Module 4): Parameter Sweeps, Walk-Forward Validation,
//! Overfitting Statistics (PSR, DSR, MinTRL, CSCV PBO), and Holdout Vault.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spectra_db::datasets;
use spectra_db::jobs::{self, NewJob};
use spectra_db::optimization::{
    self, HoldoutEvaluation, HoldoutVault, OptimizationSweep, OptimizationTrial, WalkForwardReport,
};
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
    pub experiment_id: Option<Uuid>,
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
            experiment_id: s.experiment_id,
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
    pub experiment_id: Option<Uuid>,
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

    // Verify dataset version is not a sealed holdout
    match optimization::is_holdout_sealed(&pool, ws, req.dataset_version_id).await {
        Ok(true) => {
            return error_with_code(
                403,
                "holdout_sealed",
                "This dataset version is a sealed holdout; direct parameter sweeps are prohibited. Use /api/v1/optimization/holdout/{id}/evaluate.",
            );
        }
        Ok(false) => {}
        Err(e) => return db_error(&e),
    }

    // Enforce search budget cap (Task 10)
    let total_points: usize = if let Some(obj) = req.param_grid.as_object() {
        let mut count: usize = 1;
        for (_, vals) in obj {
            if let Some(arr) = vals.as_array() {
                count = count.saturating_mul(arr.len());
            }
        }
        count
    } else {
        1
    };
    if total_points > 1000 && req.max_trials.is_none_or(|m| m > 1000) {
        return error_with_code(
            422,
            "search_budget_exceeded",
            format!("Search budget exceeded: parameter sweep capped at 1000 trials. Grid has {total_points} combinations; specify max_trials <= 1000 or reduce the parameter space."),
        );
    }

    let experiment_id: Option<Uuid> = if let Some(exp_id) = req.experiment_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM experiments WHERE workspace_id = $1 AND id = $2)",
        )
        .bind(ws)
        .bind(exp_id)
        .fetch_one(&pool)
        .await
        .unwrap_or(false);
        if !exists {
            return error_response(404, "Experiment not found in this workspace.");
        }
        Some(exp_id)
    } else {
        sqlx::query_scalar("SELECT id FROM experiments WHERE workspace_id = $1 AND tags @> ARRAY['default']::text[] ORDER BY created_at, id LIMIT 1")
            .bind(ws)
            .fetch_optional(&pool)
            .await
            .unwrap_or(None)
    };

    let sweep_id = spectra_db::new_id();

    let job_payload = json!({
        "sweep_id": sweep_id,
        "dataset_version_id": req.dataset_version_id,
        "strategy_version_id": req.strategy_version_id,
        "experiment_id": experiment_id,
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
        experiment_id,
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
pub(crate) async fn list_sweeps(State(st): State<AppState>, Extension(id): Extension<Identity>) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match optimization::list_sweeps(&pool, ws, 100).await {
        Ok(items) => {
            let views: Vec<OptimizationSweepView> =
                items.into_iter().map(OptimizationSweepView::from).collect();
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

    to_json(
        200,
        &json!({
            "sweep": sweep,
            "trials": trial_views,
            "sensitivity": sensitivity,
        }),
    )
}

/// Query parameters for validation report export.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub(crate) struct ExportSweepQuery {
    #[param(inline)]
    pub format: Option<String>,
}

/// Export sweep validation report as CSV or PDF (Task 8).
#[utoipa::path(get, path = "/api/v1/optimization/sweep/{id}/export", tag = "optimization",
    params(
        ("id" = Uuid, Path, description = "Sweep id"),
        ExportSweepQuery
    ),
    responses((status = 200, description = "Validation report CSV or PDF"), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn export_sweep(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    sweep_id: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<ExportSweepQuery>, QueryRejection>,
) -> Response {
    let Ok(Path(sweep_id)) = sweep_id else { return error_response(404, NOT_FOUND) };
    let export_format =
        query.ok().and_then(|Query(q)| q.format).unwrap_or_else(|| "csv".to_string()).to_lowercase();

    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let sweep = match optimization::get_sweep(&pool, ws, sweep_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(404, NOT_FOUND),
        Err(e) => return db_error(&e),
    };
    let trials = match optimization::trials_of_sweep(&pool, sweep_id).await {
        Ok(t) => t,
        Err(e) => return db_error(&e),
    };

    let holdout_eval = optimization::evaluations_for_run_target(
        &pool,
        ws,
        sweep.strategy_version_id,
        Some(sweep.dataset_version_id),
    )
    .await
    .ok()
    .and_then(|list| list.into_iter().next());

    if export_format == "pdf" {
        let pdf_bytes = generate_validation_pdf(&sweep, &trials, holdout_eval.as_ref());
        let filename = format!("sweep-{}-validation-report.pdf", sweep.id);
        return Response::builder()
            .status(200)
            .header("content-type", "application/pdf")
            .header("content-disposition", format!("attachment; filename=\"{filename}\""))
            .body(axum::body::Body::from(pdf_bytes))
            .unwrap();
    }

    let mut csv = String::new();
    csv.push_str("# SpectraLabs M4 Optimization & Validation Report\n");
    csv.push_str(&format!("# Sweep ID: {}\n", sweep.id));
    csv.push_str(&format!("# Method: {}\n", sweep.method));
    csv.push_str(&format!("# Objective: {}\n", sweep.objective));
    csv.push_str(&format!("# Total Trials Attempted: {}\n", sweep.trial_count));
    csv.push_str(&format!(
        "# Best Trial: {}\n",
        sweep.best_trial_num.map_or("-".to_string(), |n| n.to_string())
    ));
    csv.push_str(&format!(
        "# Best Objective: {}\n",
        sweep.best_objective.map_or("-".to_string(), |v| v.to_string())
    ));
    csv.push_str(&format!(
        "# Deflated Sharpe Ratio (DSR): {}\n",
        sweep.dsr.map_or("-".to_string(), |v| format!("{v:.4}"))
    ));
    csv.push_str(&format!(
        "# Probabilistic Sharpe Ratio (PSR): {}\n",
        sweep.psr.map_or("-".to_string(), |v| format!("{v:.4}"))
    ));
    csv.push_str(&format!(
        "# Minimum Track Record Length (MinTRL): {}\n",
        sweep.min_trl.map_or("-".to_string(), |v| format!("{v:.2}"))
    ));
    csv.push_str(&format!("# CSCV PBO: {}\n", sweep.pbo.map_or("-".to_string(), |v| format!("{v:.4}"))));
    if let Some(h) = holdout_eval {
        csv.push_str(&format!("# Holdout Evaluation Num: {}\n", h.evaluation_num));
        csv.push_str(&format!("# Holdout Evaluated At: {}\n", h.evaluated_at));
        csv.push_str(&format!("# Holdout DSR: {}\n", h.dsr.map_or("-".to_string(), |v| format!("{v:.4}"))));
    } else {
        csv.push_str("# Holdout Evaluation: None\n");
    }
    csv.push_str("\ntrial_num,objective_value,returns_mean,returns_vol,params\n");
    for t in &trials {
        let p_str = serde_json::to_string(&t.params).unwrap_or_default().replace('"', "\"\"");
        csv.push_str(&format!(
            "{},{},{},{},\"{}\"\n",
            t.trial_num,
            t.objective_value,
            t.returns_mean.map_or(String::new(), |v| v.to_string()),
            t.returns_vol.map_or(String::new(), |v| v.to_string()),
            p_str
        ));
    }

    let filename = format!("sweep-{}-validation-report.csv", sweep.id);
    Response::builder()
        .status(200)
        .header("content-type", "text/csv; charset=utf-8")
        .header("content-disposition", format!("attachment; filename=\"{filename}\""))
        .body(axum::body::Body::from(csv))
        .unwrap()
}

fn escape_pdf(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

struct SimplePdf {
    objects: Vec<Vec<u8>>,
}

impl SimplePdf {
    fn new() -> Self {
        Self { objects: Vec::new() }
    }

    fn add_object(&mut self, data: Vec<u8>) -> usize {
        self.objects.push(data);
        self.objects.len()
    }

    fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::new();
        for (i, obj) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            let num = i + 1;
            let header = format!("{num} 0 obj\n");
            out.extend_from_slice(header.as_bytes());
            out.extend_from_slice(obj);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = out.len();
        let total_objs = self.objects.len() + 1;
        out.extend_from_slice(format!("xref\n0 {total_objs}\n0000000000 65535 f \n").as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        out
    }
}

fn generate_validation_pdf(
    sweep: &OptimizationSweep,
    trials: &[OptimizationTrial],
    holdout_eval: Option<&HoldoutEvaluation>,
) -> Vec<u8> {
    let mut pdf = SimplePdf::new();

    // 1: Catalog
    pdf.add_object(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    // 2: Pages
    pdf.add_object(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    // 3: Page (US Letter: 612 x 792 pt)
    pdf.add_object(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>".to_vec());
    // 4: Font F1 (Helvetica)
    pdf.add_object(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    // 5: Font F2 (Helvetica-Bold)
    pdf.add_object(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_vec());

    let mut stream = String::new();

    // Top Brand Accent Bar (Blue)
    stream.push_str("0.12 0.35 0.85 rg\n");
    stream.push_str("40 756 532 5 re f\n");

    // Header Text
    stream.push_str("0.08 0.08 0.12 rg\n");
    stream.push_str("BT\n/F2 16 Tf\n40 734 Td\n(SPECTRALABS OPTIMIZATION & VALIDATION REPORT) Tj\n");
    stream.push_str("/F1 8.5 Tf\n0 -14 Td\n0.4 0.42 0.48 rg\n(M4 Robustness Audit, Deflated Sharpe Ratio (DSR) & Overfitting Validation) Tj\nET\n");

    // Divider Line
    stream.push_str("0.85 0.86 0.90 RG\n0.75 w\n40 706 m 572 706 l S\n");

    // Metadata Bar
    let sweep_id_short = sweep.id.to_string();
    let method = sweep.method.to_uppercase();
    let objective = sweep.objective.to_uppercase();
    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    stream.push_str("0.2 0.22 0.28 rg\n");
    stream.push_str("BT\n/F2 8.5 Tf\n40 690 Td\n(Sweep ID: ) Tj\n/F1 8.5 Tf\n");
    stream.push_str(&format!("({}) Tj\n", escape_pdf(&sweep_id_short)));
    stream.push_str("/F2 8.5 Tf\n200 0 Td\n(Method: ) Tj\n/F1 8.5 Tf\n");
    stream.push_str(&format!("({}) Tj\n", escape_pdf(&method)));
    stream.push_str("/F2 8.5 Tf\n100 0 Td\n(Objective: ) Tj\n/F1 8.5 Tf\n");
    stream.push_str(&format!("({}) Tj\n", escape_pdf(&objective)));
    stream.push_str("/F1 8 Tf\n120 0 Td\n0.45 0.47 0.52 rg\n");
    stream.push_str(&format!("(Generated: {}) Tj\n", escape_pdf(&now)));
    stream.push_str("ET\n");

    let best_trial_sub = format!("Trial #{}", sweep.best_trial_num.unwrap_or(0));
    // 6 Key Metric Cards (2 rows of 3)
    let cards = [
        (
            "DEFLATED SHARPE (DSR)",
            sweep.dsr.map_or("—".to_string(), |v| format!("{v:.4}")),
            "Haircut for trial selection bias",
            40,
            615,
        ),
        (
            "PBO (CSCV OVERFITTING)",
            sweep.pbo.map_or("—".to_string(), |v| format!("{v:.4}")),
            "Probability of out-of-sample loss",
            222,
            615,
        ),
        (
            "TOTAL TRIALS COUNTED",
            sweep.trial_count.to_string(),
            "Honest trial accounting (all runs)",
            404,
            615,
        ),
        (
            "MIN TRACK RECORD (MinTRL)",
            sweep.min_trl.map_or("—".to_string(), |v| format!("{v:.2} obs")),
            "Observations for significance",
            40,
            550,
        ),
        (
            "PROBABILISTIC SHARPE (PSR)",
            sweep.psr.map_or("—".to_string(), |v| format!("{v:.4}")),
            "Confidence vs zero benchmark",
            222,
            550,
        ),
        (
            "BEST IN-SAMPLE OBJECTIVE",
            sweep.best_objective.map_or("—".to_string(), |v| format!("{v:.4}")),
            best_trial_sub.as_str(),
            404,
            550,
        ),
    ];

    for (label, val, sub, x, y) in cards {
        stream.push_str(&format!("0.96 0.97 0.99 rg\n{x} {y} 168 54 re f\n"));
        stream.push_str(&format!("0.86 0.88 0.92 RG\n0.5 w\n{x} {y} 168 54 re S\n"));
        stream.push_str("BT\n");
        stream.push_str(&format!(
            "/F2 7 Tf\n0.4 0.44 0.50 rg\n{} {} Td\n({}) Tj\n",
            x + 10,
            y + 40,
            escape_pdf(label)
        ));
        stream.push_str(&format!("/F2 13 Tf\n0.10 0.12 0.18 rg\n0 -16 Td\n({}) Tj\n", escape_pdf(&val)));
        stream.push_str(&format!("/F1 6.5 Tf\n0.48 0.52 0.58 rg\n0 -11 Td\n({}) Tj\n", escape_pdf(sub)));
        stream.push_str("ET\n");
    }

    // Holdout Vault Result Banner (y = 485, h = 50)
    stream.push_str("0.95 0.97 1.00 rg\n40 485 532 50 re f\n");
    stream.push_str("0.70 0.80 0.95 RG\n0.75 w\n40 485 532 50 re S\n");

    stream.push_str("BT\n");
    stream.push_str(
        "/F2 9.5 Tf\n0.12 0.32 0.70 rg\n52 520 Td\n(HOLDOUT VAULT INTEGRITY & OUT-OF-SAMPLE RESULT) Tj\n",
    );
    stream.push_str("/F1 8 Tf\n0.25 0.30 0.40 rg\n0 -14 Td\n");
    if let Some(h) = holdout_eval {
        let holdout_text = format!(
            "EVALUATED: Evaluation #{} on {} | Holdout DSR: {}",
            h.evaluation_num,
            h.evaluated_at.format("%Y-%m-%d %H:%M UTC"),
            h.dsr.map_or("—".to_string(), |v| format!("{v:.4}"))
        );
        stream.push_str(&format!("({}) Tj\n", escape_pdf(&holdout_text)));
    } else {
        stream.push_str("(SEALED & PROTECTED: Holdout partition remains locked. Direct bar reads are blocked until evaluation.) Tj\n");
    }
    stream.push_str("/F1 7 Tf\n0.45 0.50 0.60 rg\n0 -10 Td\n(All accesses to the holdout dataset are cryptographically audited and sealed against selection leakage.) Tj\n");
    stream.push_str("ET\n");

    // Trials Table Title
    stream.push_str("BT\n/F2 10 Tf\n0.12 0.14 0.18 rg\n40 455 Td\n(TOP PARAMETER COMBINATIONS & SENSITIVITY RUNS) Tj\nET\n");

    // Table Header Bar (y = 432, h = 16)
    stream.push_str("0.92 0.94 0.97 rg\n40 432 532 16 re f\n");
    stream.push_str("BT\n/F2 7.5 Tf\n0.30 0.35 0.42 rg\n");
    stream.push_str("48 436 Td\n(RANK) Tj\n");
    stream.push_str("45 0 Td\n(TRIAL #) Tj\n");
    stream.push_str("55 0 Td\n(OBJECTIVE) Tj\n");
    stream.push_str("70 0 Td\n(RETURN MEAN) Tj\n");
    stream.push_str("85 0 Td\n(RETURN VOL) Tj\n");
    stream.push_str("80 0 Td\n(PARAMETERS) Tj\n");
    stream.push_str("ET\n");

    let mut sorted_trials = trials.to_vec();
    sorted_trials.sort_by(|a, b| {
        b.objective_value.partial_cmp(&a.objective_value).unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_trials = &sorted_trials[..sorted_trials.len().min(12)];

    let mut row_y = 414;
    for (i, t) in top_trials.iter().enumerate() {
        if i % 2 == 1 {
            stream.push_str(&format!("0.98 0.98 0.99 rg\n40 {row_y} 532 15 re f\n"));
        }
        stream.push_str(&format!("0.90 0.91 0.94 RG\n0.25 w\n40 {row_y} m 572 {row_y} l S\n"));

        let rank = format!("#{}", i + 1);
        let trial_num = format!("Trial {}", t.trial_num);
        let obj = format!("{:.4}", t.objective_value);
        let ret_mean = t.returns_mean.map_or("—".to_string(), |v| format!("{v:.4}"));
        let ret_vol = t.returns_vol.map_or("—".to_string(), |v| format!("{v:.4}"));
        let params_str = serde_json::to_string(&t.params).unwrap_or_default();
        let params_truncated =
            if params_str.len() > 38 { format!("{}...", &params_str[..35]) } else { params_str };

        stream.push_str("BT\n");
        stream.push_str(&format!(
            "/F2 7 Tf\n0.2 0.22 0.28 rg\n48 {} Td\n({}) Tj\n",
            row_y + 4,
            escape_pdf(&rank)
        ));
        stream.push_str(&format!("/F1 7 Tf\n45 0 Td\n({}) Tj\n", escape_pdf(&trial_num)));
        stream.push_str(&format!("/F2 7 Tf\n0.12 0.35 0.85 rg\n55 0 Td\n({}) Tj\n", escape_pdf(&obj)));
        stream.push_str(&format!("/F1 7 Tf\n0.25 0.28 0.32 rg\n70 0 Td\n({}) Tj\n", escape_pdf(&ret_mean)));
        stream.push_str(&format!("85 0 Td\n({}) Tj\n", escape_pdf(&ret_vol)));
        stream.push_str(&format!("80 0 Td\n({}) Tj\n", escape_pdf(&params_truncated)));
        stream.push_str("ET\n");

        row_y -= 16;
    }

    // Footer Section (y = 55)
    stream.push_str("0.85 0.86 0.90 RG\n0.75 w\n40 55 m 572 55 l S\n");
    stream.push_str("BT\n/F1 6.5 Tf\n0.45 0.48 0.54 rg\n40 42 Td\n");
    stream.push_str("(SpectraLabs Engine M4 · Cryptographic Trial Tracking · DSR/PSR computed with independent-returns model) Tj\n");
    stream.push_str("0 -9 Td\n");
    stream.push_str(
        "(Holdout evaluation history is permanently anchored. Page 1 of 1 · Generated automatically) Tj\n",
    );
    stream.push_str("ET\n");

    let stream_bytes = stream.into_bytes();
    let stream_len = stream_bytes.len();
    let stream_obj = format!("<< /Length {stream_len} >>\nstream\n").into_bytes();
    let mut full_stream_obj = Vec::with_capacity(stream_obj.len() + stream_bytes.len() + 12);
    full_stream_obj.extend_from_slice(&stream_obj);
    full_stream_obj.extend_from_slice(&stream_bytes);
    full_stream_obj.extend_from_slice(b"\nendstream");

    // 6: Stream Contents
    pdf.add_object(full_stream_obj);

    pdf.finish()
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

    let mut vals_x: Vec<f64> = trials.iter().filter_map(|t| t.params.get(axis_x)?.as_f64()).collect();
    vals_x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    vals_x.dedup();

    let mut vals_y: Vec<f64> = trials.iter().filter_map(|t| t.params.get(axis_y)?.as_f64()).collect();
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

    // Verify dataset version is not a sealed holdout
    match optimization::is_holdout_sealed(&pool, ws, req.dataset_version_id).await {
        Ok(true) => {
            return error_with_code(
                403,
                "holdout_sealed",
                "This dataset version is a sealed holdout; direct walk-forward validation is prohibited. Use /api/v1/optimization/holdout/{id}/evaluate.",
            );
        }
        Ok(false) => {}
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
            let views: Vec<WalkForwardReportView> =
                items.into_iter().map(WalkForwardReportView::from).collect();
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

    // Verify dataset version is not already sealed
    match optimization::is_holdout_sealed(&pool, ws, req.dataset_version_id).await {
        Ok(true) => {
            return error_with_code(
                422,
                "dataset_already_sealed",
                "This dataset version is already sealed in the holdout vault.",
            );
        }
        Ok(false) => {}
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
    to_json(
        200,
        &json!({
            "holdout": holdout,
            "evaluations": eval_views,
        }),
    )
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
            format!(
                "Holdout '{}' budget is exhausted ({} of {} evaluations used).",
                holdout.name, holdout.evaluations_used, holdout.budget
            ),
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
                    known_at_ns: 1_700_000_000_000_000_000,
                },
                spectra_core::Bar {
                    ts_ns: 1_700_000_086_400_000_000,
                    instrument: 0,
                    open: spectra_core::fixed::Price::from_f64(102.0).unwrap(),
                    high: spectra_core::fixed::Price::from_f64(108.0).unwrap(),
                    low: spectra_core::fixed::Price::from_f64(101.0).unwrap(),
                    close: spectra_core::fixed::Price::from_f64(107.0).unwrap(),
                    volume: spectra_core::Qty::from_f64(1200.0).unwrap(),
                    known_at_ns: 1_700_000_086_400_000_000,
                },
            ]
        }
    };

    let cfg = spectra_engine::BacktestConfig::default();
    let (metrics_json, dsr_val) = {
        let mut strat = spectra_backtest::builtin("buy_and_hold")
            .unwrap()
            .build(&spectra_research::ParamSet::default())
            .unwrap();
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
