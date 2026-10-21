# Datasets (M1 Data Studio)

How market data gets into Spectra and how a backtest finds it. The command-line flow in
[backtests.md](backtests.md) (`spectra-ingest`, then `spectra-db dataset-add`) still works and
gives the same dataset ids; this page covers the API the Studio uses.

Status: M1 Phase M1a is complete: ingest settings, preview, uploads, the ingest job, the dataset
API and the Studio's **Data** tab (tasks 1–8). From M1b: streaming ingest, resumable chunked uploads
(up to 1 GiB), exchange calendars, DST-aware timezones, corporate actions, backward adjustments, and
dataset derivation lineage (tasks 9–11). Research-grade data (Parquet, point-in-time) is continuing in M1b.

## The flow

1. **Preview** the first rows with the chosen settings (`POST /api/v1/datasets/preview`).
2. **Upload** the whole file, in resumable chunks (see below; `POST /api/engine-upload` takes a
   whole file in one request too). It is stored in the artifact store and counts against the workspace's
   storage quota.
3. **Ingest** it (`POST /api/v1/datasets/ingest`): a queued `ingest` job parses the file on a
   worker, stores the dataset and registers it as the next version of the named dataset.
4. Follow the job with `GET /api/v1/jobs/{id}` (progress is the share of rows read; cancel with
   `POST /api/v1/jobs/{id}/cancel`). When it succeeds, the version is in `GET /api/v1/datasets` and
   can be backtested.

| Step | Who |
|---|---|
| Preview, upload, ingest, rename, delete a version | editors and owners of the workspace |
| List and open datasets, charts, quality reports, follow jobs | viewers too |

## In the Studio: the Data tab

`/studio?tab=data` (the open dataset is in the URL: `&dataset=<id>`).

- **Upload CSV** opens the wizard: pick a file (up to 100 MiB) → choose the delimiter, timestamp
  format, DST-aware timezone or UTC offset, exchange calendar, and columns. Suggestions from the first
  rows sit next to each setting and are applied only when you click them (or "Use all suggestions").
  Then **Preview**: the parsed rows, whether the ingest would stop and why, and the sample's quality
  report. Changing a setting after a preview asks for a new preview. Name the dataset (an existing name
  adds a version), then **Upload and ingest**: upload progress, then the job's progress (rows read),
  with Cancel. A refused file shows every problem; go back, change the settings (for example, skip bad
  rows) and ingest again without re-uploading.
- **The list**: name, latest version (and how many), symbols, date range, rows, size, last updated.
- **A dataset**: rename it and describe it (editors), its version history (rows, range, source
  file, rejected rows, warnings, runs), and for the chosen version its quality report (warnings
  highlighted; rejected rows by category; calendar badges; per-symbol gaps, outlier returns, stale
  runs, zero volume, out-of-session and non-trading day bars), a price chart per symbol, where it came
  from (file, settings, dataset id), and the runs that used it.
- **Backtest this dataset** opens the Backtests tab with that version picked in the form
  (`/studio?tab=backtests&dataset=<version id>`).
- A version used by a run can't be deleted (runs must stay reproducible). Deleting a dataset's
  last version deletes the dataset.

## Dataset API

| Route | Role | |
|---|---|---|
| `GET /api/v1/calendars` | viewer | supported exchange calendars (`XNYS`, `XCME`, `XLON`, `XTKS`, `XNSE`, `24/7`) with regular hours, operating timezone and name, plus supported IANA timezones |
| `GET /api/v1/datasets` | viewer | every version, newest first (the backtest form). With `?view=grouped`, also `datasets`: one entry per dataset with its latest version, symbols, date range, rows and last update. Without it the response is unchanged. |
| `GET /api/v1/datasets/{id}` | viewer | the dataset and its versions (newest first), with each version's run count |
| `PATCH /api/v1/datasets/{id}` | editor | `{name?, description?}`; 409 if another dataset has the name |
| `GET /api/v1/datasets/versions/{id}` | viewer | metadata, the full quality report, its source, derivation lineage (`derived_from_id`, `derivation_kind`, `derivation_params`), and `chartable` |
| `DELETE /api/v1/datasets/versions/{id}` | editor | 409 `in_use` while a run or a queued/running backtest uses it; also removes its file when nothing else uses it, and releases its upload |
| `POST /api/v1/datasets/versions/{id}/derive` | editor | derives a new adjusted dataset version applying a corporate action set |
| `GET /api/v1/datasets/versions/{id}/runs` | viewer | runs that used it, newest first |
| `GET /api/v1/datasets/versions/{id}/bars?symbol=&from=&to=&points=` | viewer | one symbol's bars, reduced to at most `points` (10–5000, default 1000) for a chart: each point groups consecutive bars (first open, highest high, lowest low, last close, summed volume); `t` in ms |
| `GET /api/v1/corporate-actions` | viewer | list corporate action sets for the active workspace |
| `POST /api/v1/corporate-actions` | editor | create a corporate action set and its actions (`split` from:to, `dividend` amount) |
| `GET /api/v1/corporate-actions/{id}` | viewer | get corporate action set detail and all its constituent events |
| `DELETE /api/v1/corporate-actions/{id}` | editor | delete a corporate action set |

Every route is scoped to the caller's workspace: another workspace's ids are a 404. The bars are
read from the artifact store; versions registered from the command line are read from the API's
`SPECTRA_DATA_DIR` when it is mounted (compose mounts the `datasets` volume read-only), and
otherwise answer 409 `not_chartable`. The API keeps the last three charted datasets decoded in
memory.

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
  "utc_offset_minutes": null,
  "timezone": "America/New_York",
  "exchange": "XNYS",
  "columns": {
    "timestamp": "Date", "open": "Open", "high": "High", "low": "Low", "close": "Close",
    "volume": "Volume", "symbol": null
  },
  "default_symbol": "SPY",
  "skip_bad_rows": false,
  "quality": { "gap_factor": 5, "outlier_z": 10, "stale_run": 5 }
}
```

| Field | Required | Meaning |
|---|---|---|
| `delimiter` | yes | One punctuation character (not `"`) or a tab. |
| `time_format` | yes | `iso8601`, `epoch_s`, `epoch_ms`, `epoch_us` or `epoch_ns`. Bare numbers are never guessed. |
| `timezone` | no | DST-aware timezone name (e.g. `America/New_York`, `America/Chicago`, `Europe/London`, `Europe/Berlin`, `Asia/Tokyo`, `Asia/Hong_Kong`, `Asia/Kolkata`, `UTC`). When specified, naive ISO timestamps without zone offsets are converted using local daylight saving rules. Takes precedence over `utc_offset_minutes`. |
| `utc_offset_minutes` | if no `timezone` | Minutes east of UTC for ISO timestamps written without a zone (-720 to 840; 330 is IST, -300 is EST). Timestamps with a zone (`Z`, `+05:30`) ignore it. |
| `exchange` | no | Exchange calendar code (`XNYS`, `XCME`, `XLON`, `XTKS`, `XNSE`, `24/7`). When set, bars falling outside session hours or on market holidays are flagged, and gap detection skips weekends/holidays and overnight closures. Daily bars are judged by their trading date (a bar stamped at 18:00 local or later counts for the next day, so US bars stamped at 00:00 UTC map to the right session); weekly or longer bars are not judged. |
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
  2 MB). Every row of the sample is checked; the first 200 parsed rows are sent back for display.
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

## Resumable upload (what the Studio uses)

Files up to `SPECTRA_MAX_UPLOAD_BYTES` (default **1 GiB**; `GET /api/v1/uploads/limits` reports
it) go up in chunks, so an interrupted upload continues where it stopped. Editors only, from a
signed-in session.

1. `POST /api/v1/uploads/sessions` `{filename, size}` opens a session: `{id, received: 0,
   chunk_bytes, expires_at}`. The size counts against the storage quota from the start (413
   `quota_exceeded`); a workspace may have 5 unfinished sessions (409 `too_many_uploads`).
2. Each chunk (at most 8 MiB) goes to the dashboard's `PUT /api/engine-upload/sessions/{id}?offset=N`,
   which signs it and forwards it to the engine's `PUT /api/v1/uploads/sessions/{id}/chunk`. `N`
   must be exactly the bytes received so far; otherwise the answer is 409 `offset_mismatch` with
   `details.received`, which is where to continue. So a chunk whose answer was lost can simply be
   sent again. Chunks of one session are written one at a time (the session row is locked).
3. `GET /api/v1/uploads/sessions/{id}` says how far it got. The Studio remembers the session per
   file (name, size, modification time) in the browser, so choosing the same file again after a
   reload or a dropped connection resumes it.
4. `POST /api/v1/uploads/sessions/{id}/complete` hashes the received file, moves it into the
   artifact store and records the upload, exactly as the single upload below; 409 `incomplete`
   if bytes are missing. Completing again returns the same upload (200), so a lost answer is safe
   to retry.
5. `DELETE /api/v1/uploads/sessions/{id}` abandons it. Sessions idle for 24 hours are removed by the
   API, with their staging files.

The Studio's client retries a failed chunk up to 6 times with backoff (network errors, 429 and
5xx) and stops at once on anything else (quota, size, a missing session).

The staging files live in the artifact store's `staging/` directory (local backend) or the system
temporary directory (S3). With more than one API instance, that directory has to be shared, or a
session's chunks routed to one instance.

## Single upload: `POST /api/engine-upload?filename=prices.csv`

Still served, for scripts and smaller files. The browser sends the raw file as the body (not a
form), at most **100 MiB**. Editors only, from a signed-in session; API tokens can't upload yet.

The dashboard route writes the file to a private temporary file while hashing it (it is never held
in memory), then streams it to the engine's `POST /api/v1/uploads` with the usual signed headers
plus `X-Spectra-Content-SHA256`. The signature covers that hash and the file name (in the query
string). The engine checks the signature before reading the body, writes the body to a staging file
while hashing it, and refuses it unless size and hash match. It then moves the file into the
artifact store (a rename on the local volume).

Response `201`: `{ id, sha256, size, filename, expires_at }`. Errors: `413` over 100 MiB, or
`413` with code `quota_exceeded` when the workspace's storage is full (uploads, dataset files and
run artifacts share one quota: `workspaces.artifact_quota_bytes`, else
`SPECTRA_ARTIFACT_QUOTA_BYTES`, default 1 GiB; each distinct file counts once).

Uploads that are not ingested within **24 hours** are removed by the worker (every 10 minutes),
together with their file unless something else uses the same bytes. An upload that became a
dataset version is kept as that version's source.

Both sides write an audit entry: the dashboard's log (`dataset.uploaded`, with who, file name,
size and hash) and the engine's `audit_events` (`upload.created`, then
`dataset.version_created` or `dataset.version_unchanged`).

> `middleware.ts` skips this one path: Next.js buffers request bodies for middleware and cuts them
> at 10 MB. The route runs the same cross-site check the middleware does for every other `/api`
> route.

## Ingest: `POST /api/v1/datasets/ingest`

```json
{ "upload_id": "0192…", "dataset_name": "nifty daily", "settings": { ... } }
```

Every problem is reported at once (`details.errors`): the name (1–200 characters), each settings
field, and the upload (missing, expired, or another workspace's). A valid request queues an
`ingest` job and answers `202` with the job. (`POST /api/v1/jobs` refuses `kind: "ingest"`, so
the upload check can't be skipped.)

The worker:

1. reads the upload from the artifact store (re-verified by its SHA-256);
2. parses it with exactly the command line's rules, reporting progress every 10,000 rows and
   stopping on cancel;
3. stores the dataset's `bars.qfb` in the artifact store (so any worker can read it; recorded as
   `dataset_versions.blob_sha256`) and in its local dataset store;
4. registers it: new content becomes the next version of the dataset, **the same content is a
   no-op** (the job's result says `"existed": true`). The version records its `source`: the
   upload, the file's hash and name, and the settings, in canonical form;
5. keeps the upload as the source and writes the audit log.

The job's result: `dataset_version_id`, `version`, `existed`, `checksum` (the dataset id),
`rows`, `symbols` and the full `quality_report`.

**A refused file is a failed job, not a crash.** If a bad row stops the load (bad rows abort
unless `skip_bad_rows`), there are no valid rows, or a column is missing, the job fails without
retries. Its `error` names the first problem, and its `result` holds `rejected: true` and the
file's whole quality report, with every bad row counted by category (the file is read a second
time, without stopping, to collect them).

Before a backtest, a worker that doesn't have the version in its local store copies it from the
artifact store, verifying the file's hash and the dataset id.

## Same data, same id

The dataset id is a hash of the bars and symbol names only, so the same CSV through the command
line (`spectra-ingest`) and through an upload gives the same id when the settings read it the same
way (tested in `crates/spectra-api/tests/ingest_flow.rs`). The id doesn't depend on the file
name, row order, formatting or the upload. Settings that read it differently (another offset, no
volume column) give a different id.

## Memory

Ingest reads the file as a stream: one CSV record is held at a time, rows are kept as compact
numbers (each symbol name is stored once), and the file is never loaded whole, from either the
local volume or S3 (downloaded to a staging file in 8 MiB ranges first). Measured on a 100 MB
CSV of 2.19 M bars and 200 symbols: **269 MB** peak on the worker, against 1.48 GB before
(the same dataset id, slightly faster). As a rule of thumb, ingest needs about 1.3× the CSV's
size, roughly 120 bytes per bar, so a 1 GiB file needs about 1.4 GB on the worker.

The streaming parser gives byte-for-byte the same result as the old one, including which error a
refused file gets: on an error it reads on to the end, because an unterminated quote anywhere in
the file still takes precedence, as before. This was checked against the previous implementation
on 80,000 generated files (`tests` in `crates/spectra-data/src/records.rs` keep the old tokenizer
as a reference), and the C++ parity fixtures pass.

Integrity: every byte read for an ingest is hashed and the whole file is read whatever the parser
made of it, so a stored upload that no longer matches its SHA-256 fails the job as corrupt (never
as "bad rows").

## Corporate actions and backward adjustments (M1b Task 11)

Raw historical market data reflects nominal trade prices. When a stock undergoes a stock split
(e.g., a 2:1 or 4:1 split) or distributes cash dividends, unadjusted historical prices exhibit
artificial discontinuities ("price cliffs") that corrupt moving averages, volatility estimators,
and momentum indicators.

Spectra provides research-grade **backward adjustment** for historical datasets:

- **Exact rational scaling:** adjustments are computed backward in time using 128-bit exact
  rational multipliers (`num / den`) reduced via greatest common divisor (GCD).
  - **Stock Splits:** A split from $A$ to $B$ (e.g., 1 to 2) multiplies past prices by $A/B$ and
    scales past trading volume inversely by $B/A$.
  - **Cash Dividends:** A cash dividend $D$ paid with prior close $P_{\text{prior}}$ scales past
    prices by $(P_{\text{prior}} - D) / P_{\text{prior}}$. Volumes are unchanged.
- **Invariant preservation:** OHLC bounds are strictly maintained across integer fixed-point
  round-off: $low \le \min(open, close)$ and $high \ge \max(open, close)$ for all adjusted bars.
- **Derived version lineage:** Deriving an adjusted version leaves the original unadjusted dataset
  intact and records full cryptographic and operational lineage:
  - `derived_from_id`: the UUID of the parent dataset version.
  - `derivation_kind`: `"adjusted"`.
  - `derivation_params`: `{ corporate_action_set_id, adjust_splits, adjust_dividends }`.
  - The derived version is fully queryable, chartable, backtestable, and has its own quality report.

Corporate actions are grouped into reusable **corporate action sets** (`corporate_action_sets` and
`corporate_actions`), manageable via `GET/POST/DELETE /api/v1/corporate-actions` or directly in the
Studio Data tab.

## Limits for now

- Backtests still load the whole dataset into memory: the engine takes every bar as one list.
  Streaming bars through a backtest is M3 task 14.
- Fixed UTC offsets only (no daylight saving); exchange calendars are M1b (task 10).
