# Known issues and limits

## Not ported

- **CUDA backend.** Excluded on purpose. Requests for CUDA run on the parallel CPU backend and are
  reported as a fallback.
- **Alpaca brokerage proxy** (`/api/alpaca/*`). It needs an HTTPS client and TLS, which `spectra-api`
  does not include yet. The routes answer 501. The dashboard does not call them. Porting them means
  adding an HTTP client crate such as `reqwest` with `rustls` to `spectra-api`.

## Verification gaps

- **The dashboard was not rebuilt.** The npm registry was not reachable, so `npm install`,
  `next build` and type-checking were not run after the edits (rebranding, the OpenMP to Parallel
  rename, the trimmed metric list, the `.env.example` port fix). The API it calls is tested in
  `crates/spectra-api/tests`. Run `npm install && npm run build` before deploying.
- **Bit-exact parity tests run only on Linux with glibc.** The goldens depend on glibc's `exp`,
  `log` and `pow`, so those tests are compiled out elsewhere. All behavioural tests run on every
  platform.
- **Not tested on Windows or macOS.** The code uses only portable standard-library APIs, including the
  registry's file locks, but no test has been run on those systems.

## Model limits (inherited)

- The risk model treats the portfolio as one aggregate position: zero drift, and volatility equal to
  the simple average of the assets' volatilities. There are no correlations and no per-asset paths, so
  it cannot attribute risk to positions, and it treats the assets as perfectly correlated.
- Data: CSV only. There is no Parquet, no provider connectors, no corporate actions and no exchange
  calendars, and datasets are held fully in memory.
- Backtester: market and limit orders only, bar data only, flat-bps slippage, a single currency and
  average-cost accounting.
- PSR and DSR assume serially independent returns and independent trials.
- The registry loads the whole log into memory. The hash chain shows tampering but does not prevent
  it: anchor the head hash somewhere you trust.
- The holdout vault is process discipline plus an audit trail, not access control.

## API hardening before public exposure

`spectra-api` is meant for local use or to sit behind a reverse proxy. Before exposing it, add TLS,
authentication and rate limiting in front of it. It runs at most 32 engine computations at a time
and answers 503 to further compute requests instead of queueing them.

## Platform ops (M20) in progress

- **Rust service code was written without a compiler.** The build environment could not reach
  crates.io, so `spectra-api` (axum) and `spectra-db` (sqlx) were not compiled before review. The
  HMAC signing was compiled and tested against RFC 4231 and against the Node signer, and the SQL
  migration and seed were run against Postgres 16. Run `just check` before merging.
- **No OpenAPI spec yet.** `utoipa` lands with the first serde-typed routes (jobs, step 6).
- **`/ready` checks only the database.** The worker check joins it with the worker (step 6).
- **Users still live in `users.json`** until step 4 moves them to Postgres.

## Security

- The original `web/backend/.env` contained live Alpaca API keys. It was not copied into this
  repository. **Rotate those keys**, because they existed in a shared archive.
- `.gitignore` excludes `.env` and `.env.*` (except `.env.example`), `node_modules` and build output.
