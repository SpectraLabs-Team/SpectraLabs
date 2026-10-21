# Spectra Labs

Evidence-first quantitative research and portfolio-risk platform, written in Rust.

This repository combines and ports the two earlier C++ codebases into one Rust workspace:

- the **research engine** (`quantforge` 0.6.0): money and ledger, data ingest, backtester, sweeps,
  walk-forward, DSR/PBO validation, Survival Lab, experiment registry and holdout vault;
- the **risk engine** (PCPROJECT): Monte Carlo VaR/CVaR, distribution metrics, stress tests and the
  backend benchmark, with its FastAPI backend and Next.js dashboard.

CUDA is not part of this port. Everything else is, with the exceptions listed in
[KNOWN_ISSUES.md](KNOWN_ISSUES.md). The engine and research crates (`spectra-core`, `-data`, `-engine`,
`-research`, `-registry`, `-risk`) use the Rust standard library only, so parity results cannot drift with
a dependency upgrade. Third-party crates are allowed only in the service crates: `spectra-api` runs on
`axum` and `tokio`.

## Layout

| Crate / folder | What it is | Ported from |
| --- | --- | --- |
| `crates/spectra-core` | Fixed-point money types, accounting ledger, JSON, SHA-256, RNG, printf-exact formatting | research engine |
| `crates/spectra-data` | CSV ingest, data-quality report, content-addressed dataset store | research engine |
| `crates/spectra-engine` | Event-driven backtester, costs, metrics, performance ratios | research engine |
| `crates/spectra-research` | Sweeps, walk-forward, PSR/DSR/MinTRL, CSCV PBO, trade and benchmark analysis, Survival Lab | research engine |
| `crates/spectra-backtest` | Backtests as data: config form and presets, built-in strategies, controlled runs, CSV artifacts, summary and chart series (for the backtest job) | new |
| `crates/spectra-registry` | Hash-chained experiment registry, holdout vault, run reproduction | research engine |
| `crates/spectra-risk` | Monte Carlo risk engine (CPU and parallel backends), metrics, stress, benchmark, plugins | risk engine |
| `crates/spectra-cli` | `spectra-backtest`, `spectra-ingest`, `spectra-registry`, `spectra-risk` binaries, three demos and a throughput benchmark | both |
| `crates/spectra-api` | HTTP API for the dashboard on axum + tokio (replaces the FastAPI backend) | risk engine `web/backend` |
| `crates/spectra-db` | Postgres schema (migrations), connection pool, dev seed; `spectra-db` CLI | new |
| `web/frontend` | Next.js dashboard | risk engine `web/frontend` |
| `parity/` | C++ golden-output programs, inputs and recorded outputs | new |
| `examples/` | Sample portfolio CSV | risk engine |

[PORTING.md](PORTING.md) maps every C++ component to its Rust home and lists each deliberate
behaviour change.

## Build and test

The toolchain is pinned in `rust-toolchain.toml` (rustup installs it on first use); CI and the
Docker build use the same version, so formatting and lints agree everywhere. The minimum supported
Rust for the crates is 1.89.

```bash
cargo build --release
cargo test --workspace                 # debug: overflow checks on
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

### Before you push

```bash
scripts/install-hooks.sh     # once per clone (npm ci in web/frontend also does it)
```

The hooks run the CI checks locally: **pre-commit** checks formatting (seconds), **pre-push** runs
`scripts/ci-local.sh` (`just ci`) for what the branch changed — rustfmt, clippy with every feature
set, tests, the std-only check, the generated API description and, when `DATABASE_URL` /
`TEST_DATABASE_URL` are set, the Postgres and dashboard tests. `just ci --all` runs everything.
`main` only accepts pull requests whose CI checks pass and that are up to date with `main`.

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
cd web/frontend && npm install && npm run setup && npm run dev   # http://localhost:3000
```

## Running the platform

One command, with Docker:

```bash
cp .env.example .env                                  # set POSTGRES_PASSWORD, ENGINE_SECRET, ADMIN_EMAIL
cp web/frontend/.env.example web/frontend/.env.local  # set AUTH_SECRET, the same ENGINE_SECRET, Gmail
docker compose up --build                             # http://localhost:3000
```

This starts Postgres, applies the migrations (and seeds a `dev` workspace owned by `ADMIN_EMAIL`),
then the engine API and the dashboard. Only the dashboard (and Postgres, for `psql`) are published,
on 127.0.0.1. The engine stays inside the compose network and accepts only requests signed by the
dashboard's server with `ENGINE_SECRET`.

To run the API and dashboard natively against the containerised database, with
[`just`](https://just.systems): `just db`, `just seed`, then run `just api`, `just worker` and
`just web` in separate terminals. `just import-users-dry-run` previews importing the old
`web/frontend/.data/users.json`; `just import-users` runs the import. `just` lists every task;
`just ci` runs what CI runs.

API settings are read once at startup, and the API refuses to start until every problem is fixed:
`DATABASE_URL` and `ENGINE_SECRET` are required; `SPECTRA_API_ADDR` (default `127.0.0.1:8000`),
`SPECTRA_CORS_ORIGINS` (default `http://localhost:3000`), `SPECTRA_DB_MAX_CONNECTIONS` (default 10) and
`SPECTRA_MIGRATE_ON_START` (default false) are optional. The API stops cleanly on Ctrl-C or SIGTERM, letting in-flight requests finish. Set `SPECTRA_GIT_COMMIT` at build time to stamp reports with a commit.

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
