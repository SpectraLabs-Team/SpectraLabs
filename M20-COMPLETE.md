# M20 Platform Ops: status

Branch `m20/finish` (on top of `main` at `d471847`). Every step of the M20 plan is implemented; the
**Verified** column says how far each part has been checked. "CI" means the first full build and
test of that code is the CI run on this branch; check it before merging.

How to test everything by hand: [TESTING.md](TESTING.md). Deployment: [docs/operations.md](docs/operations.md).
Scaling: [docs/scaling-guide.md](docs/scaling-guide.md). Open items: [KNOWN_ISSUES.md](KNOWN_ISSUES.md#platform-ops-m20).

## M20a: core platform

| Step | What is there | Verified |
|---|---|---|
| 1 HTTP layer | axum 0.8 on tokio; bounded blocking pool for engine work (32, then 503); graceful shutdown | built and tested on Windows; API tests |
| 2 API conventions | `/api/v1` routes with `/api` aliases for engine routes; one error shape `{code, message, details, request_id}`; `X-Request-Id` (UUIDv7) on every response; `GET /api/v1/openapi.json` (utoipa) | API tests, spec test |
| 3 Postgres schema | `spectra-db`: sqlx migrations for users, workspaces, members, datasets, strategies, experiments, runs, run artifacts, jobs, workers, audit and registry events (hash chain enforced by trigger) | schema SQL checks against Postgres 16; CI |
| 4 Users in Postgres | dashboard auth on Postgres (`lib/server/authStore.ts` + `db.ts`); `just import-users` / `import-users-dry-run` for the old `users.json` | frontend tests on Windows |
| 5 HMAC trust | the Next.js proxy signs method, path, user, workspace, request id, timestamp and body hash; the API rejects unsigned, forged or >60 s old requests | RFC 4231 vectors, Node and Rust signers cross-checked |
| 6 Job queue and worker | `POST /api/v1/jobs`, `GET /api/v1/jobs[/{id}]`, `POST /api/v1/jobs/{id}/cancel` (signed, workspace-scoped, payload validated before queueing); `spectra-worker`: `SKIP LOCKED` claiming, heartbeats, stale requeue, retry with backoff, cancellation, per-workspace running limit; the proxy enforces viewer/editor roles on these routes | queue SQL checks against Postgres 16; API tests; CI |
| 7 Artifact storage | content-addressed (SHA-256) store, atomic writes, verified reads, per-workspace quota checked twice; job results carry `{run_id, report, artifact}` | unit tests; CI |
| 8 Configuration | `Config` / `WorkerConfig` from the environment, every problem reported at startup, secrets never printed | config tests (compiled and run) |
| 9 Docker | one `docker-compose.yml`: postgres, migrate (and seed), api, worker, frontend; private engine network; health checks on every long-running service | compose file validated; containers not started in review |
| 10 CI | fmt, clippy (pedantic, warnings are errors), tests in debug and release, Postgres service, SQL checks, backup/restore/anchor scripts, generated-types drift, frontend typecheck and tests | runs on every push |

## M20b: production

| Step | What is there | Verified |
|---|---|---|
| 11 Observability | JSON logs (`tracing`, `RUST_LOG`); Prometheus `GET /metrics` on the API (latency and status by route pattern) and on each worker (`SPECTRA_WORKER_METRICS_ADDR`: job claims, completions, durations), with histogram buckets; `/ready` reports live workers without requiring them | API tests; CI |
| 12 Deployment | compose profiles `proxy` (Caddy, automatic HTTPS for `SITE_ADDRESS`, security headers) and `monitoring` (Prometheus, discovers every worker replica); `scripts/backup-postgres.sh` (custom-format dump, archive check, checksum, retention), `scripts/restore-postgres.sh` (restore into a scratch DB, then check tables, migrations and every registry chain), `scripts/anchor-registry-hash.sh` (per-workspace heads, digest log, optional upload); `deploy/{staging,production}.env.example`; runbook | scripts run end to end against Postgres 16 (incl. corrupted backup, refused live restore, broken chain) and in CI; Caddy/Prometheus/MinIO containers not started in review |
| 13 Scaling | `ArtifactBackend`: local volume or S3-compatible bucket (`--features s3`, `ARTIFACT_STORAGE=s3`, MinIO override in `deploy/compose.s3.yml`); `just scale-workers N`, workers split by kind; OpenAPI → TypeScript (`just api-client`, checked for drift in CI) and a typed jobs client (`web/frontend/lib/jobs.ts`); versioned `RiskPlugin` (`PLUGIN_API_VERSION`, `RiskEngine::plugins()`) | plugin code compiled, clippy-clean, tested; `jobs.ts` type-checked; S3 store has an in-memory test (CI); not yet run against real S3 or MinIO |

## Before merging `m20/finish`

1. Run `cargo check` once with network access and commit the updated `Cargo.lock` (new optional
   `object_store` dependency; Docker builds use `--locked`).
2. Run `just api-client` and commit `web/frontend/lib/generated/` (CI warns until it exists).
3. Wait for a green CI run on the branch. Parts of steps 12 and 13 (`artifacts.rs`, the worker's
   metrics listener, the OpenAPI binary) were reviewed but first compiled by CI.
4. Optional but recommended: `just up-s3` once, and `docker compose --profile proxy --profile monitoring up -d`
   once, following TESTING.md sections 7 and 8.

## Not in M20

Run-time plugin loading (plugins are compiled in), job progress between 0 and 1 (engine calls are
single blocking computations), and an artifact download route for the dashboard.
