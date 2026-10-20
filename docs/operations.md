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

## Staging and production

1. Copy the template and fill in every `CHANGE-ME` (use different secrets per environment):
   ```bash
   cp deploy/production.env.example .env        # or deploy/staging.env.example
   cp web/frontend/.env.example web/frontend/.env.local   # AUTH_SECRET, SECRETS_MASTER_KEY, mail, DATABASE_URL
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
