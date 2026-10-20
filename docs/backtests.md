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

## 3. Queue the backtest

```http
POST /api/v1/backtests
{
  "dataset_version_id": "<version id>",
  "strategy": "sma_cross",
  "params": { "fast": 10, "slow": 30 },
  "config": { "preset": "default", "initial_cash": 100000 }
}
```

- `GET /api/v1/strategies/builtin` returns everything a form needs: the strategies with their
  parameter schemas (`buy_and_hold`, `equal_weight`, `sma_cross`, `zscore_reversion`), the
  configuration's JSON Schema, and the presets (`default`, `zero_cost`, `conservative_costs`).
- `params` and `config` are optional. A bad request is a 422 listing every problem
  (`params.fast: must be shorter than slow`, `config.slippage_bps: ...`), and so is a dataset
  version outside the workspace.
- The same request also works as a generic job: `POST /api/v1/jobs` with
  `{"kind": "backtest", "payload": {...}}`.

## 4. Follow it

| Route | Role | |
| --- | --- | --- |
| `GET /api/v1/backtests/{id}` | viewer | `status`, `progress` (0 to 1, every heartbeat), then `run_id`, `fingerprints`, `summary`, `artifacts` |
| `POST /api/v1/backtests/{id}/cancel` | editor | stops it at the next bar group |
| `GET /api/v1/backtests/{id}/series?kind=equity\|drawdown` | viewer | the chart series (at most 1,000 points; the maximum drawdown is always kept). 409 until it has succeeded |
| `GET /api/v1/backtests/{id}/artifacts/{kind}` | viewer | a stored file to download: `summary`, `request`, `equity_csv`, `orders_csv`, `fills_csv`, `returns_csv` or `strategies_csv`. 409 until it has succeeded |
| `GET /api/v1/backtests?status=&cursor=&limit=` | viewer | the workspace's backtests, newest first, with `headline` numbers (total return, Sharpe, max drawdown, final equity, fills) once finished |
| `GET /api/v1/datasets` | viewer | the workspace's dataset versions, newest first (for the dataset picker) |

A finished backtest stores `summary`, `equity_series`, `drawdown_series`, `request`, and the CSVs
`equity_csv`, `orders_csv`, `fills_csv`, `returns_csv`, each with its SHA-256. The API reads the
series from `ARTIFACT_DIR` (mounted read-only under docker compose).

## In the Studio

The Studio's **Backtests** tab (`/studio?tab=backtests`) is the same flow without writing JSON:
pick a dataset version and a strategy (or several, with weights), adjust the parameters and the
configuration, and run. It shows the job's progress with a cancel button, then the equity and
drawdown charts, the summary's metrics, exposure, trades, per-instrument and per-strategy tables,
the fingerprints, and the CSVs to download. Past runs are listed beside it. Viewers can look at
runs; starting or cancelling one needs the editor role.

## Checking a job against the CLI

The same request run directly gives the same fingerprints:

```bash
spectra-backtest run request.json --store "$SPECTRA_DATA_DIR" --dataset <checksum>
```

where `request.json` is the payload without `dataset_version_id`.

## Target weights

`equal_weight` is a target-weight strategy: it sets each instrument's share of equity and the
engine's `Rebalancer` turns that into orders at the next open. Its parameters choose when to trade
(`rebalance_every` bars, or `rebalance_every: 0` with a `drift` threshold), how much to invest
(`gross`, a share of the leverage limit), lot rounding (`lot_size`) and the smallest trade
(`min_trade`). Sells go before buys, and no new orders are sent while earlier ones wait to fill.

## Financing

Three optional annual rates in `config`, charged once per bar on the previous bar's books:

| Field | |
| --- | --- |
| `borrow_fee_rate` | on the market value of each short position |
| `cash_interest_rate` | earned on positive cash, including short-sale proceeds |
| `margin_interest_rate` | paid on negative cash (needs `allow_negative_cash`) |

Each charge is a ledger journal entry (`Interest` or `BorrowFee`), so the books still re-derive
from the journal, and the summary's `activity.financing` is the net total. With all three at zero
(the default) nothing is charged and fingerprints match earlier runs.

## Execution

Strategies written in Rust can place, besides market and limit orders:

| Order | Fills |
| --- | --- |
| `ctx.stop(..)` | when the bar trades through the stop (buy: high reaches it; sell: low does), at the stop or a gapped open, plus slippage; a partial fill continues as a market order |
| `ctx.stop_limit(..)` | on the trigger bar at the stop (or gapped open) if within the limit, otherwise rests as a limit order |
| `ctx.order(OrderSpec { tif, .. })` | any of the above with a time in force: `GoodTillCancelled` (default), `Day` (one bar, then `Expired`) or `GoodTillDate(ts)` |

Market impact (off by default) adds `impact_coefficient x volatility x sqrt(qty / bar volume)` to
market and stop fills, capped at 10%; volatility is measured over the `impact_window` bars before
the fill bar. Both are `config` fields.

## Several strategies on one account

Instead of `strategy` and `params`, a request can list up to 10 strategies, each with a share of
the starting cash:

```json
{
  "dataset_version_id": "<version id>",
  "strategies": [
    { "strategy": "equal_weight", "params": { "rebalance_every": 5 }, "weight": 0.5 },
    { "strategy": "zscore_reversion", "weight": 0.3 }
  ],
  "config": { "preset": "default" }
}
```

- Each strategy keeps its own books. It sees its own positions, cash and equity, sizes its orders
  from them, is held to the ledger policy (shorting, negative cash, leverage limit) on them, and
  is financed on them. A strategy cannot spend another strategy's cash: a fill it cannot afford
  on its own books is rejected even when the account could afford it.
- The weights add up to at most 1. Cash left over stays in the account as unallocated cash, and
  earns `cash_interest_rate` if one is set.
- The account is always the sum of the strategies' books. Orders from all strategies fill in
  submission order, the same way as in a single-strategy run, and the strategies are called in
  the order they are listed.
- The output adds:
  - a `strategy` column (the strategy's index in the list) to `orders.csv`;
  - `order_id,strategy` columns to `fills.csv`;
  - a `strategies.csv` artifact (`strategies_csv`) with each strategy's equity, cash and gross
    exposure at every timestamp;
  - a `strategies` section in the summary: per strategy, its allocation, final equity, profit,
    contribution to the account's return, the headline metrics, fills, fees and financing; plus
    the unallocated cash.
- `run.strategy` in the summary is `"multi"`. `strategy_sha256` hashes every strategy's name,
  source hash and weight, and the weights are part of `config_sha256`. Single-strategy runs keep
  their fingerprints.

Strategies written in Rust can do the same with `spectra_engine::run_multi_backtest`.

## Limits

| Variable | Default | |
| --- | --- | --- |
| `SPECTRA_BACKTEST_MAX_BARS` | 20,000,000 | bars per run (all instruments) |
| `SPECTRA_BACKTEST_MAX_INSTRUMENTS` | 1,000 | instruments per run |
| job time limit | 900 s | the engine stops itself 15 s before it |

A run over a limit fails at once with a message saying which limit; it is not retried.
