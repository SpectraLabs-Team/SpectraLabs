//! M11 Task 0: who can read P&L attribution.
//!
//! Decision 4 (revised 6 Nov 2026): attribution is workspace-scoped, like the backtest and paper
//! pages it belongs to. Any member of the signed workspace can read it; another workspace's
//! backtest or paper strategy is a 404; no workspace is a 400; no identity is a 401. Reading
//! paper attribution never creates a strategy or an account.
//!
//! The database tests skip when `DATABASE_URL` is unset, like the other API flows.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
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

/// A signed GET as `user` in `ws` (`None` = no active workspace).
fn signed_get(path: &str, user: Uuid, ws: Option<Uuid>) -> Request<Body> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
    let u = user.to_string();
    let w = ws.map(|w| w.to_string()).unwrap_or_default();
    let rid = "req-m11-access";
    let sig = sign(SECRET.as_bytes(), &canonical("GET", path, &ts, &u, &w, rid, b""));
    Request::builder()
        .method("GET")
        .uri(path)
        .header("x-spectra-user", u)
        .header("x-spectra-workspace", w)
        .header("x-request-id", rid)
        .header("x-spectra-timestamp", ts)
        .header("x-spectra-signature", sig)
        .body(Body::empty())
        .unwrap()
}

async fn send(app: &Router, req: Request<Body>) -> (u16, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn backtest_path(job: Uuid) -> String {
    format!("/api/v1/backtests/{job}/attribution")
}

fn paper_path(strategy: Uuid) -> String {
    format!("/api/v1/paper/attribution?strategy_id={strategy}")
}

// ------------------------------------------------------------------ no database needed

#[tokio::test]
async fn attribution_routes_require_an_identity() {
    let app = router(ApiConfig::new(Secret::new(SECRET)), None);
    for path in [backtest_path(Uuid::nil()), paper_path(Uuid::nil())] {
        let req = Request::builder().uri(&path).body(Body::empty()).unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, 401, "{path} without a signed identity");
    }
}

// ------------------------------------------------------------------ fixture

/// Two workspaces: `ws` with two members (`owner`, `member`), and `other_ws` with `outsider`.
/// `ws` has a paper strategy and a succeeded backtest with an attribution; `other_ws` has a
/// paper strategy and a succeeded backtest of its own.
struct Fixture {
    app: Router,
    pool: PgPool,
    owner: Uuid,
    member: Uuid,
    outsider: Uuid,
    ws: Uuid,
    other_ws: Uuid,
    strategy: Uuid,
    other_strategy: Uuid,
    job: Uuid,
    other_job: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let (owner, member, outsider) = (spectra_db::new_id(), spectra_db::new_id(), spectra_db::new_id());
        let (ws, other_ws) = (spectra_db::new_id(), spectra_db::new_id());
        let (strategy, other_strategy) = (spectra_db::new_id(), spectra_db::new_id());
        let (job, other_job) = (spectra_db::new_id(), spectra_db::new_id());

        for u in [owner, member, outsider] {
            sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
                .bind(u)
                .bind(format!("m11-{}@example.com", u.simple()))
                .execute(&pool)
                .await
                .unwrap();
        }
        for w in [ws, other_ws] {
            sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'M11 access test')")
                .bind(w)
                .bind(format!("ws-{}", w.simple()))
                .execute(&pool)
                .await
                .unwrap();
        }
        for (s, w, by) in [(strategy, ws, owner), (other_strategy, other_ws, outsider)] {
            sqlx::query(
                "INSERT INTO strategies (id, workspace_id, name, description, created_by) \
                 VALUES ($1, $2, $3, 'M11 access test', $4)",
            )
            .bind(s)
            .bind(w)
            .bind(format!("m11-{}", s.simple()))
            .bind(by)
            .execute(&pool)
            .await
            .unwrap();
        }
        for (j, w, by) in [(job, ws, owner), (other_job, other_ws, outsider)] {
            let result = json!({ "summary": { "attribution": {
                "total_net_pnl": 12.5, "total_gross_pnl": 14.0, "total_fees": 1.5, "total_trades": 1
            } } });
            sqlx::query(
                "INSERT INTO jobs (id, workspace_id, kind, status, finished_at, result, created_by) \
                 VALUES ($1, $2, 'backtest', 'succeeded', now(), $3, $4)",
            )
            .bind(j)
            .bind(w)
            .bind(result)
            .bind(by)
            .execute(&pool)
            .await
            .unwrap();
        }

        let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));
        Self { app, pool, owner, member, outsider, ws, other_ws, strategy, other_strategy, job, other_job }
    }

    async fn strategy_exists(&self, id: Uuid) -> bool {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM strategies WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn balance_rows(&self, ws: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM portfolio_balances WHERE workspace_id = $1")
            .bind(ws)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn clean_up(&self) {
        for ws in [self.ws, self.other_ws] {
            for table in ["paper_fills", "paper_orders", "portfolio_positions", "portfolio_balances", "jobs"]
            {
                let _ = sqlx::query(&format!("DELETE FROM {table} WHERE workspace_id = $1"))
                    .bind(ws)
                    .execute(&self.pool)
                    .await;
            }
            let _ = sqlx::query("DELETE FROM strategies WHERE workspace_id = $1")
                .bind(ws)
                .execute(&self.pool)
                .await;
            let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(ws).execute(&self.pool).await;
        }
        for u in [self.owner, self.member, self.outsider] {
            let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(u).execute(&self.pool).await;
        }
    }
}

// ------------------------------------------------------------------ backtest attribution

#[tokio::test]
async fn backtest_attribution_is_readable_by_any_member_of_its_workspace() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    for user in [f.owner, f.member] {
        let (status, body) = send(&f.app, signed_get(&backtest_path(f.job), user, Some(f.ws))).await;
        assert_eq!(status, 200, "member {user}: {body}");
        assert_eq!(body["total_net_pnl"], 12.5, "{body}");
    }

    f.clean_up().await;
}

#[tokio::test]
async fn backtest_attribution_of_another_workspace_is_not_found() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // A member of `ws` asking for `other_ws`'s backtest, from `ws`.
    let (status, body) = send(&f.app, signed_get(&backtest_path(f.other_job), f.owner, Some(f.ws))).await;
    assert_eq!(status, 404, "{body}");
    assert!(body.get("total_net_pnl").is_none(), "no numbers leak in the error: {body}");

    // The outsider asking for `ws`'s backtest from their own workspace.
    let (status, _) = send(&f.app, signed_get(&backtest_path(f.job), f.outsider, Some(f.other_ws))).await;
    assert_eq!(status, 404);

    f.clean_up().await;
}

#[tokio::test]
async fn backtest_attribution_without_a_workspace_is_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let (status, body) = send(&f.app, signed_get(&backtest_path(f.job), f.owner, None)).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["code"], "no_workspace", "{body}");

    f.clean_up().await;
}

// ------------------------------------------------------------------ paper attribution

#[tokio::test]
async fn paper_attribution_is_readable_by_any_member_of_its_workspace() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    for user in [f.owner, f.member] {
        let (status, body) = send(&f.app, signed_get(&paper_path(f.strategy), user, Some(f.ws))).await;
        assert_eq!(status, 200, "member {user}: {body}");
        // No account and no fills yet: an empty breakdown.
        assert_eq!(body["total_trades"], 0, "{body}");
    }
    // Reading did not open an account.
    assert_eq!(f.balance_rows(f.ws).await, 0, "a read must not create a paper account");

    f.clean_up().await;
}

#[tokio::test]
async fn paper_attribution_of_another_workspace_is_not_found() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let (status, body) = send(&f.app, signed_get(&paper_path(f.other_strategy), f.owner, Some(f.ws))).await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(f.balance_rows(f.ws).await, 0, "no account opened in the caller's workspace");
    assert_eq!(f.balance_rows(f.other_ws).await, 0, "no account opened in the other workspace");

    f.clean_up().await;
}

#[tokio::test]
async fn paper_attribution_for_an_unknown_strategy_creates_nothing() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let unknown = spectra_db::new_id();
    let (status, body) = send(&f.app, signed_get(&paper_path(unknown), f.owner, Some(f.ws))).await;
    assert_eq!(status, 404, "{body}");
    assert!(!f.strategy_exists(unknown).await, "a read must not create a strategy");
    assert_eq!(f.balance_rows(f.ws).await, 0, "a read must not create a paper account");

    f.clean_up().await;
}

#[tokio::test]
async fn paper_attribution_without_a_workspace_is_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let (status, body) = send(&f.app, signed_get(&paper_path(f.strategy), f.owner, None)).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["code"], "no_workspace", "{body}");

    f.clean_up().await;
}
