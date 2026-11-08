//! Risk parity portfolios (M15 follow-on) through the signed API: the weights come from the
//! closes of a dataset version up to `as_of`, are stored with the `risk_parity` method, and match
//! `spectra_backtest::portfolio::risk_parity_weights_from_closes` on the same closes. Viewers get
//! 403. Needs `DATABASE_URL`; without it the tests pass without doing anything.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_backtest::portfolio::risk_parity_weights_from_closes;
use spectra_backtest::rebalance::Date;
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::{DataStore, Dataset, DatasetInfo};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";
const DAY_NS: i64 = 86_400_000_000_000;
/// Days with a bar for AAA and BBB, from 2026-01-02.
const DAYS: usize = 120;
static COUNTER: AtomicU64 = AtomicU64::new(0);

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn fresh() -> String {
    format!(
        "{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn first_day() -> Date {
    Date::new(2026, 1, 2)
}

/// The closes of AAA and BBB, as the dataset stores them. AAA's daily log returns alternate +a,
/// -a; BBB's run +2a, +2a, -2a, -2a. Uncorrelated, BBB twice as volatile: risk parity is close to
/// (2/3, 1/3) (exactly, but for the rounding of stored prices).
fn closes() -> [Vec<f64>; 2] {
    let a = 0.01;
    let (mut pa, mut pb) = (100.0_f64, 100.0_f64);
    let (mut aaa, mut bbb) = (vec![], vec![]);
    for k in 0..DAYS {
        if k > 0 {
            pa *= f64::exp(if k % 2 == 1 { a } else { -a });
            pb *= f64::exp(if (k - 1) % 4 < 2 { 2.0 * a } else { -2.0 * a });
        }
        aaa.push(Price::from_f64(pa).unwrap().to_f64());
        bbb.push(Price::from_f64(pb).unwrap().to_f64());
    }
    [aaa, bbb]
}

struct Reply {
    status: u16,
    json: Value,
}

/// A workspace with an owner and a viewer, another workspace, and a dataset version with AAA and
/// BBB every day for [`DAYS`] days from 2026-01-02 (see [`closes`]) and CCC with no bars.
struct Fixture {
    app: Router,
    pool: PgPool,
    dir: PathBuf,
    ws: Uuid,
    owner: Uuid,
    viewer: Uuid,
    other_ws: Uuid,
    other_owner: Uuid,
    version: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-risk-parity-{}", spectra_db::new_id().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let (ws, other_ws) = (spectra_db::new_id(), spectra_db::new_id());
        for w in [ws, other_ws] {
            sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Risk parity test')")
                .bind(w)
                .bind(format!("ws-{}", w.simple()))
                .execute(&pool)
                .await
                .unwrap();
        }
        let (owner, viewer, other_owner) = (spectra_db::new_id(), spectra_db::new_id(), spectra_db::new_id());
        for (user, w, role) in
            [(owner, ws, "owner"), (viewer, ws, "viewer"), (other_owner, other_ws, "owner")]
        {
            sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
                .bind(user)
                .bind(format!("user-{}@example.com", user.simple()))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)")
                .bind(w)
                .bind(user)
                .bind(role)
                .execute(&pool)
                .await
                .unwrap();
        }

        let symbols = SymbolTable::from_symbols(["AAA", "BBB", "CCC"]);
        let (aaa, bbb) = (symbols.id("AAA").unwrap(), symbols.id("BBB").unwrap());
        let [ca, cb] = closes();
        let mut bars = Vec::new();
        for k in 0..DAYS {
            let ts = (first_day().to_days() + k as i64) * DAY_NS + 21 * 3_600_000_000_000;
            for (id, close) in [(aaa, ca[k]), (bbb, cb[k])] {
                let p = Price::from_f64(close).unwrap();
                bars.push(Bar {
                    ts_ns: ts,
                    known_at_ns: ts,
                    instrument: id,
                    open: p,
                    high: p,
                    low: p,
                    close: p,
                    volume: Qty::from_units(10_000).unwrap(),
                });
            }
        }
        bars.sort_by_key(|b| (b.ts_ns, b.instrument));
        let id = compute_dataset_id(&bars, &symbols);
        let dataset = Dataset {
            info: DatasetInfo { id, bars_sha256: spectra_core::bars_sha256(&bars), ..DatasetInfo::default() },
            quality: spectra_data::dataset::assess_quality(
                &bars,
                &symbols,
                &spectra_data::CsvOptions::default(),
            ),
            bars,
            symbols,
        };
        let byte_size = i64::try_from(spectra_data::datastore::encode_dataset(&dataset).len()).unwrap();
        let checksum = DataStore::new(dir.clone()).unwrap().put(&dataset).unwrap();
        let reg = spectra_db::datasets::register_version(
            &pool,
            &spectra_db::datasets::NewDatasetVersion {
                workspace_id: ws,
                dataset_name: "Risk parity dataset",
                checksum: &checksum,
                row_count: i64::try_from(dataset.bars.len()).unwrap(),
                byte_size,
                quality_report: json!({}),
                created_by: None,
                blob_sha256: None,
                source: None,
            },
        )
        .await
        .unwrap();

        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.data_dir = Some(dir.clone());
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, dir, ws, owner, viewer, other_ws, other_owner, version: reg.id }
    }

    async fn call_as(&self, method: &str, path: &str, user: Uuid, ws: Uuid, body: &Value) -> Reply {
        let payload = if body.is_null() { Vec::new() } else { serde_json::to_vec(body).unwrap() };
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (u, w) = (user.to_string(), ws.to_string());
        let rid = format!("req-risk-parity-{}", fresh());
        let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u, &w, &rid, &payload));
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-spectra-user", u)
            .header("x-spectra-workspace", w)
            .header("x-request-id", rid)
            .header("x-spectra-timestamp", ts)
            .header("x-spectra-signature", sig)
            .body(Body::from(payload))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply { status, json: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
    }

    async fn owner(&self, method: &str, path: &str, body: &Value) -> Reply {
        self.call_as(method, path, self.owner, self.ws, body).await
    }

    async fn post(&self, body: Value) -> Reply {
        self.owner("POST", "/api/v1/portfolios", &body).await
    }

    fn body(&self, symbols: &[&str], as_of: Option<&str>) -> Value {
        let mut b = json!({
            "name": format!("rp-{}", fresh()),
            "method": "risk_parity",
            "holdings": symbols.iter().map(|s| json!({"symbol": s})).collect::<Vec<_>>(),
            "dataset_version_id": self.version,
        });
        if let Some(d) = as_of {
            b["as_of"] = json!(d);
        }
        b
    }

    async fn clean_up(&self) {
        for ws in [self.ws, self.other_ws] {
            for table in ["portfolios", "dataset_versions", "datasets"] {
                let _ = sqlx::query(&format!("DELETE FROM {table} WHERE workspace_id = $1"))
                    .bind(ws)
                    .execute(&self.pool)
                    .await;
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stored_weights(view: &Value) -> Vec<(String, f64)> {
    view["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h["symbol"].as_str().unwrap().to_string(), h["weight"].as_f64().unwrap()))
        .collect()
}

fn expected(n: usize) -> Vec<f64> {
    let [a, b] = closes();
    let symbols = vec!["AAA".to_string(), "BBB".to_string()];
    let w = risk_parity_weights_from_closes(&symbols, &[a[..n].to_vec(), b[..n].to_vec()], 252.0).unwrap();
    w.into_iter().map(|h| h.weight).collect()
}

#[tokio::test]
async fn a_risk_parity_portfolio_is_built_from_the_dataset_and_stored() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let r = f.owner("POST", "/api/v1/portfolios", &f.body(&["AAA", "BBB"], None)).await;
    assert_eq!(r.status, 201, "{}", r.json);
    assert_eq!(r.json["method"], "risk_parity");
    let id = r.json["id"].as_str().unwrap().to_string();
    let weights = stored_weights(&r.json);
    let want = expected(DAYS);
    assert_eq!(weights.iter().map(|w| w.0.as_str()).collect::<Vec<_>>(), ["AAA", "BBB"]);
    for ((_, got), w) in weights.iter().zip(&want) {
        assert!((got - w).abs() < 1e-12, "{weights:?} vs {want:?}");
    }
    // Uncorrelated, BBB twice as volatile: about 2/3 and 1/3.
    assert!(
        (weights[0].1 - 2.0 / 3.0).abs() < 1e-3 && (weights[1].1 - 1.0 / 3.0).abs() < 1e-3,
        "{weights:?}"
    );

    // Reloads with the same weights and method.
    let r = f.owner("GET", &format!("/api/v1/portfolios/{id}"), &Value::Null).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json["method"], "risk_parity");
    assert_eq!(stored_weights(&r.json), weights);

    // as_of limits the closes to those known that day: 70 days of history.
    let as_of = Date::from_days(first_day().to_days() + 69).unwrap().to_string();
    let r = f.owner("PUT", &format!("/api/v1/portfolios/{id}"), &f.body(&["AAA", "BBB"], Some(&as_of))).await;
    assert_eq!(r.status, 200, "{}", r.json);
    let weights = stored_weights(&r.json);
    let want = expected(70);
    for ((_, got), w) in weights.iter().zip(&want) {
        assert!((got - w).abs() < 1e-12, "{weights:?} vs {want:?}");
    }
    assert_ne!(expected(70), expected(DAYS), "the cut-off changes the input");
    f.clean_up().await;
}

#[tokio::test]
async fn bad_risk_parity_requests_are_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // A holding with no bars.
    let r = f.post(f.body(&["AAA", "CCC"], None)).await;
    assert_eq!(r.status, 422, "{}", r.json);
    assert!(r.json["message"].as_str().unwrap().contains("CCC"), "{}", r.json);

    // Fewer than 60 closes up to as_of.
    let as_of = Date::from_days(first_day().to_days() + 30).unwrap().to_string();
    let r = f.post(f.body(&["AAA", "BBB"], Some(&as_of))).await;
    assert_eq!(r.status, 422);
    assert!(r.json["message"].as_str().unwrap().contains("need 60 prices"), "{}", r.json);

    // No look-ahead: as_of after the day of the save.
    let r = f.post(f.body(&["AAA", "BBB"], Some("2999-01-01"))).await;
    assert_eq!(r.status, 422);
    assert!(r.json["message"].as_str().unwrap().contains("after the day of the save"), "{}", r.json);

    // A malformed as_of, no dataset version, and another workspace's version.
    assert_eq!(f.post(f.body(&["AAA", "BBB"], Some("soon"))).await.status, 422);
    let mut no_version = f.body(&["AAA", "BBB"], None);
    no_version.as_object_mut().unwrap().remove("dataset_version_id");
    let r = f.post(no_version).await;
    assert_eq!(r.status, 422);
    assert!(r.json["message"].as_str().unwrap().contains("dataset_version_id"), "{}", r.json);
    let r = f
        .call_as("POST", "/api/v1/portfolios", f.other_owner, f.other_ws, &f.body(&["AAA", "BBB"], None))
        .await;
    assert_eq!(r.status, 404, "{}", r.json);

    // Nothing was stored.
    let r = f.owner("GET", "/api/v1/portfolios", &Value::Null).await;
    assert_eq!(r.json.as_array().unwrap().len(), 0, "{}", r.json);
    f.clean_up().await;
}

#[tokio::test]
async fn viewers_cannot_save_risk_parity_portfolios() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;
    let r = f.call_as("POST", "/api/v1/portfolios", f.viewer, f.ws, &f.body(&["AAA", "BBB"], None)).await;
    assert_eq!(r.status, 403, "{}", r.json);
    assert_eq!(r.json["code"], "viewer_read_only");
    let r = f.owner("GET", "/api/v1/portfolios", &Value::Null).await;
    assert_eq!(r.json.as_array().unwrap().len(), 0);
    f.clean_up().await;
}
