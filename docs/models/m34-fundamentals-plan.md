# M34: Fundamentals Data & Point-in-Time Store Specification

**Status**: Implemented & Verified (M34 Tasks 1-7)  
**Implementation**: `crates/spectra-data/src/fundamentals.rs`  
**Tests**: `crates/spectra-data/tests/m34_fundamentals_reference.rs`

## 1. Provider Landscape & Analysis (Task 1 & Task 2 / Decision DL-1)
- **Current M1 Market Data Providers**: M1 Data Studio currently supports CSV ingest, Parquet storage, and market-data adapters (e.g. historical EOD prices).
- **Fundamentals Vendor Assessment**:
  - *Free / Staging*: SEC EDGAR Company Facts API & Financial Statement Data Sets (FSDS). Provides point-in-time filing accession dates (`filed` / `accepted` timestamps) free of charge with zero licensing fees.
  - *Commercial*: Financial Modeling Prep (FMP) / Polygon / Refinitiv. Provides comprehensive fundamental statement feeds with restatement timelines.
- **Decision DL-1 Recommendation**: Use EDGAR FSDS format as the open baseline adapter and interface contract (`FundamentalsProvider` trait). The test adapter (`MockFundamentalsProvider`) supplies standard restatement fixtures for AAPL and MSFT.

## 2. Point-in-Time Mechanics & Look-Ahead Prevention (Task 4)
- **Key Fields**: `(symbol, period, metric, publication_ts_ns, restatement_index)`
- **Behavior**:
  - Restatements create new rows with updated `publication_ts_ns` and incremented `restatement_index`.
  - When a query asks for `metric` as of `as_of_ts_ns`, the query returns the latest revision where `publication_ts_ns <= as_of_ts_ns`.
  - Backtests running at a past historical date $T$ never see restatements published at $T + \Delta$, completely eliminating look-ahead bias.

## 3. Symbol Master Lifecycle & Delisting (Task 5)
- Tracks sector, industry, exchange, country, and status (`Active`, `Delisted`, `Suspended`) with `effective_from_ns` and `effective_to_ns`.
- Delisted symbols remain in historical queries to prevent survivorship bias, but are automatically filtered out from active trade universes.

## 4. Quality Checks & Quarantine (Task 6)
- **Checks**:
  - Impossible negatives (e.g. negative revenue, total assets, or shares outstanding) are rejected.
  - Non-finite values (NaN / Inf) are rejected.
  - Bad rows are diverted to the quarantined records collection (`quarantined_records()`), retaining auditability in the dataset browser.

## 5. Dataset Versioning (Task 7)
- Computes deterministic, content-addressed SHA-256 version identifiers (`dataset_version_id()`, prefixed `fund_v_...`). Screener and backtest runs can cite this version ID for 100% reproducible execution.
