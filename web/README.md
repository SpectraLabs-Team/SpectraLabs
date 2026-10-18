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
