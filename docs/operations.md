# Operations: deploying and running Spectra (M20 step 12)

Everything runs from `docker-compose.yml`. The default `docker compose up` starts Postgres,
migrations, the engine API, one worker and the dashboard. Two optional profiles add the public edge
and monitoring. `just --list` shows the shortcuts used below.

| Service | Reachable from | Notes |
|---|---|---|
| `frontend` (Next.js) | `127.0.0.1:3000`, or the internet through Caddy | the only public service |
| `api` (`spectra-api`) | compose network only (`api:8000`) | verifies the proxy's HMAC signature on every `/api` call |
| `worker` (`spectra-worker`) | compose network only (`worker:9100` for `/health`, `/metrics`) | scale with `--scale worker=N` |
| `postgres` | compose network only | data in the `pgdata` volume |
| `caddy` (profile `proxy`) | ports 80 and 443 | TLS and security headers in front of `frontend` |
| `prometheus` (profile `monitoring`) | `127.0.0.1:9090` | scrapes the API and every worker |
| `grafana` (profile `monitoring`) | `127.0.0.1:3001` | provisioned overview dashboard; configure `GRAFANA_ADMIN_PASSWORD` in `.env` |

## One database

The dashboard and the engine share one Postgres database (`DATABASE_URL` for every service).

| Owner | Tables | Migrations |
|---|---|---|
| Dashboard | `users`, `workspaces`, `workspace_members`, sessions, invites, API tokens, secrets, `audit_events` | `web/frontend/db/migrations` (tracked in `schema_migrations`) |
| Engine | datasets, strategies, experiments, jobs, runs, artifacts, `registry_events`, `workspace_quotas` | `crates/spectra-db/migrations` (tracked in `_sqlx_migrations`) |

- `spectra-db migrate` (the compose `migrate` service, `just migrate`) applies the dashboard's
  migrations, then the engine's. It records the dashboard's files exactly as `npm run db:migrate`
  does (same table, checksum and advisory lock), so either runner can go first. The engine's first
  migration refuses to run without the dashboard's tables.
- The engine references `users (id)` and `workspaces (id)` and never alters those tables. Its own
  per-workspace settings live in `workspace_quotas`.
- **Deleting a workspace** (a team, or a personal one with its account) deletes everything the
  engine holds for it, including the whole registry chain. A chain can never lose single events:
  registry events and notes refuse any other delete. Export the registry first
  (`GET /api/v1/registry/export` or `spectra-registry export`) and keep the anchored head hashes.
  Artifact bytes are content-addressed and may be shared, so they stay in the store; they no longer
  count against any quota.
- **Deleting a user** keeps the runs and notes they created and clears `created_by` / `author_id`.
  Finished runs and notes accept no other change.

## Staging and production

1. Copy the template and fill in every `CHANGE-ME` (use different secrets per environment):
   ```bash
   cp deploy/production.env.example .env        # or deploy/staging.env.example
   cp web/frontend/.env.example web/frontend/.env.local   # AUTH_SECRET, SECRETS_MASTER_KEY, mail
   # (compose points the dashboard at the shared database; DATABASE_URL in .env.local is not used there)
   ```
   Secrets: `openssl rand -base64 48`. `ENGINE_SECRET` must be the same in both files.
2. Point DNS for `SITE_ADDRESS` at the host and open ports 80 and 443.
3. Start everything:
   ```bash
   just up-prod        # docker compose --profile proxy --profile monitoring up -d --build
   ```
   Caddy obtains and renews the certificate for `SITE_ADDRESS` by itself.
4. Check it:
   ```bash
   docker compose ps                                   # every service healthy
   docker compose exec api curl -fsS localhost:8000/ready
   # {"status":"ready","checks":{"database":"ok","worker":"ok"}}
   ```
   `checks.worker` is `none_alive` when no worker has sent a heartbeat in 30 seconds. That does not
   make the API unready (synchronous routes work without workers), so alert on it separately.

## Accounts and workspaces in the engine database

The dashboard (Next.js) and the engine use separate databases, each with its own `users` and
`workspaces` tables. The dashboard's tables are the source of truth: sign-up, names, members and
roles live there, and roles are enforced by the `/api/engine` proxy (`lib/server/engineRoutes.ts`).

Every engine table points at the engine's own copies (`workspace_id`, `created_by`). So the API
mirrors the caller on every signed request before anything else runs
(`crates/spectra-db/queries/identity_mirror.sql`): if the signed user id or workspace id has no row
yet, one is created **with the same id**. Existing rows are never changed. This also backfills
accounts created before the mirror existed, the first time they call the engine.

- The engine is never sent the user's email or the workspace's name, so mirrored rows carry
  placeholders: user email `u-<id hex>@mirror.invalid` with no usable password (`'!'`), workspace
  slug `ws-<id hex>` and name `Workspace <first 8 hex>`. Don't use them for display; ask the
  dashboard's database.
- Membership is not mirrored (`workspace_members` stays empty for mirrored workspaces): the engine
  scopes data by the signed workspace and never reads it.
- An API token with no user mirrors only the workspace; its jobs have no `created_by`.
- Each API process remembers which pairs it has mirrored, so this costs one idempotent insert per
  new (user, workspace) pair per process.
- The `dev` workspace from `spectra-db seed` is unaffected and is still what the command-line tools
  use.

## Data uploads and storage

- The API writes uploaded files into the artifact store (`ARTIFACT_DIR`, or S3), so its
  `artifacts` volume is mounted read-write. With S3, it stages each upload in the system
  temporary directory before sending it.
- Datasets ingested through the Studio are stored in the artifact store as well. A worker copies
  one into its own `SPECTRA_DATA_DIR` the first time it backtests it, so the `datasets` volume
  is a cache. Workers on different hosts don't need to share it.
- One quota covers a workspace's run artifacts, uploads and dataset files (each distinct file
  counted once): `workspaces.artifact_quota_bytes`, else `SPECTRA_ARTIFACT_QUOTA_BYTES` (set it on
  the API and the workers; default 1 GiB).
- Uploads not ingested within 24 hours are removed by the workers, with their files unless
  something else uses the same bytes.
- Resumable uploads: `SPECTRA_MAX_UPLOAD_BYTES` on the API sets the largest file (default 1 GiB).
  Chunks are staged in `<ARTIFACT_DIR>/staging` (local) or the system temporary directory (S3) and
  the API removes sessions idle for 24 hours. With several API instances, share that directory.
  Ingest needs roughly 1.3× the file's size in memory on the worker (about 1.4 GB for 1 GiB).
- Volumes created by an image older than M1 may have a root-owned `datasets` directory. If a
  worker logs a permission error there, run
  `docker compose run --rm -u root worker chown -R 10001 /var/lib/spectra/datasets`.

## Backups

Nightly, from cron on the host (or any machine that reaches the database):

```bash
0 2 * * * cd /srv/spectralabs && DATABASE_URL=postgres://... BACKUP_KEEP_DAYS=14 scripts/backup-postgres.sh /var/backups/spectra
```

Each run writes `spectra-<UTC time>.dump` (pg_dump custom format) and a `.sha256`, after checking
the archive is readable, and deletes this script's backups older than `BACKUP_KEEP_DAYS`. Copy the
directory off the host (another region or provider): a backup on the same disk is not a backup.

**Restore-test regularly**, on a scratch database, never the live one:

```bash
createdb spectra_restore_check
RESTORE_DATABASE_URL=postgres://.../spectra_restore_check scripts/restore-postgres.sh /var/backups/spectra/spectra-20261020T020000Z.dump
```

The script verifies the checksum, restores, then checks the tables, the applied migrations and
every workspace's registry hash chain, and exits non-zero on any problem. It refuses to run when
`RESTORE_DATABASE_URL` equals `DATABASE_URL`.

To recover production: stop `api`, `worker` and `frontend`, restore into the production database
with the same script, start them again, and check `/ready`.

## Registry anchoring

Each workspace's registry is a hash chain, so its newest hash commits to its whole history. Anchor
the heads daily somewhere the database's administrators cannot rewrite:

```bash
0 3 * * * cd /srv/spectralabs && DATABASE_URL=postgres://... \
  ANCHOR_UPLOAD='aws s3 cp "$0" s3://spectra-anchors/' scripts/anchor-registry-hash.sh /var/lib/spectra/anchors
```

It writes `registry-heads-<time>.txt` (workspace, seq, hash per line) and appends
`<time> <sha256 of that file> <workspace count>` to `anchors.log`. A bucket with object lock, a
separate git repository or a release asset all work. To audit later, compare a past heads file
with the hashes at those sequence numbers in the database.

## Admin accounts

Platform admins are set from the command line, against the database the app uses. From
`web/frontend`, with `DATABASE_URL` pointing at that database:

```bash
npm run admin -- list                      # who is an admin
npm run admin -- grant someone@example.com # make an admin
npm run admin -- revoke someone@example.com
```

A role change signs that account out everywhere, so its next sign-in gets the new role. Granting
admin turns on two-step login (always on for admins). The last admin can't be revoked: grant
someone else first. Each change is recorded in the audit log (`admin.role.granted`,
`admin.role.revoked`). Admins can also grant, remove and disable accounts in the admin console;
that asks them to confirm it's them first, and they can't change their own account there.

**If something looks wrong:** check `npm run admin -- list`, then the audit log (Admin → Audit log,
filter on `admin.`). `npm run audit:verify` checks that nobody has altered the log.

## Activity retention

Daily active-time rows (`user_activity_daily`) are deleted after 400 days, as promised on the
Account page. Policy: [data-retention.md](data-retention.md). Audit records are never deleted.

**How it runs.** Each worker runs the purge every six hours, starting when it boots. A database lock
means only one purge runs at a time, however many workers there are, and running it again just finds
nothing to delete. To pause deletions, set `SPECTRA_ACTIVITY_PURGE=off` on the workers and restart them.

**By hand.** From `web/frontend`, with `DATABASE_URL` set:

```bash
npm run activity:purge -- --dry-run   # count what would be deleted; deletes nothing
npm run activity:purge                # delete rows older than 400 days
```

The script uses the same lock as the workers. If one is purging at that moment, it says so and
exits with code 3; run it again later.

**Before the first production run**, or after changing the retention period: run the dry run against
production, and check the number is what you expect.

**Turning it on.** Check the dry-run count on production (`npm run activity:purge -- --dry-run`). Once confirmed and the retention policy is signed off, set `SPECTRA_ACTIVITY_PURGE=on`, restart the workers, then check the first `activity purge removed` log line.

**Checking it works.** Each run logs `activity purge removed N daily activity row(s)` (or
`activity purge failed: ...` at error level, which goes to Sentry when `SENTRY_DSN` is set). The
worker's `/metrics` has `activity_purge_runs_total{status="ok|busy|error"}`,
`activity_purge_rows_total` and `activity_purge_last_success_timestamp_seconds`.

**If it fails:** read the error in the worker logs or Sentry. Usual causes are the database being
unreachable or the worker's database role lacking DELETE on `user_activity_daily`. Fix the cause; the
next run catches up, since it deletes everything past the cutoff. To run it at once, use the script.

**Deleted by mistake** (for example after setting too short a period): deleted rows can only come
back from a backup. Restore the backup into a scratch database (see Backups), then copy the missing
`user_activity_daily` rows across. Rows older than the backup's retention are gone.

## Monitoring

`just up-prod` includes Prometheus at `http://127.0.0.1:9090` (bound to localhost; use an SSH tunnel
from elsewhere). Useful queries:

| What | PromQL |
|---|---|
| API p95 latency by route | `histogram_quantile(0.95, sum by (le, path) (rate(api_request_duration_seconds_bucket[5m])))` |
| API 5xx rate | `sum(rate(api_responses_total{status=~"5.."}[5m]))` |
| Jobs finished by outcome | `sum by (kind, status) (rate(jobs_completed_total[15m]))` |
| Job duration p95 | `histogram_quantile(0.95, sum by (le, kind) (rate(job_duration_seconds_bucket[30m])))` |
| Workers up | `up{job="spectra-worker"}` |
| Hours since the last activity purge (alert above 24) | `(time() - max(activity_purge_last_success_timestamp_seconds)) / 3600` |
| Failed activity purges | `sum(increase(activity_purge_runs_total{status="error"}[1d]))` |

Logs from `spectra-api` and `spectra-worker` are JSON lines (`docker compose logs -f api worker`);
`RUST_LOG` sets the level.

## Upgrades

```bash
git pull && docker compose --profile proxy --profile monitoring up -d --build
```

The `migrate` service applies new migrations before `api` and `worker` start. Back up first.
# Error reporting

Set `SENTRY_DSN` in the root `.env` to enable error and panic reporting from both `spectra-api` and
`spectra-worker`. The SDK accepts a Sentry DSN or a Sentry-compatible GlitchTip DSN. Leave it empty
to keep errors in the structured JSON logs only. Do not put auth secrets, request bodies, or other
credentials in DSN values or event messages.
