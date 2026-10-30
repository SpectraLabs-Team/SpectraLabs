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
- **macOS:** the app runs on macOS and the Risk Lab dependence models (Aggregate, Gaussian,
  T-Copula) have been checked there by hand. The full Rust test suite has not been run on macOS yet.

## Platform follow-ups

- **Separate database roles for schema migration vs. application runtime.** Supported and hardened via `spectra-db provision-roles` and `scripts/setup-db-roles.sh`. The migration runner operates as the schema owner, while `spectra-api` and `spectra-worker` connect as `POSTGRES_APP_USER` (`spectra_app`) with read-only access to users/workspaces and read-write access to engine tables.
- **Sentry error tracking delivery.** Verified and monitored via `/api/v1/observability/sentry` status inspection and `/api/v1/observability/sentry/verify` test event dispatch and flush.
- **Artifact bytes of deleted workspaces:** resolved. Every 30 minutes the worker deletes stored files
  that no workspace references any more (`sweep_unreferenced` in `crates/spectra-db/src/artifacts.rs`,
  commit `b055adf`). Files still referenced by another workspace are kept.

## Model limits (inherited)

- The default **Aggregate** risk model treats the portfolio as one aggregate position: zero drift, and
  volatility equal to the simple average of the assets' volatilities, so it treats the assets as
  perfectly correlated. The **Gaussian** and **T-Copula** dependence models simulate correlated
  per-asset paths (the T-Copula adds tail dependence); use them when diversification or joint crashes matter.
- Data: CSV only. There is no Parquet (corporate actions, exchange calendars, point-in-time revisions with symbol changes and delistings, and market data provider connectors with scheduled refreshes were added in tasks 10–13); datasets are held fully in memory during a backtest (ingest streams since M1b task 9; streaming backtests are M3 task 14).
- Backtester: market, limit, stop and stop-limit orders; flat-bps slippage, bid-ask spread costs, volume-based slippage and square-root market impact; a single currency and average-cost accounting.
- PSR and DSR assume serially independent returns and independent trials.
- The registry loads the whole log into memory. The hash chain shows tampering but does not prevent
  it: anchor the head hash somewhere you trust.
- The holdout vault is enforced by the database (M4a). A dataset version's checksum is the key to its
  bars, and the queries the services use never return a sealed holdout's checksum:
  `resolve_dataset_version()` refuses it and logs the attempt in `holdout_access_log`, and
  `grant_holdout_evaluation()` is the only way to it. That function spends one unit of budget, logs
  the grant, and only then returns the data, under the holdout's row lock, so concurrent evaluations
  cannot overspend. The access log is append-only, and a holdout's budget cannot be refunded or moved
  to other data. Every route and worker that loads bars goes through these functions, including the
  generic `POST /api/v1/jobs` route, which previously did not check. Checked by
  `crates/spectra-db/tests/holdout_checks.sql` and `crates/spectra-api/tests/optimization_flow.rs`.
- **Limit:** the guard covers the queries the services ship. A raw SQL session with the
  application role (`spectra_app`) can still `SELECT checksum FROM dataset_versions`, because that
  role reads the table directly. Closing that needs column privileges on `checksum` and
  `blob_sha256`, with the remaining queries moved behind `SECURITY DEFINER` functions (follow-up).
- **Holdout evaluation does not yet evaluate the requested strategy.** `evaluate_holdout` always runs
  the built-in `buy_and_hold` with default parameters, whatever strategy is sent, and if the sealed
  bars cannot be loaded it runs on two synthetic bars and records the result as real. Fix before
  relying on holdout results.

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
