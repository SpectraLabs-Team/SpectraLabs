//! Streaming Parquet reader for dataset ingestion (M1b task 9 part B).
//!
//! Enforces exact-decimal fixed point rules, rejects floating-point types,
//! validates timestamps, and builds a [`Dataset`] with identical ID to CSV.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::types::{
    Decimal128Type, Int16Type, Int32Type, Int64Type, Int8Type, TimestampMicrosecondType,
    TimestampMillisecondType, TimestampNanosecondType, TimestampSecondType, UInt32Type, UInt64Type,
};
use arrow_array::Array;
use arrow_schema::{DataType, TimeUnit};
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::reader::ChunkReader;
use spectra_core::{bars_sha256, Bar, Error, InstrumentId, Price, Qty, Result};
use spectra_data::{
    assess_quality, compute_dataset_id, format_timestamp, parse_fixed, parse_timestamp, CsvOptions,
    CsvPreview, DataQualityReport, Dataset, InvalidRowPolicy, RowIssue, TimeFormat,
};

fn data_err(msg: impl Into<String>) -> Error {
    Error::Data(msg.into())
}

const TIMESTAMP_ALIASES: &[&str] = &["timestamp", "time", "date", "datetime", "ts"];
const OPEN_ALIASES: &[&str] = &["open", "o"];
const HIGH_ALIASES: &[&str] = &["high", "h"];
const LOW_ALIASES: &[&str] = &["low", "l"];
const CLOSE_ALIASES: &[&str] = &["close", "c"];
const VOLUME_ALIASES: &[&str] = &["volume", "vol", "v"];
const SYMBOL_ALIASES: &[&str] = &["symbol", "ticker", "sym", "instrument"];
const KNOWN_AT_ALIASES: &[&str] = &["known_at", "as_of", "publish_time", "published_at"];

fn normalise_header(name: &str) -> String {
    name.trim().to_lowercase()
}

fn find_column(headers: &[String], explicit: &str, aliases: &[&str], exact: bool) -> Option<usize> {
    if !explicit.is_empty() {
        let needle = normalise_header(explicit);
        return headers.iter().position(|h| h == &needle);
    }
    if exact {
        return None;
    }
    for alias in aliases {
        if let Some(pos) = headers.iter().position(|h| h == *alias) {
            return Some(pos);
        }
    }
    None
}

struct Rejections<'a> {
    policy: InvalidRowPolicy,
    rep: &'a mut DataQualityReport,
}

impl Rejections<'_> {
    fn reject(&mut self, line: usize, category: &str, msg: &str) -> Result<()> {
        self.rep.rows_rejected += 1;
        *self.rep.rejected_by_category.entry(category.to_string()).or_insert(0) += 1;
        if self.rep.sample_issues.len() < 10 {
            self.rep.sample_issues.push(RowIssue {
                line,
                category: category.to_string(),
                message: msg.to_string(),
            });
        }
        if self.policy == InvalidRowPolicy::Reject {
            return Err(data_err(format!("line {line} [{category}]: {msg}")));
        }
        Ok(())
    }
}

struct ParsedRow {
    line: usize,
    sym: u32,
    ts: i64,
    known_at: i64,
    o: i64,
    h: i64,
    l: i64,
    c: i64,
    v: i64,
    naive: bool,
}

/// Reads the string value from an Arrow array at index `row`.
fn get_string_value(array: &Arc<dyn Array>, row: usize) -> Option<String> {
    if array.is_null(row) {
        return None;
    }
    match array.data_type() {
        DataType::Utf8 => Some(array.as_string::<i32>().value(row).to_string()),
        DataType::LargeUtf8 => Some(array.as_string::<i64>().value(row).to_string()),
        DataType::Utf8View => Some(array.as_string_view().value(row).to_string()),
        DataType::Dictionary(key_type, _) => match key_type.as_ref() {
            DataType::Int8 => {
                let dict = array.as_dictionary::<Int8Type>();
                let val_array = dict.values().as_string::<i32>();
                let key = dict.keys().value(row) as usize;
                Some(val_array.value(key).to_string())
            }
            DataType::Int16 => {
                let dict = array.as_dictionary::<Int16Type>();
                let val_array = dict.values().as_string::<i32>();
                let key = dict.keys().value(row) as usize;
                Some(val_array.value(key).to_string())
            }
            DataType::Int32 => {
                let dict = array.as_dictionary::<Int32Type>();
                let val_array = dict.values().as_string::<i32>();
                let key = dict.keys().value(row) as usize;
                Some(val_array.value(key).to_string())
            }
            DataType::Int64 => {
                let dict = array.as_dictionary::<Int64Type>();
                let val_array = dict.values().as_string::<i32>();
                let key = dict.keys().value(row) as usize;
                Some(val_array.value(key).to_string())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Parses a timestamp from an Arrow array at `row`.
fn parse_arrow_timestamp(
    array: &Arc<dyn Array>,
    row: usize,
    opt: &CsvOptions,
) -> std::result::Result<(i64, bool), String> {
    if array.is_null(row) {
        return Err("timestamp is null".into());
    }
    match array.data_type() {
        DataType::Timestamp(unit, tz) => {
            let raw: i64 = match unit {
                TimeUnit::Second => array.as_primitive::<TimestampSecondType>().value(row) * 1_000_000_000,
                TimeUnit::Millisecond => {
                    array.as_primitive::<TimestampMillisecondType>().value(row) * 1_000_000
                }
                TimeUnit::Microsecond => array.as_primitive::<TimestampMicrosecondType>().value(row) * 1_000,
                TimeUnit::Nanosecond => array.as_primitive::<TimestampNanosecondType>().value(row),
            };
            let naive = tz.is_none();
            let ts_ns = if naive {
                let offset_min = match opt.time.timezone {
                    Some(tz_spec) => i64::from(tz_spec.offset_minutes_for_utc(raw / 1_000_000_000)),
                    None => i64::from(opt.time.naive_utc_offset_minutes),
                };
                raw - offset_min * 60 * 1_000_000_000
            } else {
                raw
            };
            Ok((ts_ns, naive))
        }
        DataType::Int64 => {
            let val = array.as_primitive::<Int64Type>().value(row);
            let raw = match opt.time.format {
                TimeFormat::EpochSeconds => val.checked_mul(1_000_000_000),
                TimeFormat::EpochMillis => val.checked_mul(1_000_000),
                TimeFormat::EpochMicros => val.checked_mul(1_000),
                TimeFormat::EpochNanos => Some(val),
                TimeFormat::Iso8601 => None,
            };
            match raw {
                Some(ns) => Ok((ns, false)),
                None => {
                    Err(format!("cannot parse timestamp integer '{val}' under format {:?}", opt.time.format))
                }
            }
        }
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            let s = get_string_value(array, row).unwrap_or_default();
            if let Some(parsed) = parse_timestamp(&s, &opt.time) {
                Ok((parsed.ts_ns, parsed.naive))
            } else {
                Err(format!("cannot parse timestamp '{s}' (formats must be explicit; see TimeFormat)"))
            }
        }
        _ => Err(format!("unsupported timestamp column type {:?}", array.data_type())),
    }
}

/// Parses an exact-decimal fixed-point number from an Arrow array at `row`.
fn parse_arrow_number(
    array: &Arc<dyn Array>,
    row: usize,
    col_name: &str,
) -> std::result::Result<i64, String> {
    if array.is_null(row) {
        return Err(format!("cannot parse {col_name} 'null' (nulls are not allowed)"));
    }
    match array.data_type() {
        DataType::Float32 | DataType::Float64 => {
            Err(format!(
                "cannot parse {col_name} (floating point numbers are not allowed: prices and volumes must be exact decimals or integers)"
            ))
        }
        DataType::Decimal128(precision, scale) => {
            let val: i128 = array.as_primitive::<Decimal128Type>().value(row);
            let _ = precision;
            let s = *scale;
            if s > 8 {
                let excess = (s - 8) as u32;
                let divisor = 10i128.pow(excess);
                if val % divisor != 0 {
                    return Err(format!(
                        "cannot parse {col_name} (max 8 decimal places, no rounding)"
                    ));
                }
                let adjusted = val / divisor;
                i64::try_from(adjusted)
                    .map_err(|_| format!("value out of range for {col_name}"))
            } else {
                let multiplier = 10i128.pow((8 - s) as u32);
                let raw = val.checked_mul(multiplier)
                    .ok_or_else(|| format!("value overflow for {col_name}"))?;
                i64::try_from(raw)
                    .map_err(|_| format!("value out of range for {col_name}"))
            }
        }
        DataType::Int64 => {
            let val = array.as_primitive::<Int64Type>().value(row);
            let raw = i128::from(val).checked_mul(100_000_000)
                .ok_or_else(|| format!("value overflow for {col_name}"))?;
            i64::try_from(raw)
                .map_err(|_| format!("value out of range for {col_name}"))
        }
        DataType::Int32 => {
            let val = array.as_primitive::<Int32Type>().value(row);
            let raw = i128::from(val).checked_mul(100_000_000)
                .ok_or_else(|| format!("value overflow for {col_name}"))?;
            i64::try_from(raw)
                .map_err(|_| format!("value out of range for {col_name}"))
        }
        DataType::UInt64 => {
            let val = array.as_primitive::<UInt64Type>().value(row);
            let raw = i128::from(val).checked_mul(100_000_000)
                .ok_or_else(|| format!("value overflow for {col_name}"))?;
            i64::try_from(raw)
                .map_err(|_| format!("value out of range for {col_name}"))
        }
        DataType::UInt32 => {
            let val = array.as_primitive::<UInt32Type>().value(row);
            let raw = i128::from(val).checked_mul(100_000_000)
                .ok_or_else(|| format!("value overflow for {col_name}"))?;
            i64::try_from(raw)
                .map_err(|_| format!("value out of range for {col_name}"))
        }
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => {
            let s = get_string_value(array, row).unwrap_or_default();
            parse_fixed(&s).ok_or_else(|| {
                format!("cannot parse {col_name} '{s}' (max 8 decimal places, no rounding)")
            })
        }
        _ => Err(format!(
            "unsupported column type {:?} for {col_name}",
            array.data_type()
        )),
    }
}

/// Loads a Parquet dataset from any reader implementing [`ChunkReader`].
///
/// Reads in row-groups / record batches to stream large files without loading the
/// entire file into memory at once.
///
/// # Errors
/// Returns an error if the schema is invalid, required columns are missing,
/// or data rows fail validation (under [`InvalidRowPolicy::Reject`]).
pub fn load_parquet<R: ChunkReader + 'static>(
    reader: R,
    opt: &CsvOptions,
    progress: Option<&mut dyn FnMut(u64) -> bool>,
) -> Result<Dataset> {
    load_parquet_internal(reader, opt, false, progress)
}

/// Loads a Parquet dataset from a file on disk.
///
/// # Errors
/// Returns an error if the file cannot be opened or if data validation fails.
pub fn load_parquet_file<P: AsRef<std::path::Path>>(
    path: P,
    opt: &CsvOptions,
    progress: Option<&mut dyn FnMut(u64) -> bool>,
) -> Result<Dataset> {
    let file = std::fs::File::open(path).map_err(|e| data_err(format!("cannot open file: {e}")))?;
    load_parquet(file, opt, progress)
}

/// Lenient report reader that continues past rejected rows to count every problem.
///
/// # Errors
/// Returns an error if the file cannot be opened or if the schema is unparseable.
pub fn report_parquet_file<P: AsRef<std::path::Path>>(path: P, opt: &CsvOptions) -> Result<Dataset> {
    let file = std::fs::File::open(path).map_err(|e| data_err(format!("cannot open file: {e}")))?;
    report_parquet(file, opt)
}

/// Returns the total number of data rows in a Parquet file, if valid.
#[must_use]
pub fn parquet_total_rows<P: AsRef<std::path::Path>>(path: P) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).ok()?;
    Some(builder.metadata().file_metadata().num_rows() as u64)
}

/// Lenient report reader that continues past rejected rows to count every problem.
///
/// # Errors
/// Returns an error if the Parquet file cannot be opened or the schema is unparseable.
pub fn report_parquet<R: ChunkReader + 'static>(reader: R, opt: &CsvOptions) -> Result<Dataset> {
    let lenient = CsvOptions { on_invalid: InvalidRowPolicy::Skip, ..opt.clone() };
    load_parquet_internal(reader, &lenient, true, None)
}

/// Preview the first `max_rows` data rows of a Parquet file.
///
/// # Errors
/// Returns an error if the Parquet file cannot be read or metadata is corrupted.
pub fn preview_parquet(content: &[u8], opt: &CsvOptions, max_rows: usize) -> Result<CsvPreview> {
    let bytes = Bytes::copy_from_slice(content);
    let lenient = CsvOptions { on_invalid: InvalidRowPolicy::Skip, ..opt.clone() };
    let builder = ParquetRecordBatchReaderBuilder::try_new(bytes)
        .map_err(|e| data_err(format!("cannot open Parquet file: {e}")))?;

    let file_meta = builder.metadata().file_metadata();
    let total_rows = file_meta.num_rows() as usize;
    let schema = builder.schema();
    let header: Vec<String> = schema.fields().iter().map(|f| normalise_header(f.name())).collect();

    let ds = load_parquet_internal_batch(builder, &lenient, true, Some(max_rows), None)?;

    let rows_not_read = total_rows.saturating_sub(max_rows);
    let would_abort = if opt.on_invalid == InvalidRowPolicy::Reject {
        ds.quality.sample_issues.first().map(|i| format!("line {} [{}]: {}", i.line, i.category, i.message))
    } else {
        None
    }
    .or_else(|| ds.bars.is_empty().then(|| "no valid data rows".to_string()));

    Ok(CsvPreview { header, dataset: ds, rows_not_read, would_abort })
}

fn load_parquet_internal<R: ChunkReader + 'static>(
    reader: R,
    opt: &CsvOptions,
    allow_empty: bool,
    progress: Option<&mut dyn FnMut(u64) -> bool>,
) -> Result<Dataset> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(reader)
        .map_err(|e| data_err(format!("cannot open Parquet file: {e}")))?;
    load_parquet_internal_batch(builder, opt, allow_empty, None, progress)
}

fn load_parquet_internal_batch<R: ChunkReader + 'static>(
    builder: ParquetRecordBatchReaderBuilder<R>,
    opt: &CsvOptions,
    allow_empty: bool,
    max_rows: Option<usize>,
    mut progress: Option<&mut dyn FnMut(u64) -> bool>,
) -> Result<Dataset> {
    let file_meta = builder.metadata().file_metadata();
    let total_file_rows = file_meta.num_rows();
    let schema = Arc::clone(builder.schema());
    let headers: Vec<String> = schema.fields().iter().map(|f| normalise_header(f.name())).collect();

    let exact = opt.explicit_columns;
    let col_ts = find_column(&headers, &opt.timestamp_column, TIMESTAMP_ALIASES, exact);
    let col_open = find_column(&headers, &opt.open_column, OPEN_ALIASES, exact);
    let col_high = find_column(&headers, &opt.high_column, HIGH_ALIASES, exact);
    let col_low = find_column(&headers, &opt.low_column, LOW_ALIASES, exact);
    let col_close = find_column(&headers, &opt.close_column, CLOSE_ALIASES, exact);
    let col_volume = find_column(&headers, &opt.volume_column, VOLUME_ALIASES, exact);
    let col_symbol = find_column(&headers, &opt.symbol_column, SYMBOL_ALIASES, exact);
    let col_known_at = find_column(&headers, &opt.known_at_column, KNOWN_AT_ALIASES, exact);

    for (name, given, found) in [
        ("volume", &opt.volume_column, col_volume),
        ("symbol", &opt.symbol_column, col_symbol),
        ("known_at", &opt.known_at_column, col_known_at),
    ] {
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

    // Build reader with streaming batch size (8192 rows per batch)
    let reader = builder
        .with_batch_size(8192)
        .build()
        .map_err(|e| data_err(format!("cannot read Parquet record batches: {e}")))?;

    let mut ds = Dataset::default();
    let mut names: Vec<String> = Vec::new();
    let mut index: HashMap<String, u32> = HashMap::new();
    let mut intern = |s: &str| -> u32 {
        if let Some(&i) = index.get(s) {
            return i;
        }
        let i = u32::try_from(names.len()).expect("fewer than 2^32 symbols");
        names.push(s.to_string());
        index.insert(s.to_string(), i);
        i
    };

    let mut rows: Vec<ParsedRow> = Vec::new();
    let mut naive_count = 0usize;
    let mut current_row_index = 0usize;

    {
        let mut rj = Rejections { policy: opt.on_invalid, rep: &mut ds.quality };

        for maybe_batch in reader {
            let batch = maybe_batch.map_err(|e| data_err(format!("Parquet read error: {e}")))?;
            let num_rows = batch.num_rows();
            let ts_arr = batch.column(col_ts);
            let open_arr = batch.column(col_open);
            let high_arr = batch.column(col_high);
            let low_arr = batch.column(col_low);
            let close_arr = batch.column(col_close);
            let vol_arr = col_volume.map(|c| batch.column(c));
            let sym_arr = col_symbol.map(|c| batch.column(c));
            let known_at_arr = col_known_at.map(|c| batch.column(c));

            for r in 0..num_rows {
                if let Some(limit) = max_rows {
                    if current_row_index >= limit {
                        break;
                    }
                }

                current_row_index += 1;
                // Line numbering: header is line 1, first data row is line 2
                let line_num = current_row_index + 1;
                rj.rep.rows_total += 1;

                if current_row_index.is_multiple_of(10_000) {
                    if let Some(ref mut rep) = progress {
                        let frac = if total_file_rows > 0 { current_row_index as u64 } else { 0 };
                        if !rep(frac) {
                            return Err(data_err("cancelled"));
                        }
                    }
                }

                // 1. Symbol
                let sym_str = if let Some(arr) = sym_arr {
                    match get_string_value(arr, r) {
                        Some(s) if !s.trim().is_empty() => s,
                        _ => {
                            rj.reject(line_num, "empty_symbol", "symbol is empty")?;
                            continue;
                        }
                    }
                } else {
                    opt.default_symbol.clone()
                };

                // 2. Timestamp
                let (ts_ns, naive) = match parse_arrow_timestamp(ts_arr, r, opt) {
                    Ok(res) => res,
                    Err(msg) => {
                        rj.reject(line_num, "bad_timestamp", &msg)?;
                        continue;
                    }
                };

                // 3. Known at (point-in-time)
                let known_at_ns = if let Some(arr) = known_at_arr {
                    match parse_arrow_timestamp(arr, r, opt) {
                        Ok((kat, _)) => {
                            if kat < ts_ns {
                                rj.reject(
                                    line_num,
                                    "known_at_before_timestamp",
                                    &format!(
                                        "known_at '{}' cannot be earlier than bar timestamp '{}'",
                                        format_timestamp(kat),
                                        format_timestamp(ts_ns)
                                    ),
                                )?;
                                continue;
                            }
                            kat
                        }
                        Err(msg) => {
                            rj.reject(line_num, "bad_known_at", &msg)?;
                            continue;
                        }
                    }
                } else {
                    ts_ns
                };

                // 4. Prices
                let o = match parse_arrow_number(open_arr, r, "open") {
                    Ok(v) => v,
                    Err(msg) => {
                        rj.reject(line_num, "bad_number", &msg)?;
                        continue;
                    }
                };
                let h = match parse_arrow_number(high_arr, r, "high") {
                    Ok(v) => v,
                    Err(msg) => {
                        rj.reject(line_num, "bad_number", &msg)?;
                        continue;
                    }
                };
                let l = match parse_arrow_number(low_arr, r, "low") {
                    Ok(v) => v,
                    Err(msg) => {
                        rj.reject(line_num, "bad_number", &msg)?;
                        continue;
                    }
                };
                let c = match parse_arrow_number(close_arr, r, "close") {
                    Ok(v) => v,
                    Err(msg) => {
                        rj.reject(line_num, "bad_number", &msg)?;
                        continue;
                    }
                };

                let v = if let Some(arr) = vol_arr {
                    match parse_arrow_number(arr, r, "volume") {
                        Ok(val) => val,
                        Err(msg) => {
                            rj.reject(line_num, "bad_number", &msg)?;
                            continue;
                        }
                    }
                } else {
                    0
                };

                if o <= 0 || h <= 0 || l <= 0 || c <= 0 {
                    rj.reject(line_num, "non_positive_price", "prices must be greater than zero")?;
                    continue;
                }
                if l > o.min(c) || h < o.max(c) {
                    rj.reject(line_num, "ohlc_inconsistent", "low/high do not bracket open and close")?;
                    continue;
                }
                if v < 0 {
                    rj.reject(line_num, "negative_volume", "volume is negative")?;
                    continue;
                }

                let sym = intern(&sym_str);
                rows.push(ParsedRow {
                    line: line_num,
                    sym,
                    ts: ts_ns,
                    known_at: known_at_ns,
                    o,
                    h,
                    l,
                    c,
                    v,
                    naive,
                });
            }

            if let Some(limit) = max_rows {
                if current_row_index >= limit {
                    break;
                }
            }
        }
    }

    if rows.is_empty() {
        if allow_empty {
            return Ok(ds);
        }
        return Err(data_err("no valid data rows"));
    }

    // Symbol table
    let sorted: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    ds.symbols.symbols = sorted.into_iter().map(str::to_string).collect();
    let id_of: Vec<InstrumentId> = names.iter().map(|n| ds.symbols.id(n).expect("symbol in table")).collect();

    let mut order: Vec<usize> = (0..rows.len()).collect();
    let key = |i: usize| (rows[i].ts, id_of[rows[i].sym as usize], rows[i].known_at);
    ds.quality.input_was_sorted = order.windows(2).all(|w| key(w[0]) <= key(w[1]));
    order.sort_by_key(|&i| key(i));

    ds.bars.reserve(rows.len());
    let mut prev: Option<usize> = None;
    let mut conflicting = 0usize;
    for &ri in &order {
        let r = &rows[ri];
        if let Some(pi) = prev {
            let p = &rows[pi];
            if p.ts == r.ts && p.sym == r.sym && p.known_at == r.known_at {
                let identical = p.o == r.o && p.h == r.h && p.l == r.l && p.c == r.c && p.v == r.v;
                if !identical {
                    conflicting += 1;
                }
                let msg = format!(
                    "{} the {} bar at {} (first seen on line {})",
                    if identical { "exact duplicate of" } else { "CONFLICTS with" },
                    names[r.sym as usize],
                    format_timestamp(r.ts),
                    p.line
                );
                let mut rj = Rejections { policy: opt.on_invalid, rep: &mut ds.quality };
                rj.reject(r.line, if identical { "duplicate" } else { "duplicate_conflict" }, &msg)?;
                continue;
            }
        }
        ds.bars.push(Bar {
            ts_ns: r.ts,
            known_at_ns: r.known_at,
            instrument: id_of[r.sym as usize],
            open: Price::from_raw(r.o),
            high: Price::from_raw(r.h),
            low: Price::from_raw(r.l),
            close: Price::from_raw(r.c),
            volume: Qty::from_raw(r.v),
        });
        if r.naive {
            naive_count += 1;
        }
        prev = Some(ri);
    }
    drop(order);
    drop(rows);

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

    ds.info.bar_count = ds.bars.len();
    if let Some(first) = ds.bars.first() {
        ds.info.first_ts = first.ts_ns;
    }
    if let Some(last) = ds.bars.last() {
        ds.info.last_ts = last.ts_ns;
    }
    ds.info.bars_sha256 = bars_sha256(&ds.bars);
    ds.info.id = compute_dataset_id(&ds.bars, &ds.symbols);
    ds.info.engine_version = spectra_core::ENGINE_VERSION.to_string();

    if let Some(ref mut rep) = progress {
        let _ = rep(total_file_rows as u64);
    }

    Ok(ds)
}
