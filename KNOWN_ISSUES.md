# Known issues and limits

## Not ported

- **CUDA backend.** Excluded on purpose. Requests for CUDA run on the parallel CPU backend and are
  reported as a fallback.
- **Alpaca brokerage proxy** (`/api/alpaca/*`). Live trading order proxy routes answer 501 until the order execution proxy is wired. Market data historical bars and scheduled dataset refreshes from Alpaca are fully supported via `AlpacaConnector` in `spectra-api` (M1b task 13).

## Verification gaps

- **CI still needs to verify the latest commit.** Rust workspace tests, formatting, clippy, frontend
  lint, typecheck and production build pass locally on Windows. The frontend integration tests need
  Postgres and are run by CI. Confirm CI is green for the exact commit being merged.
- **Compose cannot be run from this Windows environment** because Docker is not installed. The PR
  workflow now has steps to build both images, start a fresh stack and check `/ready`, exercise the
  proxy and monitoring profiles, and store a real job artifact through the MinIO profile. Confirm
  those steps pass before merging.
- **Bit-exact parity tests run only on Linux with glibc.** The goldens depend on glibc's `exp`,
  `log` and `pow`, so those tests are compiled out elsewhere. All behavioural tests run on every
  platform.
- **Not tested on macOS.** The full Rust test suite and frontend checks passed locally on Windows;
  no macOS run has been reported.

## Platform follow-ups

- **One database role for every service.** The dashboard, API, worker and migrations connect as the
  same Postgres user. Separate roles (migrations own the schema; the API and worker get only the
  grants they need on engine tables and read access to `workspaces`) are a hardening step before
  production.
- **Artifact bytes of deleted workspaces stay in the store.** Their rows go with the workspace and no
  longer count against a quota, but the content-addressed files are not garbage-collected yet (another
  workspace may hold the same bytes). A sweep that removes files no `run_artifacts` row references is
  still to be written.

## Model limits (inherited)

- The risk model treats the portfolio as one aggregate position: zero drift, and volatility equal to
  the simple average of the assets' volatilities. There are no correlations and no per-asset paths, so
  it cannot attribute risk to positions, and it treats the assets as perfectly correlated.
- Data: CSV only. There is no Parquet (corporate actions, exchange calendars, point-in-time revisions with symbol changes and delistings, and market data provider connectors with scheduled refreshes were added in tasks 10–13); datasets are held fully in memory during a backtest (ingest streams since M1b task 9; streaming backtests are M3 task 14).
- Backtester: market, limit, stop and stop-limit orders; flat-bps slippage, bid-ask spread costs, volume-based slippage and square-root market impact; a single currency and average-cost accounting.
- PSR and DSR assume serially independent returns and independent trials.
- The registry loads the whole log into memory. The hash chain shows tampering but does not prevent
  it: anchor the head hash somewhere you trust.
- The holdout vault enforces access control at the API & database boundary: direct reads of sealed bars, exports and backtests are prohibited without a metered evaluate call.

## API deployment boundary

`spectra-api` is designed to stay on loopback/private networking behind the Next.js proxy or a
trusted reverse proxy. Engine routes verify HMAC-signed requests; browser authentication and
rate-limiting are handled by the frontend. Terminate public TLS at the edge proxy and do not expose
the engine listener directly to the internet. The API runs at most 32 engine computations at a
time and answers 503 to further compute requests instead of queueing them.

## Platform ops (M20)

M20 is implemented on branch `m20/finish` and reaches `main` when it is merged. Status by step is in
[M20-COMPLETE.md](M20-COMPLETE.md); deployment is in [docs/operations.md](docs/operations.md) and
scaling in [docs/scaling-guide.md](docs/scaling-guide.md).

- **`Cargo.lock` must be regenerated once.** Steps 12 and 13 add the optional `object_store`
  dependency (the `s3` feature). Optional dependencies are recorded in the lock file, so the first
  `cargo check` with crates.io access updates `Cargo.lock`; commit it. Until then
  `docker build` (which uses `--locked`) fails.
- **Deployment is CI-verified, not manually operated here.** Container startup and MinIO artifact
  storage are smoke-tested in the PR workflow; configure `SENTRY_DSN` and confirm delivery in the
  selected Sentry or GlitchTip instance before relying on remote error reports.
- **S3 migration needs an explicit copy.** Existing local-volume artifacts are not copied when
  switching to S3; use the documented sync procedure before switching a deployment.

## Security

- The original `web/backend/.env` contained live Alpaca API keys. It was not copied into this
  repository. **Rotate those keys**, because they existed in a shared archive.
- `.gitignore` excludes `.env` and `.env.*` (except `.env.example`), `node_modules` and build output.
