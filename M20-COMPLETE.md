# M20 Platform Ops: implementation status

Branch `m20/finish` is being completed against the M20 plan. The **Verification** column separates
automated checks from deployment checks; a green CI run is still required before merging.

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
| 6 Job queue and worker | `POST /api/v1/jobs`, `GET /api/v1/jobs[/{id}]`, `POST /api/v1/jobs/{id}/cancel` (signed, workspace-scoped, payload validated before queueing); `spectra-worker`: `SKIP LOCKED` claiming, heartbeats, stale requeue, retry with backoff, cancellation, per-workspace running limit; benchmark repetitions report persisted progress and Studio can cancel or resume polling after an API restart | queue SQL checks against Postgres 16; restart recovery is a manual check in TESTING.md; CI pending |
| 7 Artifact storage | content-addressed (SHA-256) store, atomic writes, verified reads, per-workspace quota checked twice; job results carry `{run_id, report, artifact}` | unit tests; CI |
| 8 Configuration | `Config` / `WorkerConfig` from the environment, every problem reported at startup, secrets never printed | config tests (compiled and run) |
| 9 Docker | one `docker-compose.yml`: postgres, migrate (and seed), api, worker, frontend; private engine network; health checks on every long-running service | compose file validated; containers not started in review |
| 10 CI | fmt, clippy, debug/release tests, Postgres SQL/backup/restore/anchor checks, generated-type drift, standard-library-only dependency check, frontend typecheck/lint/tests/production build, Docker image builds and a fresh Compose readiness smoke test | workflow changes in this branch; green run still required |

## M20b: production

| Step | What is there | Verified |
|---|---|---|
| 11 Observability | JSON API request logs include the request ID; Prometheus API and worker endpoints export request/job duration, queue depth and database pool gauges sampled every 15 seconds; optional Sentry/GlitchTip capture from `SENTRY_DSN`; monitoring profile provisions Prometheus and a Grafana overview dashboard; `/ready` reports live workers without requiring them | API tests and metrics listener checks; CI pending; configure a real DSN for deployed error reporting |
| 12 Deployment | compose profiles `proxy` (Caddy, automatic HTTPS for `SITE_ADDRESS`, security headers) and `monitoring` (Prometheus, discovers every worker replica); `scripts/backup-postgres.sh` (custom-format dump, archive check, checksum, retention), `scripts/restore-postgres.sh` (restore into a scratch DB, then check tables, migrations and every registry chain), `scripts/anchor-registry-hash.sh` (per-workspace heads, digest log, optional upload); `deploy/{staging,production}.env.example`; runbook | scripts run end to end against Postgres 16 (incl. corrupted backup, refused live restore, broken chain) and in CI; Caddy/Prometheus/MinIO containers not started in review |
| 13 Scaling | `ArtifactBackend`: local volume or S3-compatible bucket (`--features s3`, `ARTIFACT_STORAGE=s3`, MinIO override in `deploy/compose.s3.yml`); `just scale-workers N`, workers split by kind; OpenAPI → TypeScript and a typed jobs client; general `Plugin` metadata contract with versioned `RiskPlugin` specialization | S3 has an in-memory test; real S3/MinIO runtime verification is scheduled in CI; no dynamic plugin loading |

## Remaining verification

1. Run a green CI workflow on this branch; it includes fresh Compose readiness and Docker builds.
2. Check the Studio restart/cancel flow locally with the instructions in `TESTING.md`.
3. Configure `SENTRY_DSN` and verify event delivery in the chosen Sentry-compatible service before
   relying on remote error reports.

## Not in M20

Run-time plugin loading (plugins are compiled in) and an artifact download route for the dashboard.
