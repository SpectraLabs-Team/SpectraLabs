# Running a backtest as a job

A backtest runs on `spectra-worker`, never inside an API request. It reads bars from the
content-addressed dataset store (`SPECTRA_DATA_DIR`) and stores its results as artifacts.

## 1. Put a dataset in the store

```bash
cargo run --release -p spectra-cli --bin spectra-ingest -- prices.csv --store "$SPECTRA_DATA_DIR"
# prints the dataset id (a 64-character checksum) and its quality report
```

CSV columns: `timestamp,symbol,open,high,low,close,volume`. Under docker compose, the store is
the `datasets` volume, mounted at `/var/lib/spectra/datasets` in the worker.

## 2. Register it as a dataset version of a workspace

```bash
cargo run -p spectra-db -- dataset-add --workspace dev --name "spy daily" \
    --store "$SPECTRA_DATA_DIR" --id <checksum>
# prints the dataset version id
```

The same checksum registered again under the same name returns the existing version; a new
checksum becomes the next version. A job can only run on versions of its own workspace.

## 3. Queue the job

```http
POST /api/v1/jobs
{
  "kind": "backtest",
  "payload": {
    "dataset_version_id": "<version id>",
    "strategy": "sma_cross",
    "params": { "fast": 10, "slow": 30 },
    "config": { "preset": "default", "initial_cash": 100000 }
  }
}
```

- `strategy`: `buy_and_hold`, `sma_cross` or `zscore_reversion`
  (`spectra-backtest strategies` lists them with their parameters).
- `params` and `config` are optional. `config` is the user form described by
  `spectra-backtest schema`; presets are `default`, `zero_cost` and `conservative_costs`.
- A bad payload is a 422 listing every problem (`params.fast: must be shorter than slow`,
  `config.slippage_bps: ...`), and so is a dataset version outside the workspace.

## 4. Follow it

`GET /api/v1/jobs/{id}` shows `progress` (0 to 1, updated every heartbeat) while it runs.
`POST /api/v1/jobs/{id}/cancel` stops it at the next bar group. When it succeeds, `result` holds:

| Field | |
| --- | --- |
| `run_id` | the `runs` row (kind `backtest`, linked to the dataset version) |
| `fingerprints` | engine version, dataset, config, result and strategy hashes |
| `summary` | metrics, drawdown, VaR/ES, activity and trade statistics |
| `artifacts` | `summary`, `equity_series`, `drawdown_series`, `request`, and the CSVs `equity_csv`, `orders_csv`, `fills_csv`, `returns_csv`, each with its SHA-256 |

## Checking a job against the CLI

The same request run directly gives the same fingerprints:

```bash
spectra-backtest run request.json --store "$SPECTRA_DATA_DIR" --dataset <checksum>
```

where `request.json` is the payload without `dataset_version_id`.

## Limits

| Variable | Default | |
| --- | --- | --- |
| `SPECTRA_BACKTEST_MAX_BARS` | 20,000,000 | bars per run (all instruments) |
| `SPECTRA_BACKTEST_MAX_INSTRUMENTS` | 1,000 | instruments per run |
| job time limit | 900 s | the engine stops itself 15 s before it |

A run over a limit fails at once with a message saying which limit; it is not retried.
