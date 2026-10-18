# Spectra Labs dashboard

Next.js dashboard for the risk engine. It talks to `spectra-api` (in `crates/spectra-api`), which
replaces the former FastAPI backend with the same routes and JSON.

```bash
# 1. API (from the repository root)
cargo run --release -p spectra-api            # http://127.0.0.1:8000

# 2. Frontend
cd web/frontend
cp .env.example .env.local                    # NEXT_PUBLIC_API_BASE_URL=http://localhost:8000
npm install
npm run dev                                   # http://localhost:3000
```

Only the browser-visible API address belongs in `.env.local`; never put provider secrets in frontend
env files. If the frontend runs on another origin, start the API with
`SPECTRA_CORS_ORIGINS=https://your.origin`.

Backends shown in the benchmark are `CPU`, `Parallel` (multi-threaded, deterministic) and `CUDA`. This
build has no CUDA backend, so CUDA runs show as a fallback to `Parallel`.

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
