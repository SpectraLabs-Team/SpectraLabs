# M34: Fundamentals Data & Point-in-Time Store Specification

**Status**: Done (2026-11-09). M34a, loading from SEC EDGAR, and M34b, the point-in-time checks on
what reads the data, are built and tested; see section 6.
**Implementation**: `crates/spectra-data/src/{fundamentals,edgar}.rs` (store, EDGAR parsing),
`crates/spectra-db/src/fundamentals.rs` (storage), `crates/spectra-api/src/{fundamentals,fundamentals_api}.rs`
(refresh job and endpoints), `crates/spectra-research/src/company_research.rs` (`statement_as_of`)
**Tests**: `crates/spectra-data/tests/m34_{fundamentals,edgar}_reference.rs`,
`crates/spectra-db/tests/fundamentals_store.rs`,
`crates/spectra-api/tests/fundamentals_{refresh_flow,point_in_time}.rs`

## 1. Provider Decision (DL-1)

**Decided: SEC EDGAR, through the XBRL Company Facts API** (`data.sec.gov/api/xbrl/companyfacts`),
with tickers mapped to CIKs from `www.sec.gov/files/company_tickers.json`. Decided 2026-11-09.

- **M1's provider does not supply fundamentals.** M1 refreshes datasets from Alpaca Market Data
  (`MarketDataConnector::fetch_bars`), which serves price bars only, so a fundamentals adapter is
  needed.
- **Why EDGAR:** free, with no licence to buy, and every value carries the date of the filing that
  reported it, which is what a point-in-time store needs.
- **Limits:** US SEC filers only; values are XBRL facts that need mapping to our metric names
  (section 7); the API gives the filing date, not the acceptance time.
- **Later:** a commercial feed (FMP, Polygon, Refinitiv) can be added behind the same adapter shape
  without changing the store or its readers.
- **Requests:** SEC asks every client to send a descriptive `User-Agent` with contact details and to
  stay under 10 requests per second. The refresh job refuses to run without `SEC_USER_AGENT` set.

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

## 6. Remaining Work and Decisions (2026-11-09)

| # | Decision | Choice |
|---|---|---|
| 1 | Provider (DL-1) | SEC EDGAR Company Facts (section 1) |
| 2 | Revisions | Keep every revision with its known-at time; never overwrite (already how the store works) |
| 3 | Where loaded data lives | Postgres, append-only, with a content-addressed version per load (section 8) |
| 4 | Fetching code | In `spectra-api` (HTTP); parsing stays in `spectra-data`, which must remain std-only |
| 5 | Readers in scope for M34b | An as-of fundamentals API, and M37 company research statements built from the store |
| 6 | User-uploaded fundamentals files | Out of scope for M34 (so M29 upload limits are not a dependency) |

M34a, provider and loading (all done):
1. **Provider decision (DL-1)**: SEC EDGAR, recorded in section 1 (#314).
2. **EDGAR parser** (`spectra-data`): Company Facts JSON and the ticker map to `FundamentalRecord`s
   with known-at times and restatement indices. Fixture tests, no network (#315).
3. **Storage** (`spectra-db`): append-only fact, quarantine and load tables; as-of queries (#317).
4. **Refresh job** (`spectra-api`): an EDGAR client, a `fundamentals_refresh` job, a refresh endpoint
   and refresh schedules swept by the worker. A refresh adds new revisions and keeps the old ones
   (#319).

M34b, point-in-time checks (all done):
5. **As-of reads**: `GET /api/v1/fundamentals` and the M37 statement endpoint return only values
   known at `as_of`; tests read before a revision's known-at time and get nothing.
6. **Quarantine coverage**: quarantined rows are stored apart and never reach a reader or a screen;
   a test runs an M36 screen on data loaded from the database and checks a quarantined NaN cannot
   let a symbol through.

## 7. EDGAR Mapping

**Known-at time.** Company Facts gives each value's filing date (`filed`) but not the acceptance
time, and EDGAR accepts filings until 22:00 ET. A value is treated as known from **12:00 UTC on the
day after it was filed**: never before it was public, and before the next US market open.

**Periods.** EDGAR repeats a value in later filings as a comparative, and its `fy`/`fp` fields
describe the filing, not the value. A value is identified by its own dates (`start`, `end`), and its
period label (`2024-Q1`, `2024-FY`) comes from the first filing that reported it. Only quarter-length
(80 to 100 days) and year-length (350 to 380 days) durations are kept; year-to-date durations are
dropped. Balance-sheet values (instants) take the label of their first filing.

**Revisions.** For each symbol, period and metric, filings are read in filing order. The first value
is revision 0; a later filing that reports a different value adds the next revision; a later filing
that repeats the same value adds nothing.

**Metrics.** A metric's concepts are merged value by value: when two concepts report the same value
in the same filing, the one listed first wins, and a concept listed later fills periods the others do
not report (companies move from `Revenues` to `RevenueFromContract…` over the years).

| Metric | Concepts | Unit |
|---|---|---|
| `revenue` | `Revenues`, `RevenueFromContractWithCustomerExcludingAssessedTax`, `SalesRevenueNet` | USD |
| `net_income` | `NetIncomeLoss` | USD |
| `operating_income` | `OperatingIncomeLoss` | USD |
| `gross_profit` | `GrossProfit` | USD |
| `total_assets` | `Assets` | USD |
| `total_liabilities` | `Liabilities` | USD |
| `stockholders_equity` | `StockholdersEquity` | USD |
| `current_assets` | `AssetsCurrent` | USD |
| `current_liabilities` | `LiabilitiesCurrent` | USD |
| `cash_and_equivalents` | `CashAndCashEquivalentsAtCarryingValue` | USD |
| `long_term_debt` | `LongTermDebtNoncurrent`, `LongTermDebt` | USD |
| `operating_cash_flow` | `NetCashProvidedByUsedInOperatingActivities` | USD |
| `capital_expenditures` | `PaymentsToAcquirePropertyPlantAndEquipment` | USD |
| `eps_diluted` | `EarningsPerShareDiluted` | USD/shares |
| `shares_outstanding` | `dei:EntityCommonStockSharesOutstanding` | shares |

## 8. Storage

Loaded values live in Postgres, per workspace:

- **`fundamental_facts`**: one row per symbol, period, metric and filing, with its value, known-at
  time, revision and filing accession number. Rows are never updated (a trigger refuses it) and are
  deleted only with their workspace, so a refresh can only add revisions; reloading a filing adds
  nothing.
- **`fundamental_quarantine`**: rows that failed the quality checks, with the reason. Readers never
  query this table.
- **`fundamental_loads`**: one row per symbol per refresh: the content-addressed `fund_v_` version of
  what was loaded, and how many rows were new, already present or quarantined.
- **`fundamental_refresh_schedules`**: symbols to refresh on an interval; the worker enqueues due
  refreshes, like M1's provider refresh schedules.

M1's `dataset_versions` hold price-bar datasets (bars, checksums, Parquet files), so fundamentals get
their own tables instead. Reproducibility comes from the same two properties: loaded data never
changes, and every load has a content-addressed version a run can cite.

## 9. Reading Fundamentals

All reads are per workspace and need any member role; refreshes and schedules need an editor.

| Endpoint | Returns |
|---|---|
| `GET /api/v1/fundamentals?symbol=&as_of=&metric=` | Each value's latest revision with `known_at <= as_of` |
| `GET /api/v1/fundamentals/statement?symbol=&as_of=&period=` | An M37 statement from those values; lines with no value are `null` and listed in `missing` |
| `POST /api/v1/fundamentals/refresh` | Queues a `fundamentals_refresh` job for 1 to 100 symbols |
| `GET /api/v1/fundamentals/loads`, `/quarantine` | Loads with their `fund_v_` versions; quarantined rows |
| `GET\|PUT\|DELETE /api/v1/fundamentals/schedule` | The workspace's refresh schedule (1 to 720 hours) |

- **`as_of`** is a `YYYY-MM-DD` date, read as the end of that day in UTC, or an RFC 3339 time; it
  defaults to now. A value filed on day D is known from 12:00 UTC on D+1, so `as_of=D` does not
  see it and `as_of=D+1` does.
- **Statements** map loaded metrics to M37 line items. Derived when both inputs are known:
  `cost_of_goods_sold`, `free_cash_flow`, `diluted_shares_outstanding` (net income over diluted EPS).
  `total_debt` is long-term debt. Operating expenses, interest expense and inventory are not loaded
  from EDGAR, so they are always `null`. M37 ratios are not served from these statements, because
  missing lines would be read as zero.
- **Screens** (M36 `run_screen`) take a store built from `facts_known_by`, which returns accepted
  rows known at the screen's time only.
