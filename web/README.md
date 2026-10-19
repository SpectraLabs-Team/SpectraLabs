# Spectra Labs dashboard

Next.js dashboard for the risk engine. It talks to `spectra-api` (in `crates/spectra-api`), which
replaces the former FastAPI backend with the same routes and JSON.

```bash
# 1. API (from the repository root)
cargo run --release -p spectra-api            # http://127.0.0.1:8000

# 2. Frontend
cd web/frontend
npm install
npm run setup                                 # creates .env.local: AUTH_SECRET, DATABASE_URL
npm run dev                                   # creates the database + tables, then http://localhost:3000
```

`npm run setup` copies `.env.example` to `.env.local` and fills in a random `AUTH_SECRET` and the
local `DATABASE_URL`; add `ADMIN_EMAIL`, `GMAIL_USER` and `GMAIL_APP_PASSWORD` by hand. The server
refuses to start without a valid `AUTH_SECRET` and a `DATABASE_URL`. `.env.local` is git-ignored:
never commit it. If the frontend runs on another origin, start the API with
`SPECTRA_CORS_ORIGINS=https://your.origin`.

Backends shown in the benchmark are `CPU`, `Parallel` (multi-threaded, deterministic) and `CUDA`. This
build has no CUDA backend, so CUDA runs show as a fallback to `Parallel`.

## Database

Accounts and sign-in codes live in Postgres (16 recommended). The connection is `DATABASE_URL` in
`.env.local`. `npm run dev` creates the database if it is missing and applies any new migrations
from `db/migrations/`; `npm start` applies migrations. Pick one way to run Postgres on your machine:

- **Postgres.app** (simplest on macOS): download from postgresapp.com, open it, click Initialize.
  Keep the default `DATABASE_URL=postgresql://localhost:5432/spectralabs`.
- **Homebrew**: `brew install postgresql@16 && brew services start postgresql@16`. Same default URL.
- **Docker**: `docker compose up -d db` from the repository root, then set
  `DATABASE_URL=postgresql://spectra:spectra@localhost:5432/spectralabs`.

| Command | What it does |
| --- | --- |
| `npm run db:migrate` | Apply pending migrations (`-- --status` lists them) |
| `npm run db:import-users` | One-off: copy accounts from the old `.data/users.json` (`-- --dry-run` first) |
| `npm test` | Auth tests, in a throwaway schema of `TEST_DATABASE_URL` (or `DATABASE_URL`) |
| `npm run admin -- list \| grant \| revoke <email>` | Platform admin role (signs that account out everywhere) |
| `npm run secrets:rotate` | Re-wrap stored broker keys under a new `SECRETS_MASTER_KEY` (see `.env.example`) |
| `npm run audit:verify` | Check the audit log's hash chain (exit 1 if any row was altered or removed) |

Passwords are hashed with Argon2id; older PBKDF2 hashes are upgraded at the next sign-in. New
passwords are checked against HaveIBeenPwned (k-anonymity range API; set `HIBP_DISABLED=1` offline).
Rate-limit counters and login lockouts live in Postgres, so restarts don't reset them.

Schema changes go in a new numbered file (`db/migrations/002_….sql`); never edit one that has been
applied.

## Security

Scanning in CI, spike alerts (`ALERT_*` in `.env.example`) and the pre-real-money checklist are in [SECURITY.md](../SECURITY.md).

## Pages

- `/`: landing page. Live engine status, hero cards fed by the engine when it is running, the
  metrics taxonomy (each metric marked Live, Backtest or Roadmap), and the Rust vs NumPy benchmark.
- `/studio`: Risk & Stress Lab, Portfolio Studio, Hardware Benchmarks and Market Data & Alpaca
  Settings. Paper trading is a simulated ledger kept in the browser; live trading stays disabled
  while `/api/alpaca/*` returns 501.

Dark mode is the default; the header and sidebar have a light/dark toggle (stored in `localStorage`).

## Benchmark baseline

The landing-page chart reads `frontend/lib/benchmarkBaseline.json`: Rust (`spectra-risk`) and NumPy
running the same VaR kernel on the same machine. Regenerate it on your own hardware:

```bash
cargo build --release -p spectra-cli
python3 web/bench/bench_baseline.py > web/frontend/lib/benchmarkBaseline.json
```
