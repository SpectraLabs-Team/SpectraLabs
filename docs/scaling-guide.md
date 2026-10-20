# Scaling (M20 step 13)

What exists, how to switch it on, and what it costs. Deployment basics are in
[operations.md](operations.md).

## More workers

Workers claim jobs with `FOR UPDATE SKIP LOCKED`, heartbeat every 5 s, and requeue the jobs of a
worker that has been silent for 30 s, so any number can share one database.

```bash
just scale-workers 3                     # docker compose up -d --scale worker=3
```

Prometheus finds every replica on its own (DNS discovery of `worker`). Per worker, tune
`SPECTRA_WORKER_CONCURRENCY` (jobs at once, default 2) to the CPU cores it has; across all workers,
`SPECTRA_WORKSPACE_MAX_RUNNING` caps how many jobs one workspace runs at once, so one heavy user
cannot take every slot.

**Splitting by job kind.** Give a worker `SPECTRA_WORKER_KINDS=benchmark` (any comma-separated
subset of `benchmark,risk,stress`) and it claims only those. Use it to put long benchmarks on their
own machines so quick risk jobs never wait behind them: add a second worker service in a compose
override with a different `SPECTRA_WORKER_KINDS`.

**Outside compose** (Kubernetes, Nomad, plain VMs) a worker needs only `DATABASE_URL`, the
artifact settings below and, for health checks and metrics, `SPECTRA_WORKER_METRICS_ADDR`. Use
`GET :9100/health` as the liveness probe, give it at least 60 s to stop (running jobs finish on
SIGTERM), and run as many replicas as you like.

## Artifacts in S3-compatible storage

Reports (and later equity curves and trade lists) are content-addressed: the key is the SHA-256 of
the bytes, every read is re-verified, and identical bytes are stored once. By default they live on
the `artifacts` volume, which every worker must share. For several hosts, or to stop worrying about
disk, use a bucket instead (AWS S3, Cloudflare R2, MinIO, anything S3-compatible).

The S3 code is behind the `s3` cargo feature, so builds without it pull in no object-store
dependencies.

**Try it locally with MinIO:**

```bash
just up-s3      # docker compose -f docker-compose.yml -f deploy/compose.s3.yml --profile minio up -d --build
```

This builds the worker with `--features spectra-api/s3`, starts MinIO (console on
`http://127.0.0.1:9001`), creates the bucket and sets `ARTIFACT_STORAGE=s3`.

**In production** create the bucket (private, versioning or object lock recommended), give the
worker credentials limited to `s3:GetObject` and `s3:PutObject` on it (`GetObject` also covers
the existence check the worker makes before uploading), and set in `.env`:

```
ARTIFACT_S3_BUCKET=spectra-artifacts-production
ARTIFACT_S3_PREFIX=                        # optional key prefix
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
AWS_REGION=eu-west-1
AWS_ENDPOINT=https://s3.eu-west-1.amazonaws.com
AWS_ALLOW_HTTP=false
```

then `docker compose -f docker-compose.yml -f deploy/compose.s3.yml up -d --build`.

**Moving existing artifacts:** the key layout is the same as on disk
(`<prefix>/sha256/ab/cd/<hash>`), so copy the volume's `sha256/` directory once before switching,
e.g. `aws s3 sync <volume>/sha256 s3://$ARTIFACT_S3_BUCKET/sha256`. Database rows do not change.

Storage costs are small: a risk report is a few kilobytes; at S3 Standard prices a million of them
cost cents per month.

## Typed API client for the dashboard

The engine describes itself with OpenAPI (`GET /api/v1/openapi.json`, generated from the Rust
handlers with utoipa).

- `web/frontend/lib/jobs.ts` is the typed client for the job queue: `createJob`, `getJob`,
  `listJobs`, `cancelJob` and `waitForJob` (polls until done, with progress callbacks and abort).
  It goes through the authenticated proxy like every other engine call.
- `just api-client` prints the spec from the code (no server needed) to
  `web/frontend/lib/generated/engine-api.json` and generates `engine-api.ts` from it with
  `openapi-typescript`. Commit both. CI regenerates them and fails if the committed copies are
  stale, so an API change cannot silently drift from the dashboard's types.

```ts
import { createJob, waitForJob } from '@/lib/jobs';

const job = await createJob({ kind: 'benchmark', payload: { portfolio_csv, simulations: 100_000 } });
const done = await waitForJob(job.id, { onUpdate: (j) => setStatus(j.status) });
if (done.status === 'succeeded') show(done.result?.report);
```

## Risk plugins

`spectra_risk::RiskPlugin` is versioned. A plugin implements `name` and `compute`, and may override
`version` (its own release, shown by `RiskEngine::plugins()`). `api_version` defaults to
`PLUGIN_API_VERSION` at build time; `RiskEngine::register_plugin` refuses a plugin written against
another version instead of letting it run against an interface it does not know.

```rust
struct WorstLoss;
impl RiskPlugin for WorstLoss {
    fn name(&self) -> &str { "worst_loss" }
    fn version(&self) -> &str { "1.2.0" }
    fn compute(&self, losses: &[f64], _confidence: f64) -> spectra_core::Result<f64> {
        Ok(losses.iter().copied().fold(f64::NEG_INFINITY, f64::max))
    }
}

let mut engine = RiskEngine::new();
engine.register_plugin(Box::new(WorstLoss))?;
let report = engine.calculate_named("worst_loss", &portfolio, &config)?;
```

Bump `PLUGIN_API_VERSION` whenever `RiskPlugin` changes incompatibly. Plugins are compiled in;
loading them at run time (dynamic libraries or WebAssembly) is not built.

## Where the limits are now

| Resource | Limit today | When it bites | Next step |
|---|---|---|---|
| Synchronous engine calls | 32 at once per API process, then 503 | many users running large risk calls interactively | send large runs through jobs (`lib/jobs.ts`) |
| Job throughput | workers × `SPECTRA_WORKER_CONCURRENCY` | queue depth grows (`jobs` with `status='queued'`) | add workers |
| Artifact volume | one shared disk | more than one host, or disk full | S3 |
| Postgres | one instance | sustained heavy writes | managed Postgres with a read replica |
