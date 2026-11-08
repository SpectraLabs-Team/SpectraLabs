//! Chart drawings and saved layout templates (M36).
//!
//! Stores user chart annotations (trendlines, levels, Fibonacci, text) and saved templates
//! with workspace scoping, edit permissions, count limits, and soft-delete recovery.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Maximum number of active drawings allowed on a single chart.
pub const MAX_DRAWINGS_PER_CHART: i64 = 200;

/// Maximum payload size for a single drawing (64 KB).
pub const MAX_DRAWING_BYTES: usize = 64 * 1024;

/// Maximum number of saved chart templates per workspace.
pub const MAX_TEMPLATES_PER_WORKSPACE: i64 = 50;

/// Error returned when drawing operations fail validation.
#[derive(Debug)]
pub enum DrawingError {
    LimitExceeded,
    PayloadTooLarge,
    InvalidTool(String),
    Db(sqlx::Error),
    Serialization(serde_json::Error),
}

impl std::fmt::Display for DrawingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitExceeded => {
                write!(f, "Chart has reached the maximum drawing limit of {MAX_DRAWINGS_PER_CHART}")
            }
            Self::PayloadTooLarge => {
                write!(f, "Drawing payload size exceeds maximum of {MAX_DRAWING_BYTES} bytes")
            }
            Self::InvalidTool(tool) => write!(f, "Invalid drawing tool: {tool}"),
            Self::Db(e) => write!(f, "Database error: {e}"),
            Self::Serialization(e) => write!(f, "Serialization error: {e}"),
        }
    }
}

impl std::error::Error for DrawingError {}

impl From<sqlx::Error> for DrawingError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl From<serde_json::Error> for DrawingError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e)
    }
}

/// Error returned when template operations fail validation.
#[derive(Debug)]
pub enum TemplateError {
    LimitExceeded,
    InvalidName,
    Db(sqlx::Error),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitExceeded => {
                write!(f, "Workspace has reached the maximum template limit of {MAX_TEMPLATES_PER_WORKSPACE}")
            }
            Self::InvalidName => {
                write!(f, "Template name cannot be empty or longer than 100 characters")
            }
            Self::Db(e) => write!(f, "Database error: {e}"),
        }
    }
}

impl std::error::Error for TemplateError {}

impl From<sqlx::Error> for TemplateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// A stored chart drawing row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct DrawingRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub chart_id: String,
    pub dataset_version_id: Option<String>,
    pub tool: String,
    pub points: Value,
    pub style: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Parameters for creating a new chart drawing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDrawing {
    pub chart_id: String,
    pub dataset_version_id: Option<String>,
    pub tool: String,
    pub points: Value,
    pub style: Value,
}

/// Lists all active (non-deleted) drawings for a given chart in a workspace.
pub async fn list_active_drawings(
    pool: &PgPool,
    workspace_id: Uuid,
    chart_id: &str,
) -> Result<Vec<DrawingRow>, sqlx::Error> {
    sqlx::query_as::<_, DrawingRow>(
        "SELECT id, workspace_id, user_id, chart_id, dataset_version_id, \
                tool, points, style, created_at, updated_at, deleted_at \
         FROM chart_drawings \
         WHERE workspace_id = $1 AND chart_id = $2 AND deleted_at IS NULL \
         ORDER BY created_at ASC",
    )
    .bind(workspace_id)
    .bind(chart_id)
    .fetch_all(pool)
    .await
}

/// Counts active drawings on a chart.
pub async fn count_active_drawings(
    pool: &PgPool,
    workspace_id: Uuid,
    chart_id: &str,
) -> Result<i64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chart_drawings \
         WHERE workspace_id = $1 AND chart_id = $2 AND deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(chart_id)
    .fetch_one(pool)
    .await?;

    Ok(count)
}

/// Creates a new drawing on a chart, enforcing the 200-drawing limit.
pub async fn create_drawing(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    new_drawing: &NewDrawing,
) -> Result<DrawingRow, DrawingError> {
    if !matches!(new_drawing.tool.as_str(), "trendline" | "horizontal_level" | "fibonacci" | "text") {
        return Err(DrawingError::InvalidTool(new_drawing.tool.clone()));
    }

    let payload_bytes =
        serde_json::to_vec(&new_drawing.points)?.len() + serde_json::to_vec(&new_drawing.style)?.len();
    if payload_bytes > MAX_DRAWING_BYTES {
        return Err(DrawingError::PayloadTooLarge);
    }

    let current_count = count_active_drawings(pool, workspace_id, &new_drawing.chart_id).await?;
    if current_count >= MAX_DRAWINGS_PER_CHART {
        return Err(DrawingError::LimitExceeded);
    }

    let row = sqlx::query_as::<_, DrawingRow>(
        "INSERT INTO chart_drawings ( \
             workspace_id, user_id, chart_id, dataset_version_id, \
             tool, points, style \
         ) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, workspace_id, user_id, chart_id, dataset_version_id, \
                   tool, points, style, created_at, updated_at, deleted_at",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(&new_drawing.chart_id)
    .bind(&new_drawing.dataset_version_id)
    .bind(&new_drawing.tool)
    .bind(&new_drawing.points)
    .bind(&new_drawing.style)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Updates points or style on an existing drawing.
pub async fn update_drawing(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    points: Option<Value>,
    style: Option<Value>,
) -> Result<Option<DrawingRow>, DrawingError> {
    let row = sqlx::query_as::<_, DrawingRow>(
        "UPDATE chart_drawings \
         SET points = COALESCE($3, points), \
             style = COALESCE($4, style), \
             updated_at = clock_timestamp() \
         WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL \
         RETURNING id, workspace_id, user_id, chart_id, dataset_version_id, \
                   tool, points, style, created_at, updated_at, deleted_at",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(points)
    .bind(style)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Soft-deletes a drawing by setting `deleted_at = clock_timestamp()`.
pub async fn soft_delete_drawing(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE chart_drawings \
         SET deleted_at = clock_timestamp() \
         WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(workspace_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Fetches a single drawing by ID (including soft-deleted if needed).
pub async fn get_drawing(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<DrawingRow>, sqlx::Error> {
    sqlx::query_as::<_, DrawingRow>(
        "SELECT id, workspace_id, user_id, chart_id, dataset_version_id, \
                tool, points, style, created_at, updated_at, deleted_at \
         FROM chart_drawings \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

/// A stored chart template row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct TemplateRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub version: i32,
    pub indicators: Value,
    pub drawings: Value,
    pub comparisons: Value,
    pub dataset_versions: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Parameters for creating a new chart template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTemplate {
    pub name: String,
    pub version: i32,
    pub indicators: Value,
    pub drawings: Value,
    pub comparisons: Value,
    pub dataset_versions: Value,
}

/// Lists all templates for a workspace.
pub async fn list_templates(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<TemplateRow>, sqlx::Error> {
    sqlx::query_as::<_, TemplateRow>(
        "SELECT id, workspace_id, user_id, name, version, \
                indicators, drawings, comparisons, dataset_versions, \
                created_at, updated_at \
         FROM chart_templates \
         WHERE workspace_id = $1 \
         ORDER BY name ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

/// Counts templates in a workspace.
pub async fn count_templates(pool: &PgPool, workspace_id: Uuid) -> Result<i64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM chart_templates WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(pool)
        .await?;

    Ok(count)
}

/// Creates a new template, enforcing the 50-template limit per workspace.
pub async fn create_template(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    new_template: &NewTemplate,
) -> Result<TemplateRow, TemplateError> {
    let trimmed_name = new_template.name.trim();
    if trimmed_name.is_empty() || trimmed_name.len() > 100 {
        return Err(TemplateError::InvalidName);
    }

    let count = count_templates(pool, workspace_id).await?;
    if count >= MAX_TEMPLATES_PER_WORKSPACE {
        return Err(TemplateError::LimitExceeded);
    }

    let row = sqlx::query_as::<_, TemplateRow>(
        "INSERT INTO chart_templates ( \
             workspace_id, user_id, name, version, \
             indicators, drawings, comparisons, dataset_versions \
         ) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id, workspace_id, user_id, name, version, \
                   indicators, drawings, comparisons, dataset_versions, \
                   created_at, updated_at",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(trimmed_name)
    .bind(new_template.version)
    .bind(&new_template.indicators)
    .bind(&new_template.drawings)
    .bind(&new_template.comparisons)
    .bind(&new_template.dataset_versions)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Fetches a template by ID in a workspace.
pub async fn get_template(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<TemplateRow>, sqlx::Error> {
    sqlx::query_as::<_, TemplateRow>(
        "SELECT id, workspace_id, user_id, name, version, \
                indicators, drawings, comparisons, dataset_versions, \
                created_at, updated_at \
         FROM chart_templates \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

/// Deletes a template by ID in a workspace.
pub async fn delete_template(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM chart_templates \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(workspace_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
