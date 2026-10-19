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

# The API, natively, against the database in DATABASE_URL.
api:
    cargo run --release -p spectra-api

# The dashboard, natively.
web:
    cd web/frontend && npm run dev

# Everything CI runs for Rust, plus the schema checks when DATABASE_URL is set.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f crates/spectra-db/tests/schema_checks.sql

# Frontend checks.
check-web:
    cd web/frontend && npm run typecheck && npm test

# Open psql on the dev database.
psql:
    docker compose exec postgres psql -U spectra spectra
