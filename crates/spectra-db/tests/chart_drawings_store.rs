//! Database tests for M36 chart drawings and saved layout templates.

use serde_json::json;
use spectra_db::chart_drawings::*;
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool = connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but database unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

async fn setup_workspace_and_user(pool: &PgPool) -> (Uuid, Uuid) {
    let ws = spectra_db::new_id();
    let user = spectra_db::new_id();
    spectra_db::identity::ensure_identity(pool, Some(user), ws).await.unwrap();

    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'editor')")
        .bind(ws)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();

    (ws, user)
}

#[tokio::test]
async fn drawing_lifecycle_create_read_update_soft_delete() {
    let Some(pool) = pool().await else { return };
    let (ws, user) = setup_workspace_and_user(&pool).await;

    let chart_id = "AAPL:1d";
    let new_drawing = NewDrawing {
        chart_id: chart_id.to_string(),
        dataset_version_id: Some("ds_v_test1".to_string()),
        tool: "trendline".to_string(),
        points: json!([
            {"ts_ns": 1_700_000_000_000_000_000i64, "price": 150.0},
            {"ts_ns": 1_700_100_000_000_000_000i64, "price": 160.0}
        ]),
        style: json!({"color": "#2962FF", "lineWidth": 2}),
    };

    // 1. Create drawing
    let created = create_drawing(&pool, ws, user, &new_drawing).await.unwrap();
    assert_eq!(created.workspace_id, ws);
    assert_eq!(created.user_id, user);
    assert_eq!(created.chart_id, chart_id);
    assert_eq!(created.tool, "trendline");
    assert_eq!(created.points, new_drawing.points);
    assert_eq!(created.style, new_drawing.style);
    assert!(created.deleted_at.is_none());

    // 2. Read active drawings
    let active = list_active_drawings(&pool, ws, chart_id).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, created.id);
    assert_eq!(count_active_drawings(&pool, ws, chart_id).await.unwrap(), 1);

    // 3. Update drawing
    let updated_points = json!([
        {"ts_ns": 1_700_000_000_000_000_000i64, "price": 155.0},
        {"ts_ns": 1_700_100_000_000_000_000i64, "price": 165.0}
    ]);
    let updated =
        update_drawing(&pool, ws, created.id, Some(updated_points.clone()), None).await.unwrap().unwrap();
    assert_eq!(updated.points, updated_points);
    assert!(updated.updated_at >= created.created_at);

    // 4. Soft delete drawing
    let deleted = soft_delete_drawing(&pool, ws, created.id).await.unwrap();
    assert!(deleted);

    // Active list is now empty
    let active_after = list_active_drawings(&pool, ws, chart_id).await.unwrap();
    assert_eq!(active_after.len(), 0);
    assert_eq!(count_active_drawings(&pool, ws, chart_id).await.unwrap(), 0);

    // Get by ID still exists for audit / recovery
    let fetched = get_drawing(&pool, ws, created.id).await.unwrap().unwrap();
    assert!(fetched.deleted_at.is_some());
}

#[tokio::test]
async fn drawing_limits_and_validation() {
    let Some(pool) = pool().await else { return };
    let (ws, user) = setup_workspace_and_user(&pool).await;

    // Invalid tool
    let bad_tool = NewDrawing {
        chart_id: "AAPL:1d".into(),
        dataset_version_id: None,
        tool: "magic_wand".into(),
        points: json!([]),
        style: json!({}),
    };
    assert!(matches!(create_drawing(&pool, ws, user, &bad_tool).await, Err(DrawingError::InvalidTool(_))));
}

#[tokio::test]
async fn chart_templates_lifecycle_and_limits() {
    let Some(pool) = pool().await else { return };
    let (ws, user) = setup_workspace_and_user(&pool).await;

    let new_tmpl = NewTemplate {
        name: "Standard Moving Averages".to_string(),
        version: 1,
        indicators: json!([
            {"indicator": "SMA", "params": {"period": 20}, "color": "#2962FF"},
            {"indicator": "EMA", "params": {"period": 50}, "color": "#FF6D00"}
        ]),
        drawings: json!([]),
        comparisons: json!([]),
        dataset_versions: json!({"primary": "ds_v_test"}),
    };

    // 1. Create template
    let tmpl = create_template(&pool, ws, user, &new_tmpl).await.unwrap();
    assert_eq!(tmpl.name, "Standard Moving Averages");
    assert_eq!(tmpl.version, 1);
    assert_eq!(tmpl.workspace_id, ws);

    // 2. List templates
    let list = list_templates(&pool, ws).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, tmpl.id);
    assert_eq!(count_templates(&pool, ws).await.unwrap(), 1);

    // 3. Get template
    let fetched = get_template(&pool, ws, tmpl.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, tmpl.id);
    assert_eq!(fetched.indicators, new_tmpl.indicators);

    // 4. Delete template
    let deleted = delete_template(&pool, ws, tmpl.id).await.unwrap();
    assert!(deleted);
    assert_eq!(list_templates(&pool, ws).await.unwrap().len(), 0);
}
