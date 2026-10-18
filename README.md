# Spectra Labs

Evidence-first quantitative research and portfolio-risk platform, written in Rust.

This repository combines and ports the two earlier C++ codebases into one Rust workspace:

- the **research engine** (`quantforge` 0.6.0): money and ledger, data ingest, backtester, sweeps,
  walk-forward, DSR/PBO validation, Survival Lab, experiment registry and holdout vault;
- the **risk engine** (PCPROJECT): Monte Carlo VaR/CVaR, distribution metrics, stress tests and the
  backend benchmark, with its FastAPI backend and Next.js dashboard.

CUDA is not part of this port. Everything else is, with the exceptions listed in
[KNOWN_ISSUES.md](KNOWN_ISSUES.md). The workspace uses the Rust standard library only (no third-party
crates).

## Layout

| Crate / folder | What it is | Ported from |
| --- | --- | --- |
| `crates/spectra-core` | Fixed-point money types, accounting ledger, JSON, SHA-256, RNG, printf-exact formatting | research engine |
| `crates/spectra-data` | CSV ingest, data-quality report, content-addressed dataset store | research engine |
| `crates/spectra-engine` | Event-driven backtester, costs, metrics, performance ratios | research engine |
| `crates/spectra-research` | Sweeps, walk-forward, PSR/DSR/MinTRL, CSCV PBO, trade and benchmark analysis, Survival Lab | research engine |
| `crates/spectra-registry` | Hash-chained experiment registry, holdout vault, run reproduction | research engine |
| `crates/spectra-risk` | Monte Carlo risk engine (CPU and parallel backends), metrics, stress, benchmark, plugins | risk engine |
| `crates/spectra-cli` | `spectra-ingest`, `spectra-registry`, `spectra-risk` binaries and three demos | both |
| `crates/spectra-api` | HTTP API for the dashboard (replaces the FastAPI backend) | risk engine `web/backend` |
| `web/frontend` | Next.js dashboard | risk engine `web/frontend` |
| `parity/` | C++ golden-output programs, inputs and recorded outputs | new |
| `examples/` | Sample portfolio CSV | risk engine |

[PORTING.md](PORTING.md) maps every C++ component to its Rust home and lists each deliberate
behaviour change.

## Build and test

Requires Rust 1.89 or newer.

```bash
cargo build --release
cargo test --workspace                 # debug: overflow checks on
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Run

```bash
# Risk engine
./target/release/spectra-risk risk examples/sample_portfolio.csv --metric cvar --horizon-days 1
./target/release/spectra-risk stress examples/sample_portfolio.csv --scenario market-crash
./target/release/spectra-risk benchmark examples/sample_portfolio.csv --simulations 100000
./target/release/spectra-risk metrics          # which metric names are computable

# Data and registry
./target/release/spectra-ingest data.csv --store ./store
./target/release/spectra-registry --root ./registry verify

# Demos
cargo run --release -p spectra-cli --example sma_cross
cargo run --release -p spectra-cli --example overfitting
cargo run --release -p spectra-cli --example research_workflow

# Dashboard: API, then the frontend
./target/release/spectra-api                   # http://127.0.0.1:8000
cd web/frontend && cp .env.example .env.local && npm install && npm run dev   # http://localhost:3000
```

API settings: `SPECTRA_API_ADDR` (default `127.0.0.1:8000`) and `SPECTRA_CORS_ORIGINS` (default
`http://localhost:3000`). Set `SPECTRA_GIT_COMMIT` at build time to stamp reports with a commit.

## Portfolio CSV

```text
symbol,quantity,price[,volatility]
AAPL,1000,190.00,0.28
MSFT,500,420.00
```

The optional fourth column is annualized volatility; assets without one use 20%. The default risk
horizon is one year (`--horizon-days 1` for one trading day).

## How the port was verified

Each C++ codebase was built and frozen as the reference. Programs in `parity/cpp/` run fixed
scenarios on it and record the output in `parity/golden/`; the Rust tests must reproduce those files
exactly:

| Area | Evidence |
| --- | --- |
| Ledger | 12 scripts, 4,800 operations, byte-identical journals and books |
| Data | 31 CSV cases; byte-identical `bars.qfb` and `meta.json` |
| Backtester | 39 runs with identical result fingerprints |
| Research | 565-line transcript: every trial fingerprint, PBO, DSR, walk-forward and Survival Lab report |
| Registry | a 140-event registry log byte-identical to the C++ one; each engine verifies the other's logs |
| Risk | Monte Carlo losses bit-identical path by path (CPU backend); every metric, engine run and error message |

Each parity test was mutation-checked (a one-character change to the Rust code makes it fail).
`parity/regen.sh` and `parity/regen_risk.sh` rebuild the golden files from the C++ sources. The
bit-exact tests run on Linux with glibc, where the goldens were produced.
