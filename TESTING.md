# Testing M20 (platform ops)

How to check that everything M20 added works: automated checks first, then a manual run through
the stack. Commands are for bash (Linux, macOS, WSL or Git Bash on Windows); `just` recipes also
work from PowerShell.

## 1. Automated checks (run these first)

```bash
just check          # cargo fmt --check, clippy -D warnings, cargo test --workspace, schema checks
just check-web      # npm run typecheck && npm test (in web/frontend)
cargo test -p spectra-db --features s3 --lib     # S3 artifact store against an in-memory bucket
```

CI runs all of that plus, against a real Postgres: the queue and schema SQL checks, a backup,
a restore into a second database with integrity checks, a registry anchor, and a check that the
generated TypeScript API types are current. A green CI run on the branch is the main signal.

> First time after pulling M20 steps 12–13: `cargo check` updates `Cargo.lock` (the new optional
> `object_store` dependency). Commit it, or `docker build` fails (it uses `--locked`).

## 2. Bring the stack up

```bash
cp .env.example .env                       # set POSTGRES_PASSWORD, ENGINE_SECRET (openssl rand -base64 48), ADMIN_EMAIL
cp web/frontend/.env.example web/frontend/.env.local   # same ENGINE_SECRET; AUTH_SECRET etc.
docker compose up -d --build
docker compose ps                          # postgres, api, worker, frontend: healthy; migrate: exited 0
```

The `migrate` service seeds an admin and a `dev` workspace when `ADMIN_EMAIL` is set. Keep the
workspace id for the job tests:

```bash
export SPECTRA_WORKSPACE=$(docker compose exec -T postgres psql -U spectra -d spectra -At -c "select id from workspaces where slug='dev'")
```

The engine port is private, so engine calls below run inside the containers. `engine-call` signs a
request the way the dashboard's proxy does; the frontend container already has `ENGINE_URL` and
`ENGINE_SECRET`:

```bash
engine() { docker compose exec -T -e SPECTRA_WORKSPACE frontend node --input-type=module - "$@" < scripts/engine-call.mjs; }
```

## 3. Probes, OpenAPI and signatures (steps 1, 2, 5)

```bash
docker compose exec api curl -fsS localhost:8000/health
#  {"status":"healthy",...}
docker compose exec api curl -fsS localhost:8000/ready
#  {"status":"ready","checks":{"database":"ok","worker":"ok"}}
docker compose stop worker; sleep 35
docker compose exec api curl -fsS localhost:8000/ready
#  ... "worker":"none_alive"  (still 200: the API does not need a worker)
docker compose start worker

docker compose exec api curl -fsS localhost:8000/api/v1/openapi.json | head -c 300
#  {"openapi":"3.1.0","info":{"title":"Spectra Labs engine API",...

docker compose exec api curl -s -o /dev/null -w '%{http_code}\n' localhost:8000/api/v1/jobs
#  401  (unsigned)
engine GET /api/v1/metrics
#  200 and the list of risk metric names (signed)
```

## 4. Jobs and artifacts (steps 6, 7)

```bash
engine POST /api/v1/jobs '{"kind":"risk","payload":{"portfolio_csv":"symbol,quantity,price\nAAPL,10,190\nMSFT,5,410","simulations":20000}}'
#  202, "status":"queued". Copy the id:
JOB=<id>
engine GET /api/v1/jobs/$JOB
#  after a second or two: "status":"succeeded", "progress":1,
#  "result":{"run_id":...,"report":{...},"artifact":{"sha256":"...","path":"sha256/..","size":...},"artifact_error":null}

engine GET '/api/v1/jobs?status=succeeded&limit=5'          # newest first, with next_cursor
engine POST /api/v1/jobs '{"kind":"risk","payload":{}}'       # 422: Portfolio CSV is required (checked before queueing)
engine POST /api/v1/jobs '{"kind":"nope","payload":{}}'       # 422: unknown kind
engine GET /api/v1/jobs/00000000-0000-7000-8000-000000000000  # 404
```

Cancel: queue a large benchmark and cancel it while it runs.

```bash
engine POST /api/v1/jobs '{"kind":"benchmark","payload":{"portfolio_csv":"symbol,quantity,price\nAAPL,10,190","simulations":1000000,"repetitions":3,"warmups":0}}'
engine POST /api/v1/jobs/<id>/cancel     # "cancel_requested":true; a few seconds later "status":"cancelled"
```

The artifact is on the volume under its hash, and re-hashing it gives the same value:

```bash
docker compose exec worker sh -c 'f=$(find /var/lib/spectra/artifacts/sha256 -type f | head -1); echo $f; sha256sum $f'
```

Worker crash recovery: start a long benchmark, then `docker compose kill worker` and
`docker compose start worker`. Within ~45 s the job is requeued and finishes, with `attempts: 2`.

## 5. Users in Postgres (step 4)

Sign up and sign in at http://localhost:3000, then:

```bash
docker compose exec -T postgres psql -U spectra -d spectra -c "select email, role, created_at from users order by created_at desc limit 5"
just import-users-dry-run      # previews importing the old web/frontend/.data/users.json, changes nothing
```

## 6. Metrics and logs (step 11)

```bash
docker compose exec api curl -fsS localhost:8000/metrics | grep -E '^api_(request_duration_seconds_bucket|responses_total)' | head
docker compose exec worker curl -fsS localhost:9100/metrics | grep -E '^jobs_(claimed|completed)_total|^job_duration_seconds_count'
docker compose logs --tail 5 api worker        # JSON lines with level, target, fields (job_id on worker lines)
```

With `docker compose --profile monitoring up -d`, http://127.0.0.1:9090/targets shows `spectra-api`
and one `spectra-worker` target per replica, all UP.

## 7. Deployment (step 12)

```bash
just backup                                     # writes backups/spectra-<time>.dump and .sha256
docker compose exec -T postgres createdb -U spectra spectra_restore_check
RESTORE_DATABASE_URL=postgres://spectra:<POSTGRES_PASSWORD>@127.0.0.1:5432/spectra_restore_check \
  scripts/restore-postgres.sh backups/spectra-*.dump
#  checksum ok ... registry chains intact ... restore: ok
just anchor                                     # anchors/anchors.log gains a line
```

(`just backup` and the restore need `pg_dump`/`psql` 16 on the host and the database reachable on
127.0.0.1:5432: run `just db`, or add a port mapping, or run them in the postgres container.)

Edge proxy: `docker compose --profile proxy up -d`, then `curl -sI http://localhost/` shows
`strict-transport-security`, `x-frame-options: DENY` and no `server` header. With
`SITE_ADDRESS=<your domain>` and DNS pointed at the host, Caddy serves HTTPS with a real certificate.

## 8. Scaling (step 13)

```bash
just scale-workers 3
docker compose exec -T postgres psql -U spectra -d spectra -c "select id, hostname, heartbeat_at from workers"
#  three rows. Queue ten jobs (step 4) and check jobs.worker_id: several workers took them.

just up-s3                                      # MinIO + worker with ARTIFACT_STORAGE=s3
docker compose logs worker | grep artifact_store     # "artifact_store":"s3://spectra-artifacts/"
#  queue a job, then open the MinIO console at http://127.0.0.1:9001: the object is at sha256/<..>/<..>/<hash>

just api-client                                 # regenerates web/frontend/lib/generated/engine-api.{json,ts}
git status web/frontend/lib/generated           # commit them the first time
```

Plugins: `cargo test -p spectra-risk --test risk plugins` (registration, listing with versions,
refusal of a plugin built for another API version).

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `docker build` fails with "lock file needs to be updated" | run `cargo check` once with network access and commit `Cargo.lock` |
| `api` never becomes healthy | `docker compose logs api migrate`; usually a wrong `POSTGRES_PASSWORD` or a failed migration |
| `/ready` says `"worker":"none_alive"` | `docker compose logs worker`; a worker that cannot reach the database exits |
| Job stays `queued` | no worker for that kind (`SPECTRA_WORKER_KINDS`), or the workspace is at `SPECTRA_WORKSPACE_MAX_RUNNING` |
| `engine` calls return 401 | `ENGINE_SECRET` differs between `.env` and `web/frontend/.env.local`, or the clock is off by more than 60 s |
| Job result has `artifact_error` | quota reached (`SPECTRA_ARTIFACT_QUOTA_BYTES`) or the artifact store is unwritable; the report is still in `result.report` |
| `next build` fails: `AUTH_SECRET environment variable is missing` | set `AUTH_SECRET` in `web/frontend/.env.local` |
