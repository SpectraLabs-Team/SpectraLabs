//! M24 end-to-end: instrument records, the workspace base currency, FX rates,
//! and their snapshot into a queued backtest.
//!
//! Needs `DATABASE_URL`; without it the test passes without doing anything,
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
    _store: ArtifactBackend,
    dir: PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-instr-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, _store: store, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    fn clean_up(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }

    /// Sends a signed request in `ws`; returns the status and the JSON body (Null when empty).
    async fn call_in(&self, ws: Uuid, method: &str, path: &str, body: &str) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws) = (self.user.to_string(), ws.to_string());
        let req_id = spectra_db::new_id().to_string();
        let sig =
            sign(SECRET.as_bytes(), &canonical(method, path, &ts, &user, &ws, &req_id, body.as_bytes()));
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", ws)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn call(&self, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let text = if body.is_null() { String::new() } else { body.to_string() };
        self.call_in(self.ws, method, path, &text).await
    }

    /// A dataset version in the fixture's workspace (rows only; nothing is run here).
    async fn dataset_version(&self) -> Uuid {
        spectra_db::identity::ensure_identity(&self.pool, Some(self.user), self.ws)
            .await
            .expect("ensure identity for workspace");
        let dataset: Uuid =
            sqlx::query_scalar("INSERT INTO datasets (workspace_id, name) VALUES ($1, 'mixed') RETURNING id")
                .bind(self.ws)
                .fetch_one(&self.pool)
                .await
                .unwrap();
        sqlx::query_scalar(
            "INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size) \
             VALUES ($1, $2, 1, repeat('e', 64), 10, 100) RETURNING id",
        )
        .bind(self.ws)
        .bind(dataset)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }
}

fn fields(body: &Value) -> Vec<String> {
    body["details"]["errors"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["field"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn instruments_fx_and_their_snapshot_into_a_backtest() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let version = f.dataset_version().await;

    // Base currency: USD until chosen; bad codes are refused.
    let (status, body) = f.call("GET", "/api/v1/base-currency", &Value::Null).await;
    assert_eq!((status, body["base_currency"].as_str()), (200, Some("USD")), "{body}");
    let (status, _) = f.call("PUT", "/api/v1/base-currency", &json!({"base_currency": "usd"})).await;
    assert_eq!(status, 422);

    // Records: same rules as a backtest request, every problem at once.
    let (status, body) = f
        .call("POST", "/api/v1/instruments", &json!({"symbol": "ESZ6", "type": "future", "multiplier": -1}))
        .await;
    assert_eq!(status, 422, "{body}");
    let bad = fields(&body);
    assert!(bad.contains(&"multiplier".to_string()) && bad.contains(&"tick_size".to_string()), "{body}");
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "ESZ6", "type": "future", "multiplier": 50, "tick_size": "0.25", "expiry": "2026-12-18"}),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["multiplier"], "50.00000000");
    let (status, _) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "ESZ6", "type": "future", "tick_size": 1, "expiry": "2027-01-01"}),
        )
        .await;
    assert_eq!(status, 409);
    let (status, _) = f
        .call("POST", "/api/v1/instruments", &json!({"symbol": "SAP", "currency": "EUR", "tick_size": 0.01}))
        .await;
    assert_eq!(status, 201);
    let (status, _) =
        f.call("POST", "/api/v1/instruments", &json!({"symbol": "AAPL", "tick_size": 0.01})).await;
    assert_eq!(status, 201);
    let (_, list) = f.call("GET", "/api/v1/instruments", &Value::Null).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 3, "{list}");

    // FX: exact rates, per base, replacing a rate at the same time.
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/fx-rates",
            &json!({"currency": "EUR", "rates": [{"as_of": "2026-01-02", "rate": "1.0845"}, {"as_of": "2026-01-05", "rate": 1.09}]}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["items"][0]["rate"], "1.084500000000");
    let (status, _) = f
        .call(
            "POST",
            "/api/v1/fx-rates",
            &json!({"currency": "USD", "rates": [{"as_of": "2026-01-02", "rate": 1}]}),
        )
        .await;
    assert_eq!(status, 422, "the base currency needs no rates");
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/fx-rates",
            &json!({"currency": "EUR", "rates": [{"as_of": "2026-01-02", "rate": "1.0850"}]}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["items"].as_array().unwrap().len(), 2, "replaced, not duplicated");
    assert_eq!(body["items"][0]["rate"], "1.085000000000");

    // A queued backtest carries the snapshot: the base, the records that change a run
    // (the plain AAPL equity is left out) and the EUR series.
    let (status, job) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": version, "strategy": "buy_and_hold"}),
        )
        .await;
    assert_eq!(status, 202, "{job}");
    let req = &job["request"];
    assert_eq!(req["base_currency"], "USD", "{job}");
    let symbols: Vec<&str> =
        req["instruments"].as_array().unwrap().iter().map(|i| i["symbol"].as_str().unwrap()).collect();
    assert_eq!(symbols, ["ESZ6", "SAP"], "{job}");
    assert_eq!(req["fx_rates"]["EUR"].as_array().unwrap().len(), 2, "{job}");
    // and the worker will accept it as it is stored
    let payload = spectra_core::Json::parse(&req.to_string()).unwrap();
    let parsed = spectra_backtest::BacktestJob::from_payload(&payload).expect("stored payload parses");
    assert_eq!(parsed.request.instruments.instruments.len(), 2);

    // A request that gives its own instruments is taken as given.
    let (status, job) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": version, "strategy": "buy_and_hold", "instruments": []}),
        )
        .await;
    assert_eq!(status, 202, "{job}");
    assert_eq!(job["request"]["instruments"], json!([]));

    // Delete.
    let (status, _) = f.call("DELETE", "/api/v1/instruments/SAP", &Value::Null).await;
    assert_eq!(status, 200);
    let (status, _) = f.call("DELETE", "/api/v1/instruments/SAP", &Value::Null).await;
    assert_eq!(status, 404);
    f.clean_up();
}

#[tokio::test]
async fn bond_pricing_round_trips_and_validates() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    // A 10-year 8% annual bond at a 10% yield: price 87.7109, Macaulay duration 7.0439 (textbook values).
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/pricing/bond",
            &json!({"maturity": "2036-01-01", "coupon_rate": 0.08, "coupon_frequency": 1,
                    "day_count": "act_act_icma", "as_of": "2026-01-01", "yield": 0.10}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!((body["dirty_price"].as_f64().unwrap() - 87.7109).abs() < 1e-4, "{body}");
    assert!((body["macaulay_duration"].as_f64().unwrap() - 7.0439).abs() < 1e-4, "{body}");
    assert_eq!(body["accrued_interest"].as_f64(), Some(0.0));
    // And back: the yield from that price.
    let (status, back) = f
        .call(
            "POST",
            "/api/v1/pricing/bond",
            &json!({"maturity": "2036-01-01", "coupon_rate": 0.08, "coupon_frequency": 1,
                    "day_count": "act_act_icma", "as_of": "2026-01-01", "dirty_price": body["dirty_price"]}),
        )
        .await;
    assert_eq!(status, 200, "{back}");
    assert!((back["yield"].as_f64().unwrap() - 0.10).abs() < 1e-9, "{back}");
    // Every problem at once.
    let (status, bad) = f
        .call(
            "POST",
            "/api/v1/pricing/bond",
            &json!({"maturity": "soon", "coupon_rate": 0.08, "coupon_frequency": 3,
                    "day_count": "act_act", "as_of": "2026-01-01", "yield": 0.1, "clean_price": 99}),
        )
        .await;
    assert_eq!(status, 422, "{bad}");
    let fs = fields(&bad);
    for want in ["maturity", "day_count", "coupon_frequency", ""] {
        assert!(fs.contains(&want.to_string()), "{want}: {bad}");
    }
    f.clean_up();
}

#[tokio::test]
async fn options_are_priced_and_kept_out_of_paper_trading() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    spectra_db::identity::ensure_identity(&f.pool, Some(f.user), f.ws).await.unwrap();

    // Hull: S = K = 100, one year, r = 5%, vol 20%: call 10.4506.
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/pricing/option",
            &json!({"right": "call", "spot": 100, "strike": 100, "years": 1, "rate": 0.05, "volatility": 0.2}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!((body["price"].as_f64().unwrap() - 10.450_583_572_185_565).abs() < 1e-9, "{body}");
    assert_eq!(body["model_status"], "awaiting M26 review");
    // And the implied volatility from that price.
    let (status, back) = f
        .call(
            "POST",
            "/api/v1/pricing/option",
            &json!({"right": "call", "spot": 100, "strike": 100, "years": 1, "rate": 0.05, "price": body["price"]}),
        )
        .await;
    assert_eq!(status, 200, "{back}");
    assert!((back["volatility"].as_f64().unwrap() - 0.2).abs() < 1e-9, "{back}");
    let (status, bad) = f
        .call(
            "POST",
            "/api/v1/pricing/option",
            &json!({"right": "straddle", "spot": 100, "strike": 100, "years": 1, "rate": 0.05}),
        )
        .await;
    assert_eq!(status, 422, "{bad}");
    assert!(fields(&bad).contains(&"right".to_string()), "{bad}");

    // An option record, then a paper order for it: refused until M26 signs off.
    let (status, created) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "SPY261218C00600000", "type": "option", "right": "call", "strike": 600,
                    "multiplier": 100, "tick_size": 0.01, "expiry": "2026-12-18", "underlying": "SPY"}),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["right"], "call");
    let (status, refused) = f
        .call(
            "POST",
            "/api/v1/paper/orders",
            &json!({"strategy_id": spectra_db::new_id(), "symbol": "SPY261218C00600000", "side": "buy",
                    "order_type": "market", "qty": 1}),
        )
        .await;
    assert_eq!(status, 422, "{refused}");
    assert!(refused["message"].as_str().unwrap_or_default().contains("M26"), "{refused}");
    f.clean_up();
}

#[tokio::test]
async fn curves_swaps_and_z_spreads() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let quotes = json!([
        {"type": "deposit", "years": 0.25, "rate": 0.04},
        {"type": "deposit", "years": 0.5, "rate": 0.041},
        {"type": "swap", "years": 1, "rate": 0.042, "frequency": 2},
        {"type": "swap", "years": 5, "rate": 0.045, "frequency": 2},
        {"type": "swap", "years": 10, "rate": 0.047, "frequency": 2}
    ]);
    let (status, curve) =
        f.call("POST", "/api/v1/pricing/curve", &json!({"curve": {"quotes": quotes}, "at": [3, 7.5]})).await;
    assert_eq!(status, 200, "{curve}");
    assert_eq!(curve["nodes"].as_array().unwrap().len(), 5);
    assert_eq!(curve["points"].as_array().unwrap().len(), 2);
    // The 3-month deposit reprices exactly.
    let df = curve["nodes"][0]["discount_factor"].as_f64().unwrap();
    assert!((df - 1.0 / (1.0 + 0.04 * 0.25)).abs() < 1e-14, "{curve}");

    // A 5-year swap at its par rate is worth nothing; payer and receiver mirror each other.
    let swap = |fixed: f64, pay: bool| {
        json!({"curve": {"quotes": quotes}, "notional": 10_000_000, "fixed_rate": fixed, "years": 5,
               "frequency": 2, "pay_fixed": pay})
    };
    let (status, par) = f.call("POST", "/api/v1/pricing/swap", &swap(0.045, true)).await;
    assert_eq!(status, 200, "{par}");
    assert!(par["pv"].as_f64().unwrap().abs() < 1e-4, "{par}");
    assert!((par["par_rate"].as_f64().unwrap() - 0.045).abs() < 1e-12, "{par}");
    let (_, payer) = f.call("POST", "/api/v1/pricing/swap", &swap(0.04, true)).await;
    let (_, receiver) = f.call("POST", "/api/v1/pricing/swap", &swap(0.04, false)).await;
    assert!(payer["pv"].as_f64().unwrap() > 0.0, "paying below par is worth something");
    assert!((payer["pv"].as_f64().unwrap() + receiver["pv"].as_f64().unwrap()).abs() < 1e-6);
    assert!(payer["dv01"].as_f64().unwrap() > 0.0, "a payer gains when rates rise");

    // A swap record stores its terms; one without a fixed-leg frequency, or with a day count, is refused.
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "USD5Y", "type": "swap", "tick_size": "0.0001", "maturity": "2031-11-02",
                    "coupon_rate": "-0.0025", "coupon_frequency": 2}),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!((body["type"].as_str(), body["coupon_frequency"].as_u64()), (Some("swap"), Some(2)), "{body}");
    assert_eq!(body["coupon_rate"], "-0.00250000");
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "USD7Y", "type": "swap", "tick_size": "0.0001", "maturity": "2033-11-02",
                    "coupon_rate": "0.04"}),
        )
        .await;
    assert_eq!(status, 422, "{body}");
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "USD7Y", "type": "swap", "tick_size": "0.0001", "maturity": "2033-11-02",
                    "coupon_rate": "0.04", "coupon_frequency": 2, "day_count": "act_360"}),
        )
        .await;
    assert_eq!(status, 422, "{body}");

    // A bond's z-spread over a flat curve: priced off curve + 100bp, it reads back 100bp.
    let flat = json!({"flat_rate": 0.03});
    let (status, zero) = f
        .call(
            "POST",
            "/api/v1/pricing/bond",
            &json!({"maturity": "2031-01-01", "coupon_rate": 0.05, "coupon_frequency": 1, "day_count": "act_act_icma",
                    "as_of": "2026-01-01", "yield": 0.05, "curve": flat}),
        )
        .await;
    assert_eq!(status, 200, "{zero}");
    let z = zero["z_spread"].as_f64().unwrap();
    // A 5% annual bond priced at par yields 5% annual; over a 3% continuous curve its
    // spread is near ln(1.05) - 0.03.
    assert!((z - (1.05f64.ln() - 0.03)).abs() < 1e-3, "{zero}");

    // Every problem with a curve is reported with its field.
    let (status, bad) = f
        .call(
            "POST",
            "/api/v1/pricing/curve",
            &json!({"curve": {"quotes": [{"type": "swap", "years": 1, "rate": 0.04}, {"type": "fra", "years": 2, "rate": 0.04}]}}),
        )
        .await;
    assert_eq!(status, 422, "{bad}");
    let fs = fields(&bad);
    assert!(
        fs.contains(&"curve.quotes[0].frequency".to_string())
            && fs.contains(&"curve.quotes[1].type".to_string()),
        "{bad}"
    );
    let (status, _) =
        f.call("POST", "/api/v1/pricing/curve", &json!({"curve": {"flat_rate": 0.03, "quotes": []}})).await;
    assert_eq!(status, 422, "quotes and a flat rate together are refused");
    f.clean_up();
}
