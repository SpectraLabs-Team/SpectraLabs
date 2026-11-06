//! Dataset prices lookup for paper trading.
//!
//! Provides `latest_closes` (pure) to extract the latest close price for requested symbols
//! from a loaded dataset, and `latest_prices` (async) to resolve the active dataset version
//! for a workspace, load it, and return its latest closes.

use std::collections::HashMap;
use std::path::Path;

use uuid::Uuid;

/// Extract the latest close price per symbol from a loaded dataset.
///
/// # Errors
/// Returns an error if any requested symbol has no bars in the dataset.
#[allow(dead_code)]
pub fn latest_closes(
    dataset: &spectra_data::Dataset,
    symbols: &[&str],
) -> Result<HashMap<String, f64>, String> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result = HashMap::new();
    for &sym in symbols {
        let inst_id = dataset.symbols.id(sym).or_else(|| dataset.symbols.id(&sym.to_uppercase()));
        let latest_bar =
            inst_id.and_then(|id| dataset.bars.iter().filter(|b| b.instrument == id).max_by_key(|b| b.ts_ns));

        match latest_bar {
            Some(bar) => {
                result.insert(sym.to_string(), bar.close.to_f64());
            }
            None => return Err(format!("no bars for {sym}")),
        }
    }

    Ok(result)
}

/// Look up the latest close prices for `symbols` in workspace `ws`.
///
/// Resolves `dataset_version_id` (or the latest version registered in `ws`), loads the dataset
/// from `data_dir`, and returns the latest close per symbol.
///
/// # Errors
/// Returns an error if the dataset version cannot be resolved, is sealed in holdout, fails to
/// load, or if any requested symbol has no bars in the dataset.
#[allow(dead_code)]
pub async fn latest_prices(
    pool: &spectra_db::PgPool,
    ws: Uuid,
    data_dir: &Path,
    dataset_version_id: Option<Uuid>,
    symbols: &[&str],
) -> Result<HashMap<String, f64>, String> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }

    let target_version_id = if let Some(id) = dataset_version_id {
        Some(id)
    } else {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM dataset_versions WHERE workspace_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(ws)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("failed to query latest dataset version: {e}"))?;

        row.map(|(id,)| id)
    };

    let Some(target_version_id) = target_version_id else {
        return Err("no dataset version to price fills from".to_string());
    };

    let resolved = spectra_db::datasets::resolve_version(
        pool,
        ws,
        target_version_id,
        spectra_db::datasets::Access::engine("risk_volatility_lookup"),
    )
    .await
    .map_err(|e| format!("failed to resolve dataset version: {e}"))?;

    let version = match resolved {
        spectra_db::datasets::Resolved::Found(v) => v,
        spectra_db::datasets::Resolved::Sealed { holdout } => {
            return Err(format!("dataset version {target_version_id} is sealed in holdout vault: {holdout}"));
        }
        spectra_db::datasets::Resolved::Missing => {
            return Err(format!("dataset version {target_version_id} not found in workspace"));
        }
    };

    let dataset = spectra_backtest::load_dataset(data_dir, &version.checksum)
        .map_err(|e| format!("failed to load dataset {}: {e}", version.checksum))?;

    latest_closes(&dataset, symbols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::Price;
    use spectra_core::{Bar, Qty};
    use spectra_data::dataset::SymbolTable;
    use spectra_data::Dataset;

    fn make_test_dataset(symbols: &[&str], bars_data: &[(u32, i64, f64)]) -> Dataset {
        let symbols = SymbolTable::from_symbols(symbols.iter().copied());
        let bars = bars_data
            .iter()
            .map(|&(inst, ts, close)| Bar {
                ts_ns: ts,
                known_at_ns: ts,
                instrument: inst,
                open: Price::from_f64(close).unwrap(),
                high: Price::from_f64(close).unwrap(),
                low: Price::from_f64(close).unwrap(),
                close: Price::from_f64(close).unwrap(),
                volume: Qty::from_units(100).unwrap(),
            })
            .collect();
        Dataset { symbols, bars, ..Dataset::default() }
    }

    #[test]
    fn empty_symbols_returns_empty_map() {
        let ds = make_test_dataset(&["AAPL"], &[(0, 1_000, 150.0)]);
        let res = latest_closes(&ds, &[]).unwrap();
        assert!(res.is_empty());
    }

    #[test]
    fn single_symbol_returns_latest_close() {
        let ds = make_test_dataset(&["AAPL"], &[(0, 1_000, 150.0), (0, 2_000, 155.0), (0, 1_500, 152.0)]);
        let res = latest_closes(&ds, &["AAPL"]).unwrap();
        assert_eq!(res.get("AAPL"), Some(&155.0));
    }

    #[test]
    fn multiple_symbols_return_respective_latest_closes() {
        let ds = make_test_dataset(
            &["AAPL", "MSFT"],
            &[(0, 1_000, 150.0), (0, 2_000, 155.0), (1, 1_000, 300.0), (1, 3_000, 310.0)],
        );
        let res = latest_closes(&ds, &["AAPL", "MSFT"]).unwrap();
        assert_eq!(res.get("AAPL"), Some(&155.0));
        assert_eq!(res.get("MSFT"), Some(&310.0));
    }

    #[test]
    fn symbol_with_no_bars_is_rejected() {
        let ds = make_test_dataset(&["AAPL", "GOOG"], &[(0, 1_000, 150.0)]);
        let err = latest_closes(&ds, &["GOOG"]).unwrap_err();
        assert_eq!(err, "no bars for GOOG");
    }

    #[test]
    fn case_insensitive_symbol_lookup() {
        let ds = make_test_dataset(&["AAPL"], &[(0, 1_000, 150.0)]);
        let res = latest_closes(&ds, &["aapl"]).unwrap();
        assert_eq!(res.get("aapl"), Some(&150.0));
    }
}
