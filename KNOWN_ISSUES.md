# Known issues and limits

## Not ported

- **CUDA backend.** Excluded on purpose. Requests for CUDA run on the parallel CPU backend and are
  reported as a fallback.
- **Alpaca brokerage proxy** (`/api/alpaca/*`). It needs an HTTPS client and TLS, which `spectra-api`
  does not include yet. The routes answer 501. The dashboard does not call them. Porting them means
  adding an HTTP client crate such as `reqwest` with `rustls` to `spectra-api`.

## Verification gaps

- **CI still needs to verify the latest commit.** On `m20/finish`, `cargo check --workspace` and
  `cargo test --workspace` passed locally on Windows. Confirm the same checks pass in CI for the
  exact commit being merged.
- **Frontend checks passed locally on Windows.** `npm run typecheck`, `npm run build`, and the
  frontend `npm test` suite passed. These local results do not replace the CI run for the commit
  being merged.
- **Bit-exact parity tests run only on Linux with glibc.** The goldens depend on glibc's `exp`,
  `log` and `pow`, so those tests are compiled out elsewhere. All behavioural tests run on every
  platform.
- **Not tested on macOS.** The full Rust test suite and frontend checks passed locally on Windows;
  no macOS run has been reported.

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

## API deployment boundary

`spectra-api` is designed to stay on loopback/private networking behind the Next.js proxy or a
trusted reverse proxy. Engine routes verify HMAC-signed requests; browser authentication and
rate-limiting are handled by the frontend. Terminate public TLS at the edge proxy and do not expose
the engine listener directly to the internet. The API runs at most 32 engine computations at a
time and answers 503 to further compute requests instead of queueing them.

## Platform ops (M20)

The M20 platform features are implemented on branch `m20/finish`: the jobs API and worker, artifact
storage, OpenAPI endpoint, Prometheus `/metrics`, worker-aware `/ready`, Postgres-backed frontend
authentication, and the `just import-users` recipe. The worker and API routes are wired into the
workspace. These items are not present on `main` until `m20/finish` is merged.

## Security

- The original `web/backend/.env` contained live Alpaca API keys. It was not copied into this
  repository. **Rotate those keys**, because they existed in a shared archive.
- `.gitignore` excludes `.env` and `.env.*` (except `.env.example`), `node_modules` and build output.
