//! M1b task 12 end-to-end: symbol changes, instrument delistings, and
//! survivorship-free point-in-time universe resolution.
//!
//! When DATABASE_URL is set, exercises database persistence and workspace isolation;
//! when not set, passes without doing anything (like the rest of the workspace).

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
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

struct Fixture {
    app: Router,
    pool: PgPool,
    _dir: std::path::PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-life-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, _dir: dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    fn req(&self, method: &str, path_and_query: &str, body: &str) -> Request<Body> {
        self.req_as(method, path_and_query, body, self.ws)
    }

    fn req_as(&self, method: &str, path_and_query: &str, body: &str, ws: Uuid) -> Request<Body> {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let user = self.user.to_string();
        let ws_str = ws.to_string();
        let req_id = spectra_db::new_id().to_string();
        let can = canonical(method, path_and_query, &ts, &user, &ws_str, &req_id, body.as_bytes());
        let sig = sign(SECRET.as_bytes(), &can);
        Request::builder()
            .method(method)
            .uri(path_and_query)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", ws_str)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }
}

#[tokio::test]
async fn lifecycle_api_flow() {
    let Some(pool) = pool().await else {
        eprintln!("skipping lifecycle_api_flow: DATABASE_URL not set");
        return;
    };
    let f = Fixture::new(pool);

    // 1. Symbol change validation
    let res = f
        .app
        .clone()
        .oneshot(f.req(
            "POST",
            "/api/v1/symbol-changes",
            r#"{"old_symbol":"FB","new_symbol":"FB","effective_date":"2022-06-09"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 422);

    // 2. Create symbol change
    let res = f
        .app
        .clone()
        .oneshot(f.req(
            "POST",
            "/api/v1/symbol-changes",
            r#"{"old_symbol":"FB","new_symbol":"META","effective_date":"2022-06-09","notes":"Rebrand"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let sc: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sc["old_symbol"], "FB");
    assert_eq!(sc["new_symbol"], "META");

    // 3. Create delisting
    let res = f
        .app
        .clone()
        .oneshot(f.req(
            "POST",
            "/api/v1/delistings",
            r#"{"symbol":"ENRON","delisted_at":"2001-11-28","reason":"Bankruptcy"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let del: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(del["symbol"], "ENRON");

    // 4. Create universe
    let res = f
        .app
        .clone()
        .oneshot(f.req(
            "POST",
            "/api/v1/universes",
            r#"{"name":"Tech Leaders","symbols":["FB","ENRON","AAPL"]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let u: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let uid = u["id"].as_str().unwrap();

    // 5. Point-in-time universe resolution: as of 2000-01-01
    // ENRON must be active (not yet delisted), FB must be 'FB'
    let res = f
        .app
        .clone()
        .oneshot(f.req("GET", &format!("/api/v1/universes/{uid}/as-of?as_of=2000-01-01"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let pit: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = pit["items"].as_array().unwrap();
    let enron = items.iter().find(|i| i["symbol"] == "ENRON").unwrap();
    assert_eq!(enron["is_active"], true, "ENRON must be active in 2000 (survivorship bias prevention)");
    let fb = items.iter().find(|i| i["symbol"] == "FB").unwrap();
    assert_eq!(fb["resolved_symbol"], "FB");

    // 6. Point-in-time universe resolution: as of 2023-01-01
    // ENRON is delisted, FB is resolved to META
    let res = f
        .app
        .clone()
        .oneshot(f.req("GET", &format!("/api/v1/universes/{uid}/as-of?as_of=2023-01-01"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let pit23: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items23 = pit23["items"].as_array().unwrap();
    let enron23 = items23.iter().find(|i| i["symbol"] == "ENRON").unwrap();
    assert_eq!(enron23["is_active"], false, "ENRON must be inactive in 2023");
    let fb23 = items23.iter().find(|i| i["symbol"] == "FB").unwrap();
    assert_eq!(fb23["resolved_symbol"], "META", "FB must be resolved to META in 2023");

    // Clean up test workspace
    let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(f.ws).execute(&f.pool).await;
    let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(f.user).execute(&f.pool).await;
}
