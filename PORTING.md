# Porting notes

What became of each C++ component, and every place the Rust port behaves differently on purpose.
Anything not listed here behaves as the C++ code did, and the parity tests check that.

## Research engine (`quantforge` 0.6.0)

| C++ | Rust |
| --- | --- |
| `fixed.hpp`, `ledger.*` | `spectra_core::{fixed, ledger}` |
| `json.*`, `sha256.*`, `rng.hpp`, `bar.*` | `spectra_core::{json, sha256, rng, bar}` |
| `data.*`, `datastore.*` | `spectra_data::{time, dataset, datastore}` |
| `costs.*`, `metrics.*`, `backtest.*` | `spectra_engine::{costs, metrics, backtest}` |
| `stats.*`, `pbo.*`, `sweep.*`, `walkforward.*`, `validation.*`, `trades.*`, `benchmark.*`, `survival.*` | `spectra_research::*` |
| `registry.*`, `vault.*` | `spectra_registry::{registry, vault}` |
| `tools/qf_ingest`, `tools/qf_registry` | `spectra-ingest`, `spectra-registry` |
| `examples/*_demo.cpp` | `crates/spectra-cli/examples/{sma_cross, overfitting, research_workflow}.rs` |

Exceptions became `spectra_core::Error` variants carrying the same messages.

### Deliberate differences

- **Gross leverage limit** (`LedgerConfig::max_gross_leverage`, default 1.0x). In C++, short-sale
  proceeds could fund unlimited further buying (a $100k account could reach 2.9x gross). The Rust
  ledger rejects a fill that would raise an instrument's gross exposure past the limit.
  `LedgerConfig::reference(..)` turns the limit off and reproduces C++ exactly.
- **Open-order book indexed by price.** The C++ engine scanned every resting order on every bar. The
  Rust book keeps market orders in a list and limit orders in per-instrument price-ordered sets, then
  processes candidates in submission order. Results are identical, and 40,000 resting orders take
  about 56 ms instead of about 3 s.
- **Mixed bar intervals are rejected.** Instruments whose median bar spacing differs (daily mixed with
  hourly) make annualization meaningless. The C++ engine silently annualized them anyway.
- **Dataset store fsyncs before rename**, so a crash cannot leave a renamed but empty file.
- **Sweep errors are deterministic.** With several failing trials, the error from the lowest-index
  trial is returned whatever the thread count.
- **New Survival Lab statistics** (Sortino, Calmar, Omega and others in
  `spectra_engine::performance`) are computed on return series.
- **Registry locking** uses the standard library's portable file locks (the C++ code used POSIX
  `flock`), so the registry works on Windows. The file format, hashes and ids are unchanged.
- **Registry repair.** A crashed writer leaves an incomplete final line. C++ could only report it;
  `spectra-registry repair` removes it after checking every complete event, and records what it
  removed as a `registry.repaired` event. A log that is broken anywhere else is refused.
- **Stored configurations** gain `max_gross_leverage` only when a limit is set. Records of
  reference-policy runs are byte-identical to C++ ones, and C++ records load as "no limit".
- **Demo strategy guard.** The C++ demos' SMA strategy read before the start of its history when
  `fast > slow` (one grid in the workflow demo). The Rust demos wait until both windows are full. The
  parity test reproduces the C++ behaviour exactly.

## Risk engine (PCPROJECT)

| C++ | Rust |
| --- | --- |
| `portfolio`, `csv_loader`, `validation` | `spectra_risk::portfolio` |
| `risk_config`, `orchestrator`, `backend_selector`, `batch_planner`, `fallback_chain`, `workload_estimator` | `spectra_risk::{engine, simulation}` |
| `rng_manager`, `cpu/reference_kernels`, `cpu_backend` | `spectra_risk::{rng, simulation}` |
| `openmp_backend` | `spectra_risk::simulation::run_parallel` |
| `cuda/*` | not ported |
| `risk/metrics/*`, `convergence` | `spectra_risk::{metrics, engine::check_convergence}` |
| `risk_report`, `metadata` | `spectra_risk::RiskReport` |
| `plugin_registry` | `spectra_risk::{RiskPlugin, RiskEngine::register_plugin}` |
| `cli/*` (`risk`, `benchmark`, `stress`) | `spectra-risk` |
| `web/backend/main.py` | `spectra-api` |
| `web/frontend` | `web/frontend` |

### Deliberate differences

- **Exact RNG reproduction.** `mt19937_64`, libstdc++'s `generate_canonical` and its polar
  `normal_distribution` are reimplemented, so the CPU backend produces the same losses as the C++
  engine, bit for bit.
- **The parallel backend replaces OpenMP.** OpenMP results changed with the thread count. The Rust
  backend simulates fixed chunks of 131,072 paths, each with its own seed (chunk 0 uses the task seed,
  so it equals the CPU backend), and results depend only on the seed and the path count.
- **CUDA requests fall back to the parallel backend** and are reported as a fallback.
- **Each batch gets its own seed.** The C++ orchestrator reused the first batch's seed for every
  batch, so multi-batch runs repeated the same paths. Adaptive runs now use 50,000-path batches with
  distinct seeds; batch 0 equals the non-adaptive run.
- **The CPU cross-check works.** C++ `--verify` rebuilt a different task (zero volatility, a different
  seed) and compared means, so it could not detect a divergence. Rust re-runs the same first task on
  the reference kernel and compares path by path.
- **Horizon is configurable** (`horizon_years`, default 1.0, which equals the C++ behaviour). The C++
  example labelled a one-year result "1-day VaR".
- **The volatility column is read.** The C++ loader documented `symbol,quantity,price[,volatility]`
  but ignored the fourth column, so every asset got 20%. Numbers must parse completely (C++ accepted
  `12abc` as 12), and non-finite values are rejected.
- **HHI is computed** from gross position weights. C++ returned 1.0.
- **No placeholder metrics.** C++ returned VaR for Marginal, Component and Incremental VaR, 0.7 for
  Beta and 1.0 for the Diversification Ratio. It computed drawdowns and Sharpe-style ratios over the
  order of independent simulation paths, with hard-coded 2% and 5% rates. These names are still
  recognized but return `Unsupported` with the reason. Time-series ratios are available on backtest
  returns in `spectra_engine::performance`.
- **Reports are printed at full precision.** C++ printed 6 significant digits; the field names are
  unchanged and fields were added (requested backend, horizon, portfolio value, model volatility,
  convergence).
- **Stress output** adds the scenario's immediate loss and the values before and after the shock.
- **The API calls the engine directly.** The Python backend wrote temporary files and parsed CLI
  output. The routes, validation messages and JSON shapes are kept, and the API also adds CORS
  allow-listing, request size limits and socket timeouts. It binds to `127.0.0.1` by default (the
  Python one bound `0.0.0.0`).
- **The Alpaca routes return 501**: they need an HTTPS client, which is not in the standard library.
