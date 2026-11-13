# Input & File Security Inventory (M29)

This document provides a comprehensive security inventory of all user-supplied inputs, file ingests, and report/dataset exports across the SpectraLabs platform as required by Phase M29a Task 1.

## 1. User Ingest & File Inputs

| Input Channel | Data Format | Handling Function(s) | Current Security Checks & Limits |
| :--- | :--- | :--- | :--- |
| **Resumable Upload Sessions** | Binary stream / chunks | `spectra_api::sessions_api::create_session`<br>`spectra_api::sessions_api::upload_part` | Max file size check (`upload_max_bytes`, default 100 MiB). Session TTL 24h. |
| **Dataset Upload (CSV)** | Text / CSV | `spectra_api::uploads_api::complete_upload`<br>`spectra_data::security::validate_text_upload`<br>`spectra_data::csv::load_csv` | Size limit: 100 MiB (`DEFAULT_MAX_FILE_SIZE`). UTF-8 validation & BOM strip. NUL byte rejection (`IllegalControlCharacters`). Row cap: 5,000,000. Field length cap: 4,096 chars. Finite float validation (`validate_finite`). |
| **Dataset Upload (Parquet)** | Binary / Parquet | `spectra_api::uploads_api::complete_upload`<br>`spectra_parquet::reader::load_parquet` | Size limit: 100 MiB. Magic-byte verification (`PAR1`). Decompressed row-group size inspected from file metadata before decompression to defeat zip/parquet bombs. |
| **Strategy Source Code** | DSL text / Python / Rust | `spectra_api::strategies_api::create_strategy`<br>`spectra_api::strategies_api::update_strategy` | Max text length: 256 KiB. UTF-8 check via `validate_text_upload`. Sandboxed execution boundary in engine runner. |
| **Portfolio CSV Input** | CSV text | `spectra_api::jobs_api::create_job`<br>`spectra_api::engine::run_registry_backtest` | Text validation via `validate_text_upload`. Max payload: 5 MiB. Parsed row-by-row with strict column and finite float checks. |
| **Formula & Alerts DSL** | Text expression | `spectra_research::formula::parse_formula` | Max formula length: 2,048 chars. AST recursion depth cap (max depth 32) to prevent stack overflow. |
| **Lesson & Course Rich Text** | Markdown / TeX | `web/frontend/components/learn/RichText.tsx:RichText`<br>`web/frontend/components/learn/RichText.tsx:Formula` | Tokenizer restricts tags to bold, code, math (\( \)), and links. No raw HTML tags (`dangerouslySetInnerHTML` replaced with safe AST renderer). |

---

## 2. Exports & File Downloads

| Export Endpoint | Output Format | Generation Function | Security & Formula Injection Protections |
| :--- | :--- | :--- | :--- |
| `GET /api/v1/optimization/sweep/{id}/export` | CSV | `spectra_api::optimization_api::generate_validation_csv` | `escape_csv_cell` applied to string fields (`params`, notes). Size capped at 5 MiB. `Content-Type: text/csv; charset=utf-8`. |
| `GET /api/v1/optimization/sweep/{id}/export` | PDF | `spectra_api::optimization_api::generate_validation_pdf` | Binary PDF generated via `SimplePdf`. Formula injection not applicable. |
| `GET /api/v1/paper/gap/reports/{id}/export` | CSV | `spectra_api::gap_api::gap_csv` | `escape_csv_cell` applied to symbol, alert messages, and note strings. CRLF/LF normalized. |
| `GET /api/v1/paper/gap/reports/{id}/export` | PDF | `spectra_api::gap_api::gap_pdf` | Binary vector PDF rendering. |
| `GET /api/v1/alpha/signal-tests/{id}/export` | CSV | `spectra_research::alpha::SignalReport::to_csv` | `escape_csv_cell` on `signal_name`, universe symbols, and disclaimers. |
| `GET /api/v1/research/screener/export` | CSV | `spectra_research::screener::ScreenerResult::to_csv` | Full formula injection escaping with `escape_csv_cell` on all text/numeric fields. Single quote prefix for triggers (`=`, `+`, `-`, `@`, `\t`, `\r`, `%`). |
| Backtest Run Artifacts | CSV (`equity_curve`, `trades`, `returns`) | `spectra_api::engine::backtest_artifacts`<br>`spectra_worker::backtest_artifacts` | `escape_csv_cell` applied to instrument symbols and trade notes. |
| `GET /api/v1/datasets/versions/{id}/export` | Parquet / CSV | `spectra_api::datasets_api::export_version` | Strict MIME type (`application/vnd.apache.parquet`). `Content-Disposition: attachment; filename="<sanitized>.parquet"`. |
| `GET /api/admin/audit?format=csv` | CSV | `web/frontend/app/api/admin/audit/route.ts` | Formula escaping via `toCsv` / `escapeText`. Single-quote protection on user emails, IPs, actions. |
| Frontend Attribution Waterfall & Instruments | CSV | `web/frontend/lib/csvExport.ts:buildWaterfallCsv`<br>`web/frontend/lib/csvExport.ts:buildInstrumentsCsv` | UTF-8 BOM prefix, CRLF line endings, numbers preserved, formula cells escaped with `'`. |
| Frontend Compare Runs & Tear Sheet Export | CSV | `web/frontend/components/studio/backtests/CompareRuns.tsx`<br>`web/frontend/components/studio/backtests/RunDetail.tsx` | Escaped with `escapeText` / `toCsv`. Safe attachment downloads. |

---

## 3. Standard Download Security Headers

All file download routes return:
- `X-Content-Type-Options: nosniff` (enforced at API root router middleware)
- `Content-Disposition: attachment; filename="<sanitized_filename>"`
- Explicit `Content-Type` matching the exact payload format (e.g., `text/csv; charset=utf-8` or `application/vnd.apache.parquet`)
