//! Data Studio: ingest market data, check its quality, and give it an identity.
//!
//! Principles
//!  * Exact: prices and quantities are parsed from decimal text straight into
//!    fixed-point integers; no floating point is involved. Text with more than
//!    8 significant decimal places is rejected rather than silently rounded.
//!  * Explicit: timestamps must declare their format. A bare number is never
//!    guessed to be seconds or milliseconds, and naive local times use the
//!    configured UTC offset (a fixed offset; DST-aware exchange calendars are
//!    not implemented).
//!  * Loud: by default any invalid row aborts the load. Skipping bad rows is an
//!    opt-in, and every skipped row is counted and reported.
//!  * Content-addressed: a dataset's id is a hash of its bars and symbol table,
//!    so the same data always has the same id regardless of row order or file
//!    formatting.
//!
//! Not done (yet): split/dividend adjustment, exchange calendars, currency
//! conversion, Parquet input.

use std::collections::{BTreeMap, BTreeSet};

use spectra_core::{
    bars_sha256, validate_bars, Bar, Error, InstrumentId, Price, Qty, Result, Sha256, ENGINE_VERSION,
};

use crate::time::{format_timestamp, parse_fixed, parse_timestamp, trim, TimeOptions};

fn data_err(msg: impl Into<String>) -> Error {
    Error::Data(msg.into())
}

/// What to do with a row that cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvalidRowPolicy {
    /// Abort the load at the first invalid row.
    #[default]
    Reject,
    /// Drop the row, count it by category and report it.
    Skip,
}

/// CSV ingestion options and quality thresholds.
#[derive(Debug, Clone, PartialEq)]
pub struct CsvOptions {
    /// Field separator.
    pub delimiter: u8,
    /// Timestamp format and naive offset.
    pub time: TimeOptions,
    /// Invalid-row policy.
    pub on_invalid: InvalidRowPolicy,
    /// Used when the file has no symbol column.
    pub default_symbol: String,
    /// Column names (case-insensitive). Empty means auto-detect from common aliases.
    pub timestamp_column: String,
    /// See `timestamp_column`.
    pub open_column: String,
    /// See `timestamp_column`.
    pub high_column: String,
    /// See `timestamp_column`.
    pub low_column: String,
    /// See `timestamp_column`.
    pub close_column: String,
    /// See `timestamp_column`.
    pub volume_column: String,
    /// See `timestamp_column`.
    pub symbol_column: String,
    /// A gap is an interval longer than `gap_factor` times the median interval.
    pub gap_factor: f64,
    /// Robust z-score (median/MAD) of close-to-close returns that counts as an outlier.
    pub outlier_z: f64,
    /// Consecutive frozen bars that count as a stale run.
    pub stale_run: usize,
    /// When `true`, column names are taken exactly as given: an empty name
    /// means the file has no such column, and nothing is matched by alias.
    /// The Studio's ingest settings set it, so a guessed column is never used
    /// silently; the command line keeps the alias matching (`false`).
    pub explicit_columns: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            time: TimeOptions::default(),
            on_invalid: InvalidRowPolicy::Reject,
            default_symbol: "DEFAULT".into(),
            timestamp_column: String::new(),
            open_column: String::new(),
            high_column: String::new(),
            low_column: String::new(),
            close_column: String::new(),
            volume_column: String::new(),
            symbol_column: String::new(),
            gap_factor: 5.0,
            outlier_z: 10.0,
            stale_run: 5,
            explicit_columns: false,
        }
    }
}

/// One rejected row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowIssue {
    /// 1-based line number in the file.
    pub line: usize,
    /// Rejection category.
    pub category: String,
    /// Human-readable explanation.
    pub message: String,
}

/// Per-symbol quality statistics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolStats {
    /// Symbol name.
    pub symbol: String,
    /// Number of bars.
    pub bars: usize,
    /// First timestamp.
    pub first_ts: i64,
    /// Last timestamp.
    pub last_ts: i64,
    /// Median interval between consecutive bars, nanoseconds.
    pub median_interval_ns: i64,
    /// Intervals longer than `gap_factor` times the median.
    pub gaps: usize,
    /// Longest such gap, nanoseconds.
    pub largest_gap_ns: i64,
    /// Bars with zero volume.
    pub zero_volume_bars: usize,
    /// Close-to-close returns beyond `outlier_z` robust sigma.
    pub outlier_returns: usize,
    /// Runs of `stale_run` or more frozen bars.
    pub stale_runs: usize,
}

/// What ingestion found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataQualityReport {
    /// Data rows read (header excluded).
    pub rows_total: usize,
    /// Rows that became bars.
    pub rows_accepted: usize,
    /// Rows dropped under [`InvalidRowPolicy::Skip`].
    pub rows_rejected: usize,
    /// Rejections by category.
    pub rejected_by_category: BTreeMap<String, usize>,
    /// The first few rejected rows (capped at 50).
    pub sample_issues: Vec<RowIssue>,
    /// Whether rows arrived sorted by (timestamp, symbol).
    pub input_was_sorted: bool,
    /// Per-symbol statistics.
    pub symbols: Vec<SymbolStats>,
    /// Things worth a human look.
    pub warnings: Vec<String>,
    /// Standing limitations of the processing.
    pub notes: Vec<String>,
}

/// Sorted, unique symbol names; a symbol's index is its [`InstrumentId`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolTable {
    /// Symbol names in instrument-id order.
    pub symbols: Vec<String>,
}

impl SymbolTable {
    /// The instrument id of `symbol`.
    pub fn id(&self, symbol: &str) -> Option<InstrumentId> {
        self.symbols.binary_search_by(|s| s.as_str().cmp(symbol)).ok().map(|i| i as InstrumentId)
    }
    /// The symbol of an instrument id.
    pub fn name(&self, id: InstrumentId) -> Option<&str> {
        self.symbols.get(id as usize).map(String::as_str)
    }
}

/// Identity and lineage of a dataset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatasetInfo {
    /// Content hash of bars and symbols.
    pub id: String,
    /// Bars fingerprint (the value recorded in backtest run records).
    pub bars_sha256: String,
    /// Hash of the exact input bytes (empty if not from a file).
    pub source_sha256: String,
    /// Set for derived datasets.
    pub parent_id: String,
    /// Free text, e.g. how a derived dataset was made.
    pub note: String,
    /// Engine version that created the dataset.
    pub engine_version: String,
    /// Number of bars.
    pub bar_count: usize,
    /// First timestamp.
    pub first_ts: i64,
    /// Last timestamp.
    pub last_ts: i64,
}

/// A validated, identified set of bars.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dataset {
    /// Bars sorted by (timestamp, instrument) and validated.
    pub bars: Vec<Bar>,
    /// Symbol table.
    pub symbols: SymbolTable,
    /// Identity and lineage.
    pub info: DatasetInfo,
    /// Quality findings.
    pub quality: DataQualityReport,
}

/// Dataset identity: SHA-256 over a version tag, the bars fingerprint and the
/// symbol table. Identical to the C++ reference, so ids are portable.
pub fn compute_dataset_id(bars: &[Bar], symbols: &SymbolTable) -> String {
    let mut h = Sha256::new();
    h.update_str("quantforge-dataset-v1");
    h.update_str(&bars_sha256(bars));
    h.update_u64(symbols.symbols.len() as u64);
    for s in &symbols.symbols {
        h.update_u64(s.len() as u64);
        h.update_str(s);
    }
    h.hex_digest()
}

/// The element at sorted position `len / 2` (the upper median for even sizes),
/// as `std::nth_element` gives it.
pub(crate) fn upper_median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, f64::total_cmp).1
}

fn standard_notes() -> Vec<String> {
    vec![
        "Prices are used exactly as supplied: no split or dividend adjustment is applied.".into(),
        "No exchange calendar is applied: gaps may be legitimate market closures.".into(),
        "Timestamps are treated as bar start times in UTC.".into(),
    ]
}

/// Quality analysis of any validated bar set (gaps, outliers, stale prices,
/// zero volume).
pub fn assess_quality(bars: &[Bar], symbols: &SymbolTable, cfg: &CsvOptions) -> DataQualityReport {
    let mut rep = DataQualityReport {
        rows_total: bars.len(),
        rows_accepted: bars.len(),
        input_was_sorted: true,
        notes: standard_notes(),
        ..DataQualityReport::default()
    };

    let mut by_symbol: Vec<Vec<usize>> = vec![Vec::new(); symbols.symbols.len()];
    for (i, b) in bars.iter().enumerate() {
        if let Some(v) = by_symbol.get_mut(b.instrument as usize) {
            v.push(i);
        }
    }

    for (s, idx) in by_symbol.iter().enumerate() {
        let mut st =
            SymbolStats { symbol: symbols.symbols[s].clone(), bars: idx.len(), ..SymbolStats::default() };
        if idx.is_empty() {
            rep.symbols.push(st);
            continue;
        }
        st.first_ts = bars[idx[0]].ts_ns;
        st.last_ts = bars[*idx.last().expect("non-empty")].ts_ns;

        let mut deltas = Vec::new();
        let mut returns = Vec::new();
        let mut stale_len = 0usize;
        for (k, &bi) in idx.iter().enumerate() {
            let b = &bars[bi];
            if b.volume.is_zero() {
                st.zero_volume_bars += 1;
            }
            if k == 0 {
                continue;
            }
            let p = &bars[idx[k - 1]];
            deltas.push((b.ts_ns - p.ts_ns) as f64);
            returns.push(b.close.to_f64() / p.close.to_f64() - 1.0);
            let frozen = b.open == b.high && b.high == b.low && b.low == b.close && b.close == p.close;
            stale_len = if frozen { stale_len + 1 } else { 0 };
            if stale_len == cfg.stale_run {
                st.stale_runs += 1;
            }
        }

        let med_interval = upper_median(deltas.clone());
        st.median_interval_ns = med_interval as i64;
        if med_interval > 0.0 {
            for &d in &deltas {
                if d > cfg.gap_factor * med_interval {
                    st.gaps += 1;
                    st.largest_gap_ns = st.largest_gap_ns.max(d as i64);
                }
            }
        }

        if returns.len() >= 20 {
            let med = upper_median(returns.clone());
            let dev: Vec<f64> = returns.iter().map(|r| (r - med).abs()).collect();
            let mad = upper_median(dev);
            if mad > 0.0 {
                st.outlier_returns =
                    returns.iter().filter(|&&r| (r - med).abs() / (1.4826 * mad) > cfg.outlier_z).count();
            }
        }
        rep.symbols.push(st);
    }

    for st in &rep.symbols {
        if st.gaps > 0 {
            rep.warnings.push(format!(
                "{}: {} gap(s) longer than {}x the median interval (largest {} s). Missing bars or market closures?",
                st.symbol,
                st.gaps,
                cfg.gap_factor as i32,
                st.largest_gap_ns / 1_000_000_000
            ));
        }
        if st.outlier_returns > 0 {
            rep.warnings.push(format!(
                "{}: {} close-to-close return(s) beyond {} robust sigma. Bad ticks or unadjusted corporate actions?",
                st.symbol, st.outlier_returns, cfg.outlier_z as i32
            ));
        }
        if st.stale_runs > 0 {
            rep.warnings.push(format!(
                "{}: {} run(s) of {}+ frozen bars (stale prices).",
                st.symbol, st.stale_runs, cfg.stale_run
            ));
        }
        if st.zero_volume_bars > 0 {
            rep.warnings.push(format!("{}: {} bar(s) with zero volume.", st.symbol, st.zero_volume_bars));
        }
    }
    rep
}

// ---------------------------------------------------------------------------
// CSV ingestion
// ---------------------------------------------------------------------------

pub(crate) struct Record {
    pub(crate) line: usize,
    pub(crate) fields: Vec<String>,
}

fn to_string_lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(crate) fn parse_records(s: &[u8], delim: u8) -> Result<Vec<Record>> {
    let mut out = Vec::new();
    let mut cur = Record { line: 0, fields: Vec::new() };
    let mut field: Vec<u8> = Vec::new();
    let mut in_quotes = false;
    let mut quoted_field = false;
    let mut record_open = false;
    let mut line = 1usize;

    fn end_field(cur: &mut Record, field: &mut Vec<u8>, quoted: &mut bool) {
        let text = if *quoted { to_string_lossy(field) } else { to_string_lossy(trim(field)) };
        cur.fields.push(text);
        field.clear();
        *quoted = false;
    }

    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if !record_open {
            cur.line = line;
            record_open = true;
        }
        if in_quotes {
            if c == b'"' {
                if i + 1 < s.len() && s[i + 1] == b'"' {
                    field.push(b'"');
                    i += 1;
                } else {
                    in_quotes = false;
                }
            } else {
                if c == b'\n' {
                    line += 1;
                }
                field.push(c);
            }
        } else if c == b'"' && trim(&field).is_empty() {
            field.clear();
            in_quotes = true;
            quoted_field = true;
        } else if c == delim {
            end_field(&mut cur, &mut field, &mut quoted_field);
        } else if c == b'\n' {
            end_field(&mut cur, &mut field, &mut quoted_field);
            let blank = cur.fields.len() == 1 && cur.fields[0].is_empty();
            let done = std::mem::replace(&mut cur, Record { line: 0, fields: Vec::new() });
            if !blank {
                out.push(done);
            }
            record_open = false;
            line += 1;
        } else if c != b'\r' {
            field.push(c);
        }
        i += 1;
    }
    if in_quotes {
        return Err(data_err(format!("unterminated quoted field starting on line {}", cur.line)));
    }
    if record_open {
        end_field(&mut cur, &mut field, &mut quoted_field);
        let blank = cur.fields.len() == 1 && cur.fields[0].is_empty();
        if !blank {
            out.push(cur);
        }
    }
    Ok(out)
}

/// Header names matched when no column name is given (lower case).
pub(crate) const TIMESTAMP_ALIASES: &[&str] = &["timestamp", "time", "datetime", "date", "ts"];
pub(crate) const OPEN_ALIASES: &[&str] = &["open", "o"];
pub(crate) const HIGH_ALIASES: &[&str] = &["high", "h"];
pub(crate) const LOW_ALIASES: &[&str] = &["low", "l"];
pub(crate) const CLOSE_ALIASES: &[&str] = &["close", "c"];
pub(crate) const VOLUME_ALIASES: &[&str] = &["volume", "vol", "v"];
pub(crate) const SYMBOL_ALIASES: &[&str] = &["symbol", "ticker", "instrument", "sym"];

/// The header normalised the way columns are matched (trimmed, lower case).
pub(crate) fn normalise_header(name: &str) -> String {
    to_string_lossy(trim(name.as_bytes())).to_ascii_lowercase()
}

pub(crate) fn find_column(header: &[String], explicit: &str, aliases: &[&str], exact: bool) -> Option<usize> {
    if !explicit.is_empty() {
        let want = normalise_header(explicit);
        return header.iter().position(|h| *h == want);
    }
    if exact {
        return None;
    }
    aliases.iter().find_map(|a| header.iter().position(|h| h == a))
}

struct Row {
    line: usize,
    symbol: String,
    ts: i64,
    o: i64,
    h: i64,
    l: i64,
    c: i64,
    v: i64,
    naive: bool,
}

struct Rejections<'a> {
    policy: InvalidRowPolicy,
    rep: &'a mut DataQualityReport,
}

impl Rejections<'_> {
    fn reject(&mut self, line: usize, category: &str, msg: String) -> Result<()> {
        if self.policy == InvalidRowPolicy::Reject {
            return Err(data_err(format!("line {line} [{category}]: {msg}")));
        }
        self.rep.rows_rejected += 1;
        *self.rep.rejected_by_category.entry(category.to_string()).or_insert(0) += 1;
        if self.rep.sample_issues.len() < 50 {
            self.rep.sample_issues.push(RowIssue { line, category: category.into(), message: msg });
        }
        Ok(())
    }
}

/// Parses CSV bytes. Fails on structural problems (missing columns, no data)
/// and, under [`InvalidRowPolicy::Reject`], on the first invalid row.
///
/// Rows may arrive in any order; the result is sorted. Duplicates (same
/// timestamp and symbol) are errors under `Reject`. Under `Skip`, exact
/// duplicates are dropped (category "duplicate"); CONFLICTING duplicates keep
/// the first row in the file (category "duplicate_conflict") and raise a
/// warning, because which row survives then depends on file order.
///
/// # Errors
/// [`Error::Data`] as described.
pub fn load_csv(content: &[u8], opt: &CsvOptions) -> Result<Dataset> {
    let source_hash = spectra_core::sha256_hex(content);
    let content = content.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(content);
    let records = parse_records(content, opt.delimiter)?;
    let mut ds = load_records(&records, opt, false)?;
    ds.info.source_sha256 = source_hash;
    Ok(ds)
}

/// What [`preview_csv`] found in the first rows of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct CsvPreview {
    /// The header as read with the chosen delimiter (trimmed, lower case).
    pub header: Vec<String>,
    /// The sampled rows that parsed, as bars (sorted by timestamp, then
    /// symbol), with every rejected row counted in `quality` whatever the
    /// bad-row policy. `bars` may be empty.
    pub dataset: Dataset,
    /// Data rows after the header that were not looked at (`max_rows` reached).
    pub rows_not_read: usize,
    /// Under the chosen policy, the error that would stop ingesting these
    /// rows: the first rejected row when bad rows abort, or "no valid data
    /// rows". `None` means these rows would load.
    pub would_abort: Option<String>,
}

/// Parses the header and at most `max_rows` data rows of `content` with
/// `opt`, exactly as [`load_csv`] would, without stopping at bad rows: every
/// problem in the sample is reported, and [`CsvPreview::would_abort`] says
/// whether [`load_csv`] would refuse them. Nothing is stored.
///
/// # Errors
/// [`Error::Data`] for problems with the file as a whole: unreadable quoting,
/// no header, or a required column missing from the header.
pub fn preview_csv(content: &[u8], opt: &CsvOptions, max_rows: usize) -> Result<CsvPreview> {
    let content = content.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(content);
    let mut records = parse_records(content, opt.delimiter)?;
    let rows_not_read = records.len().saturating_sub(max_rows + 1);
    records.truncate(max_rows + 1);
    let lenient = CsvOptions { on_invalid: InvalidRowPolicy::Skip, ..opt.clone() };
    let dataset = load_records(&records, &lenient, true)?;
    let header =
        records.first().map_or_else(Vec::new, |r| r.fields.iter().map(|f| normalise_header(f)).collect());
    let would_abort = if opt.on_invalid == InvalidRowPolicy::Reject {
        dataset
            .quality
            .sample_issues
            .first()
            .map(|i| format!("line {} [{}]: {}", i.line, i.category, i.message))
    } else {
        None
    }
    .or_else(|| dataset.bars.is_empty().then(|| "no valid data rows".to_string()));
    Ok(CsvPreview { header, dataset, rows_not_read, would_abort })
}

/// [`load_csv`] after the records are parsed. `allow_empty` returns a dataset
/// with no bars (and its quality report) instead of failing when no row is
/// valid.
#[allow(clippy::too_many_lines)] // one linear pass, kept in the C++ reference's order
fn load_records(records: &[Record], opt: &CsvOptions, allow_empty: bool) -> Result<Dataset> {
    let mut ds = Dataset::default();
    if records.is_empty() {
        return Err(data_err("CSV is empty"));
    }
    let header: Vec<String> = records[0].fields.iter().map(|f| normalise_header(f)).collect();
    let exact = opt.explicit_columns;
    let col_ts = find_column(&header, &opt.timestamp_column, TIMESTAMP_ALIASES, exact);
    let col_open = find_column(&header, &opt.open_column, OPEN_ALIASES, exact);
    let col_high = find_column(&header, &opt.high_column, HIGH_ALIASES, exact);
    let col_low = find_column(&header, &opt.low_column, LOW_ALIASES, exact);
    let col_close = find_column(&header, &opt.close_column, CLOSE_ALIASES, exact);
    let col_volume = find_column(&header, &opt.volume_column, VOLUME_ALIASES, exact);
    let col_symbol = find_column(&header, &opt.symbol_column, SYMBOL_ALIASES, exact);
    // A named optional column that is not in the header is a mistake, not "no such column".
    for (name, given, found) in
        [("volume", &opt.volume_column, col_volume), ("symbol", &opt.symbol_column, col_symbol)]
    {
        if exact && !given.is_empty() && found.is_none() {
            return Err(data_err(format!("missing {name} column: {}", given.trim())));
        }
    }
    let require =
        |c: Option<usize>, name: &str| c.ok_or_else(|| data_err(format!("missing required column: {name}")));
    let col_ts = require(col_ts, "timestamp")?;
    let col_open = require(col_open, "open")?;
    let col_high = require(col_high, "high")?;
    let col_low = require(col_low, "low")?;
    let col_close = require(col_close, "close")?;

    let mut rows: Vec<Row> = Vec::new();
    let mut naive_count = 0usize;
    {
        let mut rj = Rejections { policy: opt.on_invalid, rep: &mut ds.quality };
        for rec in &records[1..] {
            rj.rep.rows_total += 1;
            if rec.fields.len() != header.len() {
                rj.reject(
                    rec.line,
                    "column_count",
                    format!("expected {} fields, found {}", header.len(), rec.fields.len()),
                )?;
                continue;
            }
            let field = |c: usize| rec.fields[c].as_str();
            let symbol = col_symbol.map_or_else(|| opt.default_symbol.clone(), |c| field(c).to_string());
            if symbol.is_empty() {
                rj.reject(rec.line, "empty_symbol", "symbol is empty".into())?;
                continue;
            }
            let Some(ts) = parse_timestamp(field(col_ts), &opt.time) else {
                rj.reject(
                    rec.line,
                    "bad_timestamp",
                    format!(
                        "cannot parse timestamp '{}' (formats must be explicit; see TimeFormat)",
                        field(col_ts)
                    ),
                )?;
                continue;
            };
            let mut nums = [0i64; 5];
            let mut ok = true;
            let mut cols =
                vec![(col_open, "open"), (col_high, "high"), (col_low, "low"), (col_close, "close")];
            if let Some(cv) = col_volume {
                cols.push((cv, "volume"));
            }
            for (k, (c, what)) in cols.iter().enumerate() {
                if let Some(v) = parse_fixed(field(*c)) {
                    nums[k] = v;
                } else {
                    rj.reject(
                        rec.line,
                        "bad_number",
                        format!("cannot parse {what} '{}' (max 8 decimal places, no rounding)", field(*c)),
                    )?;
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            let [o, h, l, c, v] = nums;
            if o <= 0 || h <= 0 || l <= 0 || c <= 0 {
                rj.reject(rec.line, "non_positive_price", "prices must be greater than zero".into())?;
                continue;
            }
            if l > o.min(c) || h < o.max(c) {
                rj.reject(rec.line, "ohlc_inconsistent", "low/high do not bracket open and close".into())?;
                continue;
            }
            if v < 0 {
                rj.reject(rec.line, "negative_volume", "volume is negative".into())?;
                continue;
            }
            rows.push(Row { line: rec.line, symbol, ts: ts.ts_ns, o, h, l, c, v, naive: ts.naive });
        }
    }
    if rows.is_empty() {
        if allow_empty {
            return Ok(ds);
        }
        return Err(data_err("no valid data rows"));
    }

    // Symbol table: sorted unique names, so instrument ids do not depend on row order.
    let names: BTreeSet<&str> = rows.iter().map(|r| r.symbol.as_str()).collect();
    ds.symbols.symbols = names.into_iter().map(str::to_string).collect();
    let ids: Vec<InstrumentId> =
        rows.iter().map(|r| ds.symbols.id(&r.symbol).expect("symbol in table")).collect();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    let key = |i: usize| (rows[i].ts, ids[i]);
    ds.quality.input_was_sorted = order.windows(2).all(|w| key(w[0]) <= key(w[1]));
    order.sort_by_key(|&i| key(i)); // stable

    ds.bars.reserve(rows.len());
    let mut prev: Option<usize> = None;
    let mut conflicting = 0usize;
    for &ri in &order {
        let r = &rows[ri];
        if let Some(pi) = prev {
            let p = &rows[pi];
            if p.ts == r.ts && p.symbol == r.symbol {
                let identical = p.o == r.o && p.h == r.h && p.l == r.l && p.c == r.c && p.v == r.v;
                if !identical {
                    conflicting += 1;
                }
                let msg = format!(
                    "{} the {} bar at {} (first seen on line {})",
                    if identical { "exact duplicate of" } else { "CONFLICTS with" },
                    r.symbol,
                    format_timestamp(r.ts),
                    p.line
                );
                let mut rj = Rejections { policy: opt.on_invalid, rep: &mut ds.quality };
                rj.reject(r.line, if identical { "duplicate" } else { "duplicate_conflict" }, msg)?;
                continue; // under Skip: keep the first occurrence; `prev` stays on it
            }
        }
        ds.bars.push(Bar {
            ts_ns: r.ts,
            instrument: ids[ri],
            open: Price::from_raw(r.o),
            high: Price::from_raw(r.h),
            low: Price::from_raw(r.l),
            close: Price::from_raw(r.c),
            volume: Qty::from_raw(r.v),
        });
        if r.naive {
            naive_count += 1; // counted only for rows that became bars
        }
        prev = Some(ri);
    }

    let analysis = assess_quality(&ds.bars, &ds.symbols, opt);
    let rep = &mut ds.quality;
    rep.rows_accepted = ds.bars.len();
    rep.symbols = analysis.symbols;
    rep.warnings = analysis.warnings;
    rep.notes = analysis.notes;
    if rep.rows_rejected > 0 {
        rep.warnings
            .insert(0, format!("{} row(s) were rejected and skipped (see categories).", rep.rows_rejected));
    }
    if conflicting > 0 {
        rep.warnings.push(format!(
            "{conflicting} conflicting duplicate(s): rows with the same timestamp and symbol but different values. \
             The FIRST row in the file was kept, so the dataset depends on file order; resolve the source data \
             instead of relying on this."
        ));
    }
    if !rep.input_was_sorted {
        rep.warnings
            .push("Input rows were not sorted by (timestamp, symbol); they were sorted on load.".into());
    }
    if col_volume.is_none() {
        rep.warnings.push(
            "No volume column: volume was set to zero (participation caps would block all fills).".into(),
        );
    }
    if naive_count > 0 {
        let off = opt.time.naive_utc_offset_minutes;
        rep.warnings.push(format!(
            "{naive_count} timestamp(s) carried no time zone and were read at UTC{}{off} minutes. Fixed offsets \
             cannot model daylight-saving changes.",
            if off >= 0 { "+" } else { "" }
        ));
    }

    ds.info = identify(&ds.bars, &ds.symbols);
    Ok(ds)
}

fn identify(bars: &[Bar], symbols: &SymbolTable) -> DatasetInfo {
    DatasetInfo {
        id: compute_dataset_id(bars, symbols),
        bars_sha256: bars_sha256(bars),
        engine_version: ENGINE_VERSION.into(),
        bar_count: bars.len(),
        first_ts: bars.first().map_or(0, |b| b.ts_ns),
        last_ts: bars.last().map_or(0, |b| b.ts_ns),
        ..DatasetInfo::default()
    }
}

/// [`load_csv`] on a file's bytes.
///
/// # Errors
/// [`Error::Data`] if the file cannot be read or parsed.
pub fn load_csv_file(path: impl AsRef<std::path::Path>, opt: &CsvOptions) -> Result<Dataset> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|_| data_err(format!("cannot open file: {}", path.display())))?;
    load_csv(&bytes, opt)
}

/// Builds a new dataset from transformed bars and records its lineage. Bars
/// must already be valid and sorted; symbols are inherited from `parent`.
///
/// # Errors
/// [`Error::Data`] if the bars are invalid, empty or use unknown instruments.
pub fn derive_dataset(parent: &Dataset, bars: Vec<Bar>, note: impl Into<String>) -> Result<Dataset> {
    if let Some(err) = validate_bars(&bars) {
        return Err(data_err(format!("derived bars are invalid: {err}")));
    }
    if bars.is_empty() {
        return Err(data_err("derived dataset has no bars"));
    }
    let symbols = parent.symbols.clone();
    if bars.iter().any(|b| b.instrument as usize >= symbols.symbols.len()) {
        return Err(data_err("derived bars use an unknown instrument id"));
    }
    let quality = assess_quality(&bars, &symbols, &CsvOptions::default());
    let mut info = identify(&bars, &symbols);
    info.parent_id.clone_from(&parent.info.id);
    info.note = note.into();
    Ok(Dataset { bars, symbols, info, quality })
}

/// Human-readable quality report.
pub fn format_quality_report(d: &Dataset) -> String {
    let q = &d.quality;
    let mut out = format!("Dataset {}\n", d.info.id);
    out += &format!("  bars: {}   symbols: {}\n", d.info.bar_count, d.symbols.symbols.len());
    out +=
        &format!("  range: {} .. {}\n", format_timestamp(d.info.first_ts), format_timestamp(d.info.last_ts));
    if !d.info.parent_id.is_empty() {
        out += &format!("  derived from {}: {}\n", d.info.parent_id, d.info.note);
    }
    out += &format!(
        "  rows: {} read, {} accepted, {} rejected\n",
        q.rows_total, q.rows_accepted, q.rows_rejected
    );
    for (cat, n) in &q.rejected_by_category {
        out += &format!("    rejected {cat}: {n}\n");
    }
    for i in &q.sample_issues {
        out += &format!("    line {} [{}] {}\n", i.line, i.category, i.message);
    }
    out += "  per symbol:\n";
    for s in &q.symbols {
        out += &format!(
            "    {}: {} bars, median interval {} s, gaps {}, outliers {}, stale runs {}, zero-volume {}\n",
            s.symbol,
            s.bars,
            s.median_interval_ns / 1_000_000_000,
            s.gaps,
            s.outlier_returns,
            s.stale_runs,
            s.zero_volume_bars
        );
    }
    if !q.warnings.is_empty() {
        out += "  warnings:\n";
        for w in &q.warnings {
            out += &format!("    - {w}\n");
        }
    }
    out += "  notes:\n";
    for n in &q.notes {
        out += &format!("    - {n}\n");
    }
    out
}
