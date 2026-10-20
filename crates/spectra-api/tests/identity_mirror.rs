//! M1 task 1: a user who signed up in the frontend (their user and workspace
//! ids exist only in the frontend's database) can use the engine's database
//! routes. Needs a Postgres to own: set `DATABASE_URL` (CI runs a Postgres
//! service). Without it the test passes without doing anything.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::Json;
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

/// A request signed the way the Next.js proxy signs a signed-in user's call.
fn signed(method: &str, path: &str, user: Uuid, workspace: Uuid, body: &str) -> Request<Body> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
    let (user, workspace) = (user.to_string(), workspace.to_string());
    let sig = sign(
        SECRET.as_bytes(),
        &canonical(method, path, &ts, &user, &workspace, "req-mirror", body.as_bytes()),
    );
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-spectra-user", user)
        .header("x-spectra-workspace", workspace)
        .header("x-request-id", "req-mirror")
        .header("x-spectra-timestamp", ts)
        .header("x-spectra-signature", sig)
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn call(app: &Router, req: Request<Body>) -> (u16, Json) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, Json::parse(std::str::from_utf8(&bytes).unwrap()).unwrap_or(Json::Null))
}

#[tokio::test]
async fn a_newly_signed_up_user_can_list_datasets_and_queue_a_backtest() {
    let Some(pool) = pool().await else { return };
    let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));
    // Not the dev seed: ids the engine's database has never seen.
    let (user, workspace) = (spectra_db::new_id(), spectra_db::new_id());

    let (status, j) = call(&app, signed("GET", "/api/v1/datasets", user, workspace, "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert!(j.at("items").unwrap().items().unwrap().is_empty());

    // A dataset version in their workspace (PR 3 adds uploads; here it is registered directly).
    let checksum = format!("{}{}", spectra_db::new_id().simple(), spectra_db::new_id().simple());
    let version = spectra_db::datasets::register_version(
        &pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: workspace,
            dataset_name: "first upload",
            checksum: &checksum,
            row_count: 100,
            byte_size: 4000,
            quality_report: serde_json::json!({}),
            created_by: Some(user),
        },
    )
    .await
    .unwrap();

    let (status, j) = call(&app, signed("GET", "/api/v1/datasets", user, workspace, "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert_eq!(j.at("items").unwrap().items().unwrap().len(), 1);

    let body = format!(r#"{{"dataset_version_id":"{}","strategy":"buy_and_hold"}}"#, version.id);
    let (status, j) = call(&app, signed("POST", "/api/v1/backtests", user, workspace, &body)).await;
    assert_eq!(status, 202, "{}", j.dump());
    let job_id = Uuid::parse_str(j.at("id").unwrap().as_str().unwrap()).unwrap();
    let created_by: Option<Uuid> = sqlx::query_scalar("SELECT created_by FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(created_by, Some(user), "the job is attributed to the signed-in user");

    // Another workspace of a different user cannot see it.
    let (other_user, other_ws) = (spectra_db::new_id(), spectra_db::new_id());
    let (status, j) = call(&app, signed("GET", "/api/v1/datasets", other_user, other_ws, "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert!(j.at("items").unwrap().items().unwrap().is_empty());

    // Leave the shared database as found (workspace rows cascade to jobs and datasets).
    for ws in [workspace, other_ws] {
        sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(ws).execute(&pool).await.unwrap();
    }
    for u in [user, other_user] {
        sqlx::query("DELETE FROM users WHERE id = $1").bind(u).execute(&pool).await.unwrap();
    }
}
