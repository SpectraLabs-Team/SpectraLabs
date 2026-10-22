//! Ingest settings as data (M1 task 2): the JSON form of [`CsvOptions`] that
//! the Studio sends, its validation, and suggestions for filling it in.
//!
//! ```json
//! {
//!   "delimiter": ",",
//!   "time_format": "iso8601",
//!   "utc_offset_minutes": 0,
//!   "columns": { "timestamp": "date", "open": "open", "high": "high", "low": "low",
//!                "close": "close", "volume": "volume", "symbol": null },
//!   "default_symbol": "SPY",
//!   "skip_bad_rows": false,
//!   "quality": { "gap_factor": 5, "outlier_z": 10, "stale_run": 5 }
//! }
//! ```
//!
//! The rules stay "explicit, loud": the delimiter, the timestamp format, the
//! UTC offset and the five price columns must be given. Columns are matched
//! by name exactly ([`CsvOptions::explicit_columns`]); an absent or `null`
//! `volume` or `symbol` means the file has none. A file with no symbol column
//! needs `default_symbol`. Bad rows abort the load unless `skip_bad_rows` is
//! `true`. `quality` is optional and defaults to the command line's values.
//!
//! [`suggest`] looks at the start of a file and proposes a delimiter, column
//! mapping and timestamp format. They are suggestions for the person to
//! confirm; nothing here applies them.
//!
//! The offset is a fixed number of minutes east of UTC. Daylight saving is not
//! modelled (exchange calendars are M1b).

use spectra_core::Json;

use crate::calendar::Exchange;
use crate::dataset::{
    find_column, normalise_header, CsvOptions, InvalidRowPolicy, CLOSE_ALIASES, HIGH_ALIASES,
    KNOWN_AT_ALIASES, LOW_ALIASES, OPEN_ALIASES, SYMBOL_ALIASES, TIMESTAMP_ALIASES, VOLUME_ALIASES,
};
use crate::records::{parse_records, Record};
use crate::time::{parse_timestamp, TimeFormat, TimeOptions};
use crate::timezone::Timezone;

/// One problem with a settings document. `field` is a dotted path
/// (`columns.close`, `quality.gap_factor`); empty for the document itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError {
    /// Dotted path of the offending field.
    pub field: String,
    /// What is wrong, for a person.
    pub message: String,
}

impl SettingsError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self { field: field.into(), message: message.into() }
    }
}

/// Fields of the settings document.
const FIELDS: &[&str] = &[
    "delimiter",
    "time_format",
    "utc_offset_minutes",
    "timezone",
    "exchange",
    "columns",
    "default_symbol",
    "skip_bad_rows",
    "quality",
];
/// Keys of `columns`: the five required ones, then the two optional ones.
const COLUMNS: &[&str] = &["timestamp", "open", "high", "low", "close", "volume", "symbol"];
const QUALITY: &[&str] = &["gap_factor", "outlier_z", "stale_run"];

/// Fixed offsets in use: UTC-12:00 to UTC+14:00.
pub const MIN_UTC_OFFSET_MINUTES: i64 = -12 * 60;
/// See [`MIN_UTC_OFFSET_MINUTES`].
pub const MAX_UTC_OFFSET_MINUTES: i64 = 14 * 60;
/// Longest default symbol.
pub const MAX_SYMBOL_LEN: usize = 64;

/// The names `time_format` accepts, in [`TimeFormat`] order.
pub const TIME_FORMATS: &[(&str, TimeFormat)] = &[
    ("iso8601", TimeFormat::Iso8601),
    ("epoch_s", TimeFormat::EpochSeconds),
    ("epoch_ms", TimeFormat::EpochMillis),
    ("epoch_us", TimeFormat::EpochMicros),
    ("epoch_ns", TimeFormat::EpochNanos),
];

fn time_format_name(f: TimeFormat) -> &'static str {
    TIME_FORMATS.iter().find(|(_, t)| *t == f).map_or("iso8601", |(n, _)| n)
}

/// Delimiters a settings document may name. Quotes, letters, digits and line
/// breaks would make the file ambiguous.
fn valid_delimiter(c: char) -> bool {
    c.is_ascii() && (c == '\t' || (c.is_ascii_punctuation() && c != '"'))
}

/// Parses a settings document into [`CsvOptions`] (with
/// `explicit_columns = true`), reporting every problem at once.
///
/// # Errors
/// One [`SettingsError`] per problem, in field order.
#[allow(clippy::too_many_lines)] // one check per field, in document order
pub fn parse_settings(doc: &Json) -> Result<CsvOptions, Vec<SettingsError>> {
    let mut errors = Vec::new();
    let Ok(entries) = doc.entries() else {
        return Err(vec![SettingsError::new("", "settings must be a JSON object")]);
    };
    for (k, _) in entries {
        if !FIELDS.contains(&k.as_str()) {
            errors.push(SettingsError::new(k.clone(), "unknown field"));
        }
    }
    let present = |k: &str| doc.find(k).filter(|v| !v.is_null());
    let mut opt = CsvOptions { explicit_columns: true, ..CsvOptions::default() };

    match present("delimiter").map(Json::as_str) {
        None => errors.push(SettingsError::new("delimiter", "required (for example \",\")")),
        Some(Ok(d)) => {
            let mut chars = d.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if valid_delimiter(c) => opt.delimiter = c as u8,
                _ => errors.push(SettingsError::new(
                    "delimiter",
                    "must be one punctuation character or a tab (not a quote)",
                )),
            }
        }
        Some(Err(_)) => errors.push(SettingsError::new("delimiter", "must be a string")),
    }

    let names = || TIME_FORMATS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    match present("time_format").map(Json::as_str) {
        None => errors.push(SettingsError::new("time_format", format!("required: one of {}", names()))),
        Some(Ok(f)) => match TIME_FORMATS.iter().find(|(n, _)| *n == f) {
            Some((_, t)) => opt.time.format = *t,
            None => errors.push(SettingsError::new("time_format", format!("must be one of {}", names()))),
        },
        Some(Err(_)) => errors.push(SettingsError::new("time_format", "must be a string")),
    }

    let has_tz = present("timezone").is_some();
    let has_offset = present("utc_offset_minutes").is_some();

    if !has_tz && !has_offset {
        errors.push(SettingsError::new(
            "utc_offset_minutes",
            "required: minutes east of UTC for timestamps without a zone (0 for UTC, 330 for IST, -300 for EST) or a timezone (e.g. \"America/New_York\")",
        ));
    }

    if let Some(tz_doc) = present("timezone") {
        match tz_doc.as_str() {
            Ok(s) => match Timezone::parse(s) {
                Some(tz) => {
                    opt.time.timezone = Some(tz);
                    if !has_offset {
                        opt.time.naive_utc_offset_minutes = tz.offset_minutes(2024, 1, 1, 12, 0);
                    }
                }
                None => errors.push(SettingsError::new(
                    "timezone",
                    "unknown timezone; use a standard IANA timezone (e.g. \"America/New_York\", \"Europe/London\", \"UTC\", \"Asia/Tokyo\") or fixed offset like \"+05:30\"",
                )),
            },
            Err(_) => errors.push(SettingsError::new("timezone", "must be a string")),
        }
    }

    if let Some(off_doc) = present("utc_offset_minutes") {
        match off_doc.as_int() {
            Ok(m) if (MIN_UTC_OFFSET_MINUTES..=MAX_UTC_OFFSET_MINUTES).contains(&m) => {
                opt.time.naive_utc_offset_minutes = i32::try_from(m).unwrap_or(0);
            }
            _ => errors.push(SettingsError::new(
                "utc_offset_minutes",
                format!("must be a whole number from {MIN_UTC_OFFSET_MINUTES} to {MAX_UTC_OFFSET_MINUTES}"),
            )),
        }
    }

    if let Some(ex_doc) = present("exchange") {
        match ex_doc.as_str() {
            Ok(s) => match Exchange::parse(s) {
                Some(ex) => opt.exchange = Some(ex),
                None => errors.push(SettingsError::new(
                    "exchange",
                    "unknown exchange; supported: XNYS (NYSE/NASDAQ), XCME (CME), XLON (LSE), XTKS (TSE), XNSE (NSE), 24/7 (Crypto)",
                )),
            },
            Err(_) => errors.push(SettingsError::new("exchange", "must be a string")),
        }
    }

    let mut has_symbol_column = false;
    match present("columns") {
        None => errors.push(SettingsError::new("columns", "required: which header names hold each field")),
        Some(cols) => match cols.entries() {
            Err(_) => errors.push(SettingsError::new("columns", "must be an object")),
            Ok(entries) => {
                for (k, _) in entries {
                    if !COLUMNS.contains(&k.as_str()) {
                        errors.push(SettingsError::new(format!("columns.{k}"), "unknown column"));
                    }
                }
                let mut seen: Vec<(String, &str)> = Vec::new();
                for (i, &key) in COLUMNS.iter().enumerate() {
                    let required = i < 5;
                    let field = format!("columns.{key}");
                    let name = match cols.find(key).filter(|v| !v.is_null()).map(Json::as_str) {
                        None if required => {
                            errors.push(SettingsError::new(field, "required"));
                            continue;
                        }
                        None => continue,
                        Some(Err(_)) => {
                            errors.push(SettingsError::new(field, "must be a string or null"));
                            continue;
                        }
                        Some(Ok(n)) => n.trim(),
                    };
                    if name.is_empty() {
                        let msg =
                            if required { "required" } else { "use null when the file has no such column" };
                        errors.push(SettingsError::new(field, msg));
                        continue;
                    }
                    let norm = normalise_header(name);
                    if let Some((_, other)) = seen.iter().find(|(n, _)| *n == norm) {
                        errors.push(SettingsError::new(field, format!("same column as columns.{other}")));
                        continue;
                    }
                    seen.push((norm, key));
                    let target = match key {
                        "timestamp" => &mut opt.timestamp_column,
                        "open" => &mut opt.open_column,
                        "high" => &mut opt.high_column,
                        "low" => &mut opt.low_column,
                        "close" => &mut opt.close_column,
                        "volume" => &mut opt.volume_column,
                        _ => {
                            has_symbol_column = true;
                            &mut opt.symbol_column
                        }
                    };
                    name.clone_into(target);
                }
            }
        },
    }

    match present("default_symbol").map(Json::as_str) {
        None if !has_symbol_column && present("columns").is_some_and(|c| c.entries().is_ok()) => {
            errors.push(SettingsError::new(
                "default_symbol",
                "required when the file has no symbol column (columns.symbol is null)",
            ));
        }
        None => {}
        Some(Ok(_)) if has_symbol_column => errors.push(SettingsError::new(
            "default_symbol",
            "only used when the file has no symbol column; remove it or set columns.symbol to null",
        )),
        Some(Ok(s)) => {
            let s = s.trim();
            if s.is_empty() || s.len() > MAX_SYMBOL_LEN || s.chars().any(char::is_control) {
                errors.push(SettingsError::new(
                    "default_symbol",
                    format!("must be 1 to {MAX_SYMBOL_LEN} characters"),
                ));
            } else {
                s.clone_into(&mut opt.default_symbol);
            }
        }
        Some(Err(_)) => errors.push(SettingsError::new("default_symbol", "must be a string")),
    }

    match present("skip_bad_rows").map(Json::as_bool) {
        None | Some(Ok(false)) => opt.on_invalid = InvalidRowPolicy::Reject,
        Some(Ok(true)) => opt.on_invalid = InvalidRowPolicy::Skip,
        Some(Err(_)) => errors.push(SettingsError::new("skip_bad_rows", "must be true or false")),
    }

    if let Some(q) = present("quality") {
        match q.entries() {
            Err(_) => errors.push(SettingsError::new("quality", "must be an object")),
            Ok(entries) => {
                for (k, _) in entries {
                    if !QUALITY.contains(&k.as_str()) {
                        errors.push(SettingsError::new(format!("quality.{k}"), "unknown field"));
                    }
                }
                let num = |k: &str| q.find(k).filter(|v| !v.is_null()).map(Json::as_double);
                match num("gap_factor") {
                    None => {}
                    Some(Ok(v)) if v > 1.0 && v <= 1000.0 => opt.gap_factor = v,
                    Some(_) => errors.push(SettingsError::new(
                        "quality.gap_factor",
                        "must be a number above 1 and at most 1000 (a gap is that many median intervals)",
                    )),
                }
                match num("outlier_z") {
                    None => {}
                    Some(Ok(v)) if v > 0.0 && v <= 1000.0 => opt.outlier_z = v,
                    Some(_) => errors.push(SettingsError::new(
                        "quality.outlier_z",
                        "must be a number above 0 and at most 1000",
                    )),
                }
                match q.find("stale_run").filter(|v| !v.is_null()).map(Json::as_int) {
                    None => {}
                    Some(Ok(v)) if (2..=100_000).contains(&v) => {
                        opt.stale_run = usize::try_from(v).unwrap_or(5);
                    }
                    Some(_) => errors.push(SettingsError::new(
                        "quality.stale_run",
                        "must be a whole number from 2 to 100000",
                    )),
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(opt)
    } else {
        Err(errors)
    }
}

/// The settings document for `opt`, in canonical form (every field present).
/// [`parse_settings`] of the result gives `opt` back, with
/// `explicit_columns = true`.
pub fn settings_json(opt: &CsvOptions) -> Json {
    let col = |name: &str| if name.is_empty() { Json::Null } else { Json::string(name) };
    let num = |v: f64| Json::number(v).unwrap_or(Json::Null);
    let has_symbol = !opt.symbol_column.is_empty();
    let mut doc = Json::object()
        .with("delimiter", Json::string((opt.delimiter as char).to_string()))
        .with("time_format", Json::string(time_format_name(opt.time.format)))
        .with("utc_offset_minutes", Json::integer(i64::from(opt.time.naive_utc_offset_minutes)))
        .with(
            "columns",
            Json::object()
                .with("timestamp", col(&opt.timestamp_column))
                .with("open", col(&opt.open_column))
                .with("high", col(&opt.high_column))
                .with("low", col(&opt.low_column))
                .with("close", col(&opt.close_column))
                .with("volume", col(&opt.volume_column))
                .with("symbol", col(&opt.symbol_column)),
        )
        .with("default_symbol", if has_symbol { Json::Null } else { Json::string(&opt.default_symbol) })
        .with("skip_bad_rows", Json::boolean(opt.on_invalid == InvalidRowPolicy::Skip))
        .with(
            "quality",
            Json::object()
                .with("gap_factor", num(opt.gap_factor))
                .with("outlier_z", num(opt.outlier_z))
                .with("stale_run", Json::integer(i64::try_from(opt.stale_run).unwrap_or(i64::MAX))),
        );
    if let Some(tz) = opt.time.timezone {
        doc = doc.with("timezone", Json::string(tz.name()));
    }
    if let Some(ex) = opt.exchange {
        doc = doc.with("exchange", Json::string(ex.code()));
    }
    doc
}

/// Delimiters [`suggest`] considers, most common first.
const CANDIDATE_DELIMITERS: &[u8] = b",;\t|";
/// Data rows [`suggest`] looks at.
const SUGGEST_ROWS: usize = 50;

/// Suggested settings for a file, from its first rows. Every field is a
/// proposal for the person to confirm or change; `None` means no confident
/// guess.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Suggestions {
    /// Delimiter that splits the header and the first rows into the same
    /// number of fields.
    pub delimiter: Option<char>,
    /// The header split with that delimiter, as written (trimmed).
    pub header: Vec<String>,
    /// Header names matched by the usual aliases, in [`COLUMNS`] order
    /// (`timestamp`, `open`, `high`, `low`, `close`, `volume`, `symbol`).
    pub columns: Vec<(&'static str, Option<String>)>,
    /// Likely `time_format` name, from the timestamp column's first values.
    pub time_format: Option<&'static str>,
    /// For ISO timestamps: `Some(true)` if they carry a zone (`Z`, `+05:30`),
    /// so `utc_offset_minutes` does not affect them.
    pub timestamps_have_zone: Option<bool>,
    /// Suggested exchange code (e.g. "XNYS").
    pub exchange: Option<&'static str>,
    /// Suggested timezone identifier (e.g. "America/New_York").
    pub timezone: Option<&'static str>,
}

impl Suggestions {
    /// As JSON for the preview response.
    pub fn to_json(&self) -> Json {
        let mut columns = Json::object();
        for (k, v) in &self.columns {
            let _ = columns.set(*k, v.as_ref().map_or(Json::Null, |n| Json::string(n.clone())));
        }
        let mut header = Json::array();
        for h in &self.header {
            let _ = header.push(Json::string(h.clone()));
        }
        let mut doc = Json::object()
            .with("delimiter", self.delimiter.map_or(Json::Null, |d| Json::string(d.to_string())))
            .with("header", header)
            .with("columns", columns)
            .with("time_format", self.time_format.map_or(Json::Null, Json::string))
            .with("timestamps_have_zone", self.timestamps_have_zone.map_or(Json::Null, Json::boolean));
        if let Some(ex) = self.exchange {
            doc = doc.with("exchange", Json::string(ex));
        }
        if let Some(tz) = self.timezone {
            doc = doc.with("timezone", Json::string(tz));
        }
        doc
    }
}

/// Proposes settings for a file from its start (`sample`). Never fails: what
/// it cannot tell is left `None`.
pub fn suggest(sample: &[u8]) -> Suggestions {
    let sample = sample.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(sample);
    // The delimiter that gives the header the most fields and every sampled row the same count.
    let mut best: Option<(u8, Vec<Record>)> = None;
    for &d in CANDIDATE_DELIMITERS {
        let Ok(mut records) = parse_records(sample, d) else { continue };
        records.truncate(SUGGEST_ROWS + 1);
        let Some(width) = records.first().map(|r| r.fields.len()) else { continue };
        // The last record may be cut off by the sample's end.
        let body = &records[1..records.len().saturating_sub(1).max(1)];
        if width < 2 || body.iter().any(|r| r.fields.len() != width) {
            continue;
        }
        if best.as_ref().is_none_or(|(_, b)| width > b[0].fields.len()) {
            best = Some((d, records));
        }
    }
    let Some((delim, records)) = best else { return Suggestions::default() };

    let header: Vec<String> = records[0].fields.iter().map(|f| f.trim().to_string()).collect();
    let norm: Vec<String> = header.iter().map(|h| normalise_header(h)).collect();
    let aliases = [
        TIMESTAMP_ALIASES,
        OPEN_ALIASES,
        HIGH_ALIASES,
        LOW_ALIASES,
        CLOSE_ALIASES,
        VOLUME_ALIASES,
        SYMBOL_ALIASES,
        KNOWN_AT_ALIASES,
    ];
    let columns: Vec<(&'static str, Option<String>)> = COLUMNS
        .iter()
        .zip(aliases)
        .map(|(&k, a)| (k, find_column(&norm, "", a, false).map(|i| header[i].clone())))
        .collect();

    let ts_col = find_column(&norm, "", TIMESTAMP_ALIASES, false);
    let values: Vec<&str> = ts_col
        .map(|c| {
            records[1..]
                .iter()
                .filter_map(|r| r.fields.get(c).map(|v| v.trim()))
                .filter(|v| !v.is_empty())
                .take(20)
                .collect()
        })
        .unwrap_or_default();
    let (time_format, timestamps_have_zone) = guess_time_format(&values);
    let timezone = if timestamps_have_zone == Some(true) { Some("UTC") } else { Some("America/New_York") };
    let exchange = Some("XNYS");
    Suggestions {
        delimiter: Some(delim as char),
        header,
        columns,
        time_format,
        timestamps_have_zone,
        exchange,
        timezone,
    }
}

/// A time format every value parses with, and whether ISO values carry a zone.
fn guess_time_format(values: &[&str]) -> (Option<&'static str>, Option<bool>) {
    if values.is_empty() {
        return (None, None);
    }
    if values.iter().all(|v| {
        let digits = v.strip_prefix('-').unwrap_or(v);
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    }) {
        // Magnitude of the present day: 10 digits in seconds, 13 in ms, 16 in µs, 19 in ns.
        let len = values.iter().map(|v| v.trim_start_matches('-').len()).max().unwrap_or(0);
        let name = match len {
            0..=11 => "epoch_s",
            12..=14 => "epoch_ms",
            15..=17 => "epoch_us",
            _ => "epoch_ns",
        };
        return (Some(name), None);
    }
    let iso = TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 0, timezone: None };
    let parsed: Option<Vec<bool>> =
        values.iter().map(|v| parse_timestamp(v, &iso).map(|t| t.naive)).collect();
    match parsed {
        Some(naive) => (Some("iso8601"), Some(naive.iter().all(|n| !n))),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Json {
        Json::parse(text).unwrap()
    }

    const GOOD: &str = r#"{"delimiter":",","time_format":"iso8601","utc_offset_minutes":330,
        "columns":{"timestamp":"Date","open":"Open","high":"High","low":"Low","close":"Close","volume":null},
        "default_symbol":"NIFTY","quality":{"gap_factor":3,"stale_run":4}}"#;

    #[test]
    fn a_complete_document_becomes_explicit_options() {
        let opt = parse_settings(&doc(GOOD)).unwrap();
        assert!(opt.explicit_columns);
        assert_eq!(opt.delimiter, b',');
        assert_eq!(
            opt.time,
            TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 330, timezone: None }
        );
        assert_eq!(
            (opt.timestamp_column.as_str(), opt.close_column.as_str(), opt.volume_column.as_str()),
            ("Date", "Close", "")
        );
        assert_eq!(opt.default_symbol, "NIFTY");
        assert_eq!(opt.on_invalid, InvalidRowPolicy::Reject, "bad rows abort by default");
        assert!((opt.gap_factor - 3.0).abs() < f64::EPSILON);
        assert!((opt.outlier_z - 10.0).abs() < f64::EPSILON, "default kept");
        assert_eq!(opt.stale_run, 4);
    }

    #[test]
    fn canonical_json_round_trips() {
        let opt = parse_settings(&doc(GOOD)).unwrap();
        let again = parse_settings(&settings_json(&opt)).unwrap();
        assert_eq!(again, opt);
        let with_symbol = parse_settings(&doc(
            r#"{"delimiter":"\t","time_format":"epoch_ms","utc_offset_minutes":0,"skip_bad_rows":true,
                "columns":{"timestamp":"t","open":"o","high":"h","low":"l","close":"c","volume":"v","symbol":"sym"}}"#,
        ))
        .unwrap();
        assert_eq!(parse_settings(&settings_json(&with_symbol)).unwrap(), with_symbol);
        assert_eq!(with_symbol.on_invalid, InvalidRowPolicy::Skip);
    }

    #[test]
    fn every_problem_is_reported_with_its_field() {
        let errors = parse_settings(&doc(r#"{"delimiter":"ab","utc_offset_minutes":2000,"extra":1,
                "columns":{"timestamp":"d","open":"x","high":"X","close":"","volume":"","colour":"red"},
                "skip_bad_rows":"yes","quality":{"gap_factor":1,"outlier_z":-1,"stale_run":1,"other":0}}"#))
        .unwrap_err();
        let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(
            fields,
            [
                "extra",
                "delimiter",
                "time_format",
                "utc_offset_minutes",
                "columns.colour",
                "columns.high",
                "columns.low",
                "columns.close",
                "columns.volume",
                "default_symbol",
                "skip_bad_rows",
                "quality.other",
                "quality.gap_factor",
                "quality.outlier_z",
                "quality.stale_run",
            ],
            "{errors:#?}"
        );
        assert!(
            errors[5].message.contains("same column as columns.open"),
            "header names are case-insensitive"
        );
    }

    #[test]
    fn nothing_is_defaulted_silently() {
        let errors = parse_settings(&doc("{}")).unwrap_err();
        let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["delimiter", "time_format", "utc_offset_minutes", "columns"]);
        assert!(parse_settings(&doc("[]")).unwrap_err()[0].message.contains("object"));
    }

    #[test]
    fn default_symbol_only_without_a_symbol_column() {
        let both = GOOD.replace(r#""volume":null"#, r#""volume":null,"symbol":"Ticker""#);
        let errors = parse_settings(&doc(&both)).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "default_symbol");
    }

    #[test]
    fn explicit_settings_give_the_command_line_dataset_id() {
        use crate::dataset::load_csv;
        let csv = b"Date,Symbol,Open,High,Low,Close,Volume\n\
            2024-01-02,SPY,470.1,472,469,471.5,100\n2024-01-02,QQQ,400,401,399,400.5,50\n\
            2024-01-03,SPY,471.5,473,470,472.25,80\n";
        let cli = load_csv(csv, &CsvOptions::default()).unwrap();
        let settings = r#"{"delimiter":",","time_format":"iso8601","utc_offset_minutes":0,
            "columns":{"timestamp":"Date","open":"Open","high":"High","low":"Low","close":"Close",
                       "volume":"Volume","symbol":"Symbol"}}"#;
        let studio = load_csv(csv, &parse_settings(&doc(settings)).unwrap()).unwrap();
        assert_eq!(studio.info.id, cli.info.id, "same file, same dataset id");

        // Explicit means explicit: a null volume is no volume, even with a "Volume" header.
        let no_volume = settings.replace(r#""Volume""#, "null");
        let d = load_csv(csv, &parse_settings(&doc(&no_volume)).unwrap()).unwrap();
        assert!(d.bars.iter().all(|b| b.volume.raw() == 0));
        assert_ne!(d.info.id, cli.info.id);
        // A named column that is not in the header is an error, not "no such column".
        let typo = settings.replace(r#""Volume""#, r#""Vlume""#);
        let err = load_csv(csv, &parse_settings(&doc(&typo)).unwrap()).unwrap_err();
        assert!(err.to_string().contains("missing volume column: Vlume"), "{err}");
    }

    #[test]
    fn suggestions_come_from_the_header_and_first_rows() {
        let s = suggest(b"\xEF\xBB\xBFDate;Ticker;Open;High;Low;Close;Vol\n2024-01-02;SPY;1;2;0.5;1.5;10\n2024-01-03;SPY;1;2;0.5;1.5;1");
        assert_eq!(s.delimiter, Some(';'));
        let cols: Vec<(&str, Option<&str>)> = s.columns.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        assert_eq!(
            cols,
            [
                ("timestamp", Some("Date")),
                ("open", Some("Open")),
                ("high", Some("High")),
                ("low", Some("Low")),
                ("close", Some("Close")),
                ("volume", Some("Vol")),
                ("symbol", Some("Ticker")),
            ]
        );
        assert_eq!((s.time_format, s.timestamps_have_zone), (Some("iso8601"), Some(false)));

        let ms = suggest(b"ts,open,high,low,close\n1704153600000,1,2,0.5,1\n1704240000000,1,2,0.5,1\n");
        assert_eq!((ms.delimiter, ms.time_format), (Some(','), Some("epoch_ms")));
        let zoned = suggest(b"time,o,h,l,c\n2024-01-02T09:15:00+05:30,1,2,0.5,1\n");
        assert_eq!(zoned.timestamps_have_zone, Some(true));
        assert_eq!(suggest(b"").delimiter, None);
        assert_eq!(suggest(b"just one column\nx\n").time_format, None);
    }

    #[test]
    fn timezone_and_exchange_settings() {
        let text = r#"{"delimiter":",","time_format":"iso8601","timezone":"America/New_York","exchange":"XNYS",
            "columns":{"timestamp":"Date","open":"Open","high":"High","low":"Low","close":"Close","volume":"Vol"},
            "default_symbol":"SPY"}"#;
        let opt = parse_settings(&doc(text)).unwrap();
        assert_eq!(opt.time.timezone, Some(Timezone::AmericaNewYork));
        assert_eq!(opt.exchange, Some(Exchange::Xnys));
        // Canonical JSON round trip
        let round_trip = parse_settings(&settings_json(&opt)).unwrap();
        assert_eq!(round_trip.time.timezone, Some(Timezone::AmericaNewYork));
        assert_eq!(round_trip.exchange, Some(Exchange::Xnys));

        // Invalid timezone and exchange report errors
        let bad = r#"{"delimiter":",","time_format":"iso8601","timezone":"Mars/Colony","exchange":"INVALID",
            "columns":{"timestamp":"Date","open":"Open","high":"High","low":"Low","close":"Close","volume":"Vol"},
            "default_symbol":"SPY"}"#;
        let errs = parse_settings(&doc(bad)).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "timezone"));
        assert!(errs.iter().any(|e| e.field == "exchange"));
    }
}
