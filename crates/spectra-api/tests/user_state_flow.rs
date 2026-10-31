//! M25.1 per-user state: isolation, version conflicts, membership, viewer rules,
//! one-time import, sandbox orders, and the draft cross-workspace fix.
//!
//! Needs `DATABASE_URL`; without it each test passes without doing anything,
//! like the rest of the workspace's database tests.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";
const STATE: &str = "/api/v1/me/state";

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
    dir: PathBuf,
    ws: Uuid,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let pool = pool().await?;
        let dir = std::env::temp_dir().join(format!("spectra-userstate-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store);
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Some(Self { app, pool, dir, ws: spectra_db::new_id() })
    }

    /// Mirror `user` and make them a `role` member of `ws`.
    async fn join_in(&self, ws: Uuid, user: Uuid, role: &str) {
        spectra_db::identity::ensure_identity(&self.pool, Some(user), ws).await.unwrap();
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3) \
             ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(ws)
        .bind(user)
        .bind(role)
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn join(&self, user: Uuid, role: &str) {
        self.join_in(self.ws, user, role).await;
    }

    /// Send a signed request as `user` in `ws`; returns the status and JSON body (Null when empty).
    async fn call(&self, user: Uuid, ws: Uuid, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user_str, ws_str) = (user.to_string(), ws.to_string());
        let req_id = spectra_db::new_id().to_string();
        let text = if body.is_null() { String::new() } else { body.to_string() };
        let sig = sign(
            SECRET.as_bytes(),
            &canonical(method, path, &ts, &user_str, &ws_str, &req_id, text.as_bytes()),
        );
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("X-Spectra-User", user_str)
            .header("X-Spectra-Workspace", ws_str)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .header("Content-Type", "application/json")
            .body(Body::from(text))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn each_user_sees_only_their_own_state() {
    let Some(f) = Fixture::new().await else { return };
    let (a, b) = (spectra_db::new_id(), spectra_db::new_id());
    f.join(a, "owner").await;
    f.join(b, "owner").await;

    let path = format!("{STATE}/watchlist");
    let (s, saved) = f.call(a, f.ws, "PUT", &path, &json!({"expected_version": 0, "value": ["SPY"]})).await;
    assert_eq!(s, 200);
    assert_eq!(saved["version"], 1);

    let (s, view) = f.call(b, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(s, 200);
    assert_eq!(view["state"]["watchlist"]["version"], 0);
    assert_eq!(view["state"]["watchlist"]["value"], json!([]));

    let (_, view) = f.call(a, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(view["state"]["watchlist"]["value"], json!(["SPY"]));
}

#[tokio::test]
async fn stale_write_returns_409_with_current_value() {
    let Some(f) = Fixture::new().await else { return };
    let a = spectra_db::new_id();
    f.join(a, "owner").await;
    let path = format!("{STATE}/risk-params");

    let (s, _) =
        f.call(a, f.ws, "PUT", &path, &json!({"expected_version": 0, "value": {"max_drawdown": 0.2}})).await;
    assert_eq!(s, 200);

    // Second device still holds version 0: refused, and told the current value.
    let (s, body) =
        f.call(a, f.ws, "PUT", &path, &json!({"expected_version": 0, "value": {"max_drawdown": 0.5}})).await;
    assert_eq!(s, 409);
    assert_eq!(body["code"], "version_conflict");
    assert_eq!(body["current"]["version"], 1);
    assert_eq!(body["current"]["value"]["max_drawdown"], 0.2);
    assert_eq!(body["details"]["current"]["version"], 1);

    let (s, body) =
        f.call(a, f.ws, "PUT", &path, &json!({"expected_version": 1, "value": {"max_drawdown": 0.5}})).await;
    assert_eq!(s, 200);
    assert_eq!(body["version"], 2);
}

#[tokio::test]
async fn non_member_is_refused() {
    let Some(f) = Fixture::new().await else { return };
    let outsider = spectra_db::new_id();

    let (s, body) = f.call(outsider, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(s, 403);
    assert_eq!(body["code"], "not_a_member");

    let path = format!("{STATE}/watchlist");
    let (s, body) =
        f.call(outsider, f.ws, "PUT", &path, &json!({"expected_version": 0, "value": ["SPY"]})).await;
    assert_eq!(s, 403);
    assert_eq!(body["code"], "not_a_member");
}

#[tokio::test]
async fn viewer_reads_but_cannot_write() {
    let Some(f) = Fixture::new().await else { return };
    let (owner, viewer) = (spectra_db::new_id(), spectra_db::new_id());
    f.join(owner, "owner").await;
    f.join(viewer, "viewer").await;

    let path = format!("{STATE}/watchlist");
    let (s, _) = f.call(owner, f.ws, "PUT", &path, &json!({"expected_version": 0, "value": ["SPY"]})).await;
    assert_eq!(s, 200);

    // State is per user: the viewer reads their own (empty) watchlist, not the owner's.
    let (s, view) = f.call(viewer, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(s, 200);
    assert_eq!(view["state"]["watchlist"]["value"], json!([]));
    let (_, owner_view) = f.call(owner, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(owner_view["state"]["watchlist"]["value"], json!(["SPY"]));

    let (s, body) =
        f.call(viewer, f.ws, "PUT", &path, &json!({"expected_version": 1, "value": ["QQQ"]})).await;
    assert_eq!(s, 403);
    assert_eq!(body["code"], "viewer_read_only");

    let (s, body) =
        f.call(viewer, f.ws, "POST", "/api/v1/me/sandbox/orders", &json!({"symbol": "SPY"})).await;
    assert_eq!(s, 403);
    assert_eq!(body["code"], "viewer_read_only");
}

#[tokio::test]
async fn import_keeps_server_data_and_is_idempotent() {
    let Some(f) = Fixture::new().await else { return };
    let a = spectra_db::new_id();
    f.join(a, "owner").await;

    // Server already holds a portfolio for this user.
    let (s, _) = f
        .call(
            a,
            f.ws,
            "PUT",
            &format!("{STATE}/portfolio"),
            &json!({"expected_version": 0, "value": {"csv": "server", "source": "upload"}}),
        )
        .await;
    assert_eq!(s, 200);

    let import = json!({
        "import_key": "browser-1",
        "data": {
            "portfolio": {"csv": "local", "source": null},
            "watchlist": ["QQQ"],
            "sandbox": {"cash": 500.0, "positions": [], "orders": [{"symbol": "SPY"}]}
        }
    });
    let (s, body) = f.call(a, f.ws, "POST", &format!("{STATE}/import"), &import).await;
    assert_eq!(s, 200);
    assert_eq!(body["noop"], false);
    assert!(body["skipped"].as_array().unwrap().contains(&json!("portfolio")));
    assert!(body["imported"].as_array().unwrap().contains(&json!("watchlist")));
    assert!(body["imported"].as_array().unwrap().contains(&json!("sandbox_orders")));

    let (_, view) = f.call(a, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(view["state"]["portfolio"]["value"]["csv"], "server");
    assert_eq!(view["state"]["watchlist"]["value"], json!(["QQQ"]));
    assert_eq!(view["state"]["sandbox"]["value"]["cash"], 500.0);

    // Same key again: no-op.
    let (_, body) = f.call(a, f.ws, "POST", &format!("{STATE}/import"), &import).await;
    assert_eq!(body["noop"], true);

    // A new key cannot overwrite data that is already on the server.
    let (_, body) = f
        .call(
            a,
            f.ws,
            "POST",
            &format!("{STATE}/import"),
            &json!({"import_key": "browser-2", "data": {"watchlist": ["IWM"]}}),
        )
        .await;
    assert_eq!(body["noop"], false);
    assert!(body["skipped"].as_array().unwrap().contains(&json!("watchlist")));
    let (_, view) = f.call(a, f.ws, "GET", STATE, &Value::Null).await;
    assert_eq!(view["state"]["watchlist"]["value"], json!(["QQQ"]));
}

#[tokio::test]
async fn sandbox_orders_append_and_list_oldest_first() {
    let Some(f) = Fixture::new().await else { return };
    let a = spectra_db::new_id();
    f.join(a, "owner").await;
    let path = "/api/v1/me/sandbox/orders";

    let (s, _) = f.call(a, f.ws, "POST", path, &json!({"symbol": "SPY", "qty": 1})).await;
    assert_eq!(s, 201);
    let (s, _) = f.call(a, f.ws, "POST", path, &json!({"symbol": "QQQ", "qty": 2})).await;
    assert_eq!(s, 201);

    // Orders must be objects.
    let (s, _) = f.call(a, f.ws, "POST", path, &json!([1, 2])).await;
    assert_eq!(s, 422);

    let (s, body) = f.call(a, f.ws, "GET", path, &Value::Null).await;
    assert_eq!(s, 200);
    let orders = body["orders"].as_array().unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0]["payload"]["symbol"], "SPY");
    assert_eq!(orders[1]["payload"]["symbol"], "QQQ");
}

#[tokio::test]
async fn null_sandbox_positions_are_stored_as_an_array() {
    let Some(f) = Fixture::new().await else { return };
    let a = spectra_db::new_id();
    f.join(a, "owner").await;

    let (s, body) = f
        .call(
            a,
            f.ws,
            "PUT",
            &format!("{STATE}/sandbox"),
            &json!({"expected_version": 0, "value": {"cash": 10.0, "positions": null}}),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(body["value"]["positions"], json!([]));
}

#[tokio::test]
async fn draft_save_cannot_touch_another_workspaces_strategy() {
    let Some(f) = Fixture::new().await else { return };
    let owner = spectra_db::new_id();
    f.join(owner, "owner").await;

    let (s, created) = f
        .call(
            owner,
            f.ws,
            "POST",
            "/api/v1/strategies",
            &json!({
                "name": "Isolated",
                "description": "",
                "builtin": "sma_cross",
                "params": { "fast": 10, "slow": 50 }
            }),
        )
        .await;
    assert!((200..300).contains(&s));
    let with_draft = created["strategy"]["id"].as_str().unwrap().to_string();

    // A second strategy in the same workspace, with no draft saved yet.
    let (_, created) = f
        .call(
            owner,
            f.ws,
            "POST",
            "/api/v1/strategies",
            &json!({
                "name": "No draft yet",
                "description": "",
                "builtin": "sma_cross",
                "params": { "fast": 10, "slow": 50 }
            }),
        )
        .await;
    let no_draft = created["strategy"]["id"].as_str().unwrap().to_string();

    let (s, _) = f
        .call(
            owner,
            f.ws,
            "PUT",
            &format!("/api/v1/strategies/{with_draft}/draft"),
            &json!({"code": "ORIGINAL", "parameters": {}, "expected_version": 0}),
        )
        .await;
    assert_eq!(s, 200);

    // An intruder in another workspace tries both strategies.
    let (ws2, intruder) = (spectra_db::new_id(), spectra_db::new_id());
    f.join_in(ws2, intruder, "owner").await;
    for id in [&with_draft, &no_draft] {
        let (s, _) = f
            .call(
                intruder,
                ws2,
                "PUT",
                &format!("/api/v1/strategies/{id}/draft"),
                &json!({"code": "HIJACK", "parameters": {}, "expected_version": 0}),
            )
            .await;
        assert_eq!(s, 404, "a save from another workspace must not succeed");
    }

    // The owner's draft is unchanged, and the strategy without a draft still accepts the owner's save.
    let (_, view) =
        f.call(owner, f.ws, "GET", &format!("/api/v1/strategies/{with_draft}/draft"), &Value::Null).await;
    assert_eq!(view["code"], "ORIGINAL");
    let (s, _) = f
        .call(
            owner,
            f.ws,
            "PUT",
            &format!("/api/v1/strategies/{no_draft}/draft"),
            &json!({"code": "FIRST", "parameters": {}, "expected_version": 0}),
        )
        .await;
    assert_eq!(s, 200);
}

#[tokio::test]
async fn sandbox_order_can_be_cancelled_and_reset() {
    let Some(f) = Fixture::new().await else { return };
    let a = spectra_db::new_id();
    f.join(a, "owner").await;
    let path = "/api/v1/me/sandbox/orders";

    let (s, created) = f.call(a, f.ws, "POST", path, &json!({"id": "client-1", "status": "filled"})).await;
    assert_eq!(s, 201);
    let row_id = created["id"].as_str().unwrap().to_string();

    // Cancel: the order's payload is replaced in place.
    let (s, _) = f
        .call(a, f.ws, "PATCH", &format!("{path}/{row_id}"), &json!({"id": "client-1", "status": "canceled"}))
        .await;
    assert_eq!(s, 200);
    let (_, list) = f.call(a, f.ws, "GET", path, &Value::Null).await;
    assert_eq!(list["orders"][0]["payload"]["status"], "canceled");

    // Another user's order id is not found, not updated.
    let other = spectra_db::new_id();
    f.join(other, "owner").await;
    let (s, _) = f
        .call(
            other,
            f.ws,
            "PATCH",
            &format!("{path}/{row_id}"),
            &json!({"id": "client-1", "status": "filled"}),
        )
        .await;
    assert_eq!(s, 404);

    // Reset: every order for this user in this workspace is removed.
    let (s, body) = f.call(a, f.ws, "DELETE", path, &Value::Null).await;
    assert_eq!(s, 200);
    assert_eq!(body["cleared"], 1);
    let (_, list) = f.call(a, f.ws, "GET", path, &Value::Null).await;
    assert_eq!(list["orders"], json!([]));
}

#[tokio::test]
async fn sandbox_trade_is_atomic_and_rejects_stale_writes() {
    let Some(f) = Fixture::new().await else { return };
    let a = spectra_db::new_id();
    f.join(a, "owner").await;
    let trade = "/api/v1/me/sandbox/transact";
    let orders = "/api/v1/me/sandbox/orders";

    // First trade from an empty sandbox (version 0).
    let (s, body) = f
        .call(
            a,
            f.ws,
            "POST",
            trade,
            &json!({
                "expected_version": 0,
                "cash": 900.0,
                "positions": [{"symbol": "SPY", "qty": "1"}],
                "order": {"id": "t-1", "symbol": "SPY", "side": "buy"}
            }),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(body["version"], 1);
    assert_eq!(body["value"]["cash"], 900.0);
    assert_eq!(body["order"]["payload"]["id"], "t-1");

    // A second device still holds version 0: refused, and nothing is written.
    let (s, body) = f
        .call(
            a,
            f.ws,
            "POST",
            trade,
            &json!({
                "expected_version": 0,
                "cash": 500.0,
                "positions": [],
                "order": {"id": "t-2", "symbol": "QQQ", "side": "buy"}
            }),
        )
        .await;
    assert_eq!(s, 409);
    assert_eq!(body["code"], "version_conflict");
    assert_eq!(body["details"]["current"]["version"], 1);
    assert_eq!(body["details"]["current"]["value"]["cash"], 900.0);

    let (_, list) = f.call(a, f.ws, "GET", orders, &Value::Null).await;
    assert_eq!(list["orders"].as_array().unwrap().len(), 1, "a refused trade must not add an order");

    // Trade again on the current version.
    let (s, body) = f
        .call(
            a,
            f.ws,
            "POST",
            trade,
            &json!({
                "expected_version": 1,
                "cash": 400.0,
                "positions": [],
                "order": {"id": "t-3", "symbol": "SPY", "side": "sell"}
            }),
        )
        .await;
    assert_eq!(s, 200);
    assert_eq!(body["version"], 2);
    let (_, list) = f.call(a, f.ws, "GET", orders, &Value::Null).await;
    assert_eq!(list["orders"].as_array().unwrap().len(), 2);
}
