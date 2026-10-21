# One-command development tasks. Install `just` (https://just.systems), or run the
# commands below by hand. `docker compose up --build` alone starts everything.

set dotenv-load := true
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# List the tasks.
default:
    @just --list

# Everything in containers: Postgres, migrations, API and dashboard on http://localhost:3000.
up:
    docker compose up --build

# Stop the stack (data volumes are kept).
down:
    docker compose down

# Only Postgres, for running the API and dashboard natively (`just api`, `just web`).
db:
    docker compose up -d postgres

# Apply migrations and seed the dev workspace (needs DATABASE_URL and ADMIN_EMAIL).
seed:
    cargo run -p spectra-db -- seed

# Run the background job worker against DATABASE_URL.
worker:
    cargo run --release -p spectra-api --bin spectra-worker

# Import legacy web/frontend/.data/users.json into Postgres.
import-users:
    cd web/frontend && npm run db:import-users

# Preview the legacy-user import without changing the database.
import-users-dry-run:
    cd web/frontend && npm run db:import-users -- --dry-run

# The API, natively, against the database in DATABASE_URL. Run `just worker` in another terminal.
api:
    cargo run --release -p spectra-api

# The dashboard, natively.
web:
    cd web/frontend && npm run dev

# The checks CI runs, for what this branch changed (`just ci --all` for everything). The pre-push
# hook runs this; set DATABASE_URL / TEST_DATABASE_URL to include the database tests.
ci *args:
    scripts/ci-local.sh {{args}}

# Use the repository's git hooks: formatting on commit, `just ci` on push.
hooks:
    scripts/install-hooks.sh

# Everything CI runs for Rust, plus the schema checks when DATABASE_URL is set.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f crates/spectra-db/tests/schema_checks.sql

# Frontend checks.
check-web:
    cd web/frontend && npm run typecheck && npm test

# Everything behind Caddy (TLS) with Prometheus: set SITE_ADDRESS in .env for a real domain.
up-prod:
    docker compose --profile proxy --profile monitoring up -d --build

# Run N workers (they share the queue safely).
scale-workers n="3":
    docker compose up -d --scale worker={{n}}

# Worker artifacts in a local RustFS bucket instead of the volume.
up-s3:
    docker compose -f docker-compose.yml -f deploy/compose.s3.yml --profile s3 up -d --build

# Back up DATABASE_URL to ./backups (BACKUP_KEEP_DAYS, default 7).
backup dir="backups":
    scripts/backup-postgres.sh {{dir}}

# Restore a backup into RESTORE_DATABASE_URL and check it.
restore-check file:
    scripts/restore-postgres.sh {{file}}

# Record every workspace's registry head hash in ./anchors.
anchor dir="anchors":
    scripts/anchor-registry-hash.sh {{dir}}

# Regenerate the dashboard's TypeScript API types from the OpenAPI spec.
api-client:
    scripts/generate-typescript-client.sh

# Open psql on the dev database.
psql:
    docker compose exec postgres psql -U spectra spectra
