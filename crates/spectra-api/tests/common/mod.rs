//! Shared helpers for spectra-api integration tests.
#![allow(dead_code)]

use std::path::Path;

use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::{DataStore, Dataset, DatasetInfo};
use spectra_db::PgPool;
use uuid::Uuid;

/// Daily bars per symbol in a seeded dataset, above the 60-bar minimum for volatility.
pub const SEEDED_BARS: usize = 80;

/// Stores a dataset with [`SEEDED_BARS`] daily bars for each of `symbols` in `data_dir` and
/// registers it as the workspace's latest dataset version, so risk checks can estimate
/// volatility for those symbols. Closes alternate between 100 and 101, about 16% annualized
/// volatility. Creates the workspace row if it does not exist. Returns the version id.
pub async fn seed_volatility_dataset(pool: &PgPool, ws: Uuid, data_dir: &Path, symbols: &[&str]) -> Uuid {
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(ws)
        .bind(format!("ws-{}", ws.simple()))
        .bind("Test Workspace")
        .execute(pool)
        .await
        .expect("insert workspace");

    // SymbolTable::id binary-searches, so the table must be sorted; instrument ids follow it.
    let symbol_table = SymbolTable::from_symbols(symbols.iter().copied());
    let n_symbols = symbol_table.symbols.len();

    let day_ns: i64 = 86_400_000_000_000;
    let start_ns: i64 = 1_704_153_600_000_000_000; // 2024-01-02
    let mut bars = Vec::with_capacity(SEEDED_BARS * n_symbols);
    for i in 0..SEEDED_BARS {
        let ts_ns = start_ns + i64::try_from(i).expect("bar index fits i64") * day_ns;
        let close: i64 = if i % 2 == 0 { 100 } else { 101 };
        for inst in 0..n_symbols {
            bars.push(Bar {
                ts_ns,
                known_at_ns: ts_ns,
                instrument: u32::try_from(inst).expect("instrument index fits u32"),
                open: Price::from_units(close).unwrap(),
                high: Price::from_units(close + 1).unwrap(),
                low: Price::from_units(close - 1).unwrap(),
                close: Price::from_units(close).unwrap(),
                volume: Qty::from_units(10_000).unwrap(),
            });
        }
    }

    let id = compute_dataset_id(&bars, &symbol_table);
    let dataset = Dataset {
        info: DatasetInfo { id, bars_sha256: spectra_core::bars_sha256(&bars), ..DatasetInfo::default() },
        quality: spectra_data::dataset::assess_quality(
            &bars,
            &symbol_table,
            &spectra_data::CsvOptions::default(),
        ),
        bars,
        symbols: symbol_table,
    };
    let byte_size =
        i64::try_from(spectra_data::datastore::encode_dataset(&dataset).len()).expect("size fits i64");
    let checksum = DataStore::new(data_dir.to_path_buf())
        .expect("open data store")
        .put(&dataset)
        .expect("store dataset");

    let version = spectra_db::datasets::register_version(
        pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: ws,
            dataset_name: "Volatility fixture",
            checksum: &checksum,
            row_count: i64::try_from(dataset.bars.len()).expect("row count fits i64"),
            byte_size,
            quality_report: serde_json::json!({}),
            created_by: None,
            blob_sha256: None,
            source: None,
        },
    )
    .await
    .expect("register dataset version");
    version.id
}

/// Stores a dataset with [`SEEDED_BARS`] daily bars for each of `symbols_with_closes` in `data_dir`
/// and registers it as the workspace's latest dataset version. The final bar for each symbol has
/// `final_close`. Creates the workspace row if it does not exist. Returns the version id.
pub async fn seed_dataset_with_latest_closes(
    pool: &PgPool,
    ws: Uuid,
    data_dir: &Path,
    symbols_with_closes: &[(&str, f64)],
) -> Uuid {
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(ws)
        .bind(format!("ws-{}", ws.simple()))
        .bind("Test Workspace")
        .execute(pool)
        .await
        .expect("insert workspace");

    let mut symbols: Vec<&str> = symbols_with_closes.iter().map(|&(s, _)| s).collect();
    symbols.sort_unstable();
    symbols.dedup();
    let symbol_table = SymbolTable::from_symbols(symbols.iter().copied());
    let n_symbols = symbol_table.symbols.len();

    let day_ns: i64 = 86_400_000_000_000;
    let start_ns: i64 = 1_704_153_600_000_000_000; // 2024-01-02
    let mut bars = Vec::with_capacity(SEEDED_BARS * n_symbols);
    for i in 0..SEEDED_BARS {
        let ts_ns = start_ns + i64::try_from(i).expect("bar index fits i64") * day_ns;
        for &(sym, final_close) in symbols_with_closes {
            let inst = symbol_table.id(sym).unwrap();
            let c = if i == SEEDED_BARS - 1 {
                final_close
            } else if i % 2 == 0 {
                final_close * 0.99
            } else {
                final_close * 1.01
            };
            bars.push(Bar {
                ts_ns,
                known_at_ns: ts_ns,
                instrument: inst,
                open: Price::from_f64(c).unwrap(),
                high: Price::from_f64(c * 1.01).unwrap(),
                low: Price::from_f64(c * 0.99).unwrap(),
                close: Price::from_f64(c).unwrap(),
                volume: Qty::from_units(10_000).unwrap(),
            });
        }
    }
    bars.sort_by_key(|b| (b.ts_ns, b.instrument));

    let id = compute_dataset_id(&bars, &symbol_table);
    let dataset = Dataset {
        info: DatasetInfo { id, bars_sha256: spectra_core::bars_sha256(&bars), ..DatasetInfo::default() },
        quality: spectra_data::dataset::assess_quality(
            &bars,
            &symbol_table,
            &spectra_data::CsvOptions::default(),
        ),
        bars,
        symbols: symbol_table,
    };
    let byte_size =
        i64::try_from(spectra_data::datastore::encode_dataset(&dataset).len()).expect("size fits i64");
    let checksum = DataStore::new(data_dir.to_path_buf())
        .expect("open data store")
        .put(&dataset)
        .expect("store dataset");

    let version = spectra_db::datasets::register_version(
        pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: ws,
            dataset_name: "Pricing fixture",
            checksum: &checksum,
            row_count: i64::try_from(dataset.bars.len()).expect("row count fits i64"),
            byte_size,
            quality_report: serde_json::json!({}),
            created_by: None,
            blob_sha256: None,
            source: None,
        },
    )
    .await
    .expect("register dataset version");
    version.id
}

/// The text a PDF draws, with string escapes undone: every `(...) Tj` operand. A copy of the
/// helper in `tear_sheet_pdf.rs`'s unit tests, which integration tests cannot reach.
pub fn drawn_text(pdf: &[u8]) -> String {
    let s = String::from_utf8_lossy(pdf).into_owned();
    let mut out = String::new();
    for line in s.lines() {
        if let (Some(start), Some(end)) = (line.find('('), line.rfind(") Tj")) {
            if start < end {
                out.push_str(&line[start + 1..end]);
                out.push('\n');
            }
        }
    }
    out.replace("\\(", "(").replace("\\)", ")").replace("\\\\", "\\")
}
