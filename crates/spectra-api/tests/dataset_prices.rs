//! Integration test for `latest_prices` against Postgres.

use std::path::PathBuf;

use spectra_api::dataset_prices::latest_prices;
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::{DataStore, Dataset, DatasetInfo};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

struct Fixture {
    pool: PgPool,
    dir: PathBuf,
    ws: Uuid,
    dataset_version_id: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-prices-{}", spectra_db::new_id().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = spectra_db::new_id();

        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Prices test workspace')")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .execute(&pool)
            .await
            .unwrap();

        // Symbol table with AAPL, GOOG, and MSFT (sorted).
        let symbols = SymbolTable::from_symbols(["AAPL", "GOOG", "MSFT"]);
        let inst_aapl = symbols.id("AAPL").unwrap();
        let inst_msft = symbols.id("MSFT").unwrap();

        // Known closes: AAPL has two bars (latest close 155.0), MSFT has one bar (close 300.0),
        // GOOG has no bars.
        let mut bars = vec![
            Bar {
                ts_ns: 1_000,
                known_at_ns: 1_000,
                instrument: inst_aapl,
                open: Price::from_f64(149.0).unwrap(),
                high: Price::from_f64(151.0).unwrap(),
                low: Price::from_f64(148.0).unwrap(),
                close: Price::from_f64(150.0).unwrap(),
                volume: Qty::from_units(100).unwrap(),
            },
            Bar {
                ts_ns: 1_000,
                known_at_ns: 1_000,
                instrument: inst_msft,
                open: Price::from_f64(295.0).unwrap(),
                high: Price::from_f64(305.0).unwrap(),
                low: Price::from_f64(290.0).unwrap(),
                close: Price::from_f64(300.0).unwrap(),
                volume: Qty::from_units(100).unwrap(),
            },
            Bar {
                ts_ns: 2_000,
                known_at_ns: 2_000,
                instrument: inst_aapl,
                open: Price::from_f64(151.0).unwrap(),
                high: Price::from_f64(156.0).unwrap(),
                low: Price::from_f64(150.0).unwrap(),
                close: Price::from_f64(155.0).unwrap(),
                volume: Qty::from_units(100).unwrap(),
            },
        ];
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
                dataset_name: "Test dataset",
                checksum: &checksum,
                row_count: i64::try_from(dataset.bars.len()).unwrap(),
                byte_size,
                quality_report: serde_json::json!({}),
                created_by: None,
                blob_sha256: None,
                source: None,
            },
        )
        .await
        .unwrap();

        Self { pool, dir, ws, dataset_version_id: reg.id }
    }

    async fn clean_up(&self) {
        let _ = sqlx::query("DELETE FROM holdouts WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM holdout_access_log WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM dataset_versions WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM datasets WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn latest_close_per_symbol_is_returned() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // Latest version resolved automatically
    let prices = latest_prices(&f.pool, f.ws, &f.dir, None, &["AAPL", "MSFT"]).await.unwrap();
    assert_eq!(prices.get("AAPL"), Some(&155.0));
    assert_eq!(prices.get("MSFT"), Some(&300.0));

    // Explicit dataset version specified
    let prices_explicit =
        latest_prices(&f.pool, f.ws, &f.dir, Some(f.dataset_version_id), &["AAPL", "MSFT"]).await.unwrap();
    assert_eq!(prices_explicit.get("AAPL"), Some(&155.0));
    assert_eq!(prices_explicit.get("MSFT"), Some(&300.0));

    f.clean_up().await;
}

#[tokio::test]
async fn symbol_with_no_bars_is_rejected() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // GOOG is in the symbol table but has no bars in the dataset
    let err = latest_prices(&f.pool, f.ws, &f.dir, None, &["GOOG"]).await.unwrap_err();
    assert_eq!(err, "no bars for GOOG");

    // UNKNOWN is not in the symbol table at all
    let err_unknown = latest_prices(&f.pool, f.ws, &f.dir, None, &["UNKNOWN"]).await.unwrap_err();
    assert_eq!(err_unknown, "no bars for UNKNOWN");

    f.clean_up().await;
}

#[tokio::test]
async fn sealed_holdout_version_is_rejected() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let holdout_id = spectra_db::new_id();
    spectra_db::optimization::insert_holdout(
        &f.pool,
        holdout_id,
        f.ws,
        "Vault 1",
        f.dataset_version_id,
        5,
        "notes",
        None,
    )
    .await
    .unwrap();

    let err = latest_prices(&f.pool, f.ws, &f.dir, Some(f.dataset_version_id), &["AAPL"]).await.unwrap_err();
    assert!(err.contains("is sealed in holdout vault: Vault 1"), "expected sealed holdout error, got: {err}");

    f.clean_up().await;
}

#[tokio::test]
async fn workspace_with_no_dataset_version_returns_expected_error() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let empty_ws = spectra_db::new_id();
    let err = latest_prices(&f.pool, empty_ws, &f.dir, None, &["AAPL"]).await.unwrap_err();
    assert_eq!(err, "no dataset version to price fills from");

    f.clean_up().await;
}
