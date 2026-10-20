# Datasets (M1 Data Studio)

How market data gets into Spectra and how a backtest finds it. The command-line flow in
[backtests.md](backtests.md) (`spectra-ingest`, then `spectra-db dataset-add`) still works and
gives the same dataset ids; this page covers the API the Studio uses.

Status: ingest settings and the preview route are in place (M1 tasks 2–3). Uploads, the ingest
job and the dataset browser follow in tasks 4–8.

## Rules

- **Exact.** Prices and volumes are parsed from decimal text straight into fixed-point (8
  decimal places). More decimals are refused, never rounded.
- **Explicit.** The person picks the delimiter, the timestamp format, the UTC offset and every
  column. The Studio shows suggestions, but nothing is applied unless the person confirms it.
- **Loud.** A bad row aborts the load unless "skip bad rows" is ticked; skipped rows are counted
  by category in the quality report.
- **Content-addressed.** A dataset's id is a hash of its bars and symbol names. The same rows give
  the same id whatever the file's order, formatting or route (command line or Studio).

## Ingest settings

The JSON form of the engine's CSV options (`crates/spectra-data/src/settings.rs`):

```json
{
  "delimiter": ",",
  "time_format": "iso8601",
  "utc_offset_minutes": 330,
  "columns": {
    "timestamp": "Date", "open": "Open", "high": "High", "low": "Low", "close": "Close",
    "volume": "Volume", "symbol": null
  },
  "default_symbol": "NIFTY",
  "skip_bad_rows": false,
  "quality": { "gap_factor": 5, "outlier_z": 10, "stale_run": 5 }
}
```

| Field | Required | Meaning |
|---|---|---|
| `delimiter` | yes | One punctuation character (not `"`) or a tab. |
| `time_format` | yes | `iso8601`, `epoch_s`, `epoch_ms`, `epoch_us` or `epoch_ns`. Bare numbers are never guessed. |
| `utc_offset_minutes` | yes | Minutes east of UTC for ISO timestamps written without a zone, -720 to 840 (330 is IST, -300 is EST). Timestamps with a zone (`Z`, `+05:30`) ignore it. A fixed offset: daylight saving is not modelled yet (M1b). |
| `columns.timestamp` … `columns.close` | yes | Header names, matched without regard to case. Two fields can't use the same column. |
| `columns.volume` | no | `null` or absent: the file has no volume (volume is zero, and the report warns). |
| `columns.symbol` | no | `null` or absent: a single-symbol file, named by `default_symbol`. |
| `default_symbol` | when there is no symbol column | Not allowed alongside a symbol column. |
| `skip_bad_rows` | no, default `false` | `false` aborts at the first bad row. |
| `quality` | no | Thresholds for the quality checks: a gap is longer than `gap_factor` median intervals; an outlier return is beyond `outlier_z` robust sigma; a stale run is `stale_run` or more frozen bars. |

Columns named in settings are taken exactly: an absent `volume` means "no volume column" even if
the header has one called `Volume`, and a named column missing from the header is an error. (The
command line still matches common aliases such as `date`, `ticker`, `vol`.) Settings that match
what the command line picked produce the same dataset id.

Invalid settings are answered with 422 and every problem at once, each with its field path:

```json
{ "code": "invalid_request",
  "message": "settings.time_format: required: one of iso8601, ...; settings.columns.close: required",
  "details": { "errors": [
    { "field": "settings.time_format", "message": "required: one of iso8601, epoch_s, epoch_ms, epoch_us, epoch_ns" },
    { "field": "settings.columns.close", "message": "required" } ] } }
```

## Preview: `POST /api/v1/datasets/preview`

Editors only. Parses the start of a file with the chosen settings exactly as ingesting would, and
stores nothing.

```json
{ "sample": "Date,Open,High,Low,Close,Volume\n2024-01-02,470.1,472,469,471.5,100\n...",
  "truncated": true,
  "settings": { ... } }
```

- `sample`: the file's first lines as text, at most 1 MB (the dashboard proxy's body limit is
  2 MB). Only the first 200 data rows are parsed.
- `truncated`: `true` when the sample was cut from a longer file; its last, possibly partial,
  line is ignored.
- `settings`: omit them to get only suggestions.

The response always carries `suggestions` (delimiter, the header, columns matched by common
aliases, a likely `time_format`, and whether ISO timestamps carry a zone). With settings it also
carries `preview`:

- `rows`: the parsed rows (prices as exact decimal text, timestamps in UTC).
- `quality`: the sample's quality report. Every bad row in the sample is listed here (category,
  line, message), whatever `skip_bad_rows` says, so the person sees them all at once.
- `would_abort`: with these settings, the error that would stop the ingest (the first bad row
  when bad rows abort), or `null`.

A file that doesn't fit the settings (a column missing from the header, unbalanced quotes) is a
422 with the problem under `details.errors[].field = "sample"`.

## Quality report

Stored with every dataset version (`dataset_versions.quality_report`) and returned by the preview.
It keeps the keys `spectra-db dataset-add` has always written (`rows_total`, `rows_accepted`,
`rows_rejected`, `warnings`, `symbols`, `first_ts_ns`, `last_ts_ns`, `bars_sha256`) and adds
`first_ts`/`last_ts`, `rejected_by_category`, `sample_issues` (first 50), `input_was_sorted`,
`notes`, `symbol_stats` (per symbol: bars, date range, median interval, gaps and the largest gap,
zero-volume bars, outlier returns, stale runs) and the `thresholds` used. The OpenAPI schema is
`QualityReport`.
