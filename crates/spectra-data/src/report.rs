//! JSON forms of a dataset's quality report and of an ingest preview, for the
//! API and for `dataset_versions.quality_report`.
//!
//! [`quality_report_json`] keeps every key `spectra-db dataset-add` has always
//! stored (`rows_total`, `rows_accepted`, `rows_rejected`, `warnings`,
//! `symbols` as a list of names, `first_ts_ns`, `last_ts_ns`, `bars_sha256`)
//! and adds the rest of the report, so older readers keep working.

use spectra_core::Json;

use crate::dataset::{CsvOptions, CsvPreview, Dataset};
use crate::time::format_timestamp;

fn strings(items: &[String]) -> Json {
    let mut out = Json::array();
    for s in items {
        let _ = out.push(Json::string(s.clone()));
    }
    out
}

fn count(n: usize) -> Json {
    Json::integer(i64::try_from(n).unwrap_or(i64::MAX))
}

/// A fixed-point raw value as decimal text without trailing zeros (`470.1`).
fn decimal(raw: i64) -> String {
    let text = spectra_core::Price::from_raw(raw).to_string();
    let text = text.trim_end_matches('0');
    text.strip_suffix('.').unwrap_or(text).to_string()
}

/// The full quality report of `d`, with the thresholds it was judged by.
pub fn quality_report_json(d: &Dataset, opt: &CsvOptions) -> Json {
    let q = &d.quality;
    let mut by_category = Json::object();
    for (c, n) in &q.rejected_by_category {
        let _ = by_category.set(c.clone(), count(*n));
    }
    let mut issues = Json::array();
    for i in &q.sample_issues {
        let _ = issues.push(
            Json::object()
                .with("line", count(i.line))
                .with("category", Json::string(i.category.clone()))
                .with("message", Json::string(i.message.clone())),
        );
    }
    let mut stats = Json::array();
    for s in &q.symbols {
        let _ = stats.push(
            Json::object()
                .with("symbol", Json::string(s.symbol.clone()))
                .with("bars", count(s.bars))
                .with("first_ts", Json::string(format_timestamp(s.first_ts)))
                .with("last_ts", Json::string(format_timestamp(s.last_ts)))
                .with("median_interval_ns", Json::integer(s.median_interval_ns))
                .with("gaps", count(s.gaps))
                .with("largest_gap_ns", Json::integer(s.largest_gap_ns))
                .with("zero_volume_bars", count(s.zero_volume_bars))
                .with("outlier_returns", count(s.outlier_returns))
                .with("stale_runs", count(s.stale_runs)),
        );
    }
    let num = |v: f64| Json::number(v).unwrap_or(Json::Null);
    Json::object()
        .with("rows_total", count(q.rows_total))
        .with("rows_accepted", count(q.rows_accepted))
        .with("rows_rejected", count(q.rows_rejected))
        .with("warnings", strings(&q.warnings))
        .with("symbols", strings(&d.symbols.symbols))
        .with("first_ts_ns", Json::integer(d.info.first_ts))
        .with("last_ts_ns", Json::integer(d.info.last_ts))
        .with("bars_sha256", Json::string(d.info.bars_sha256.clone()))
        .with("first_ts", Json::string(format_timestamp(d.info.first_ts)))
        .with("last_ts", Json::string(format_timestamp(d.info.last_ts)))
        .with("rejected_by_category", by_category)
        .with("sample_issues", issues)
        .with("input_was_sorted", Json::boolean(q.input_was_sorted))
        .with("notes", strings(&q.notes))
        .with("symbol_stats", stats)
        .with(
            "thresholds",
            Json::object()
                .with("gap_factor", num(opt.gap_factor))
                .with("outlier_z", num(opt.outlier_z))
                .with("stale_run", count(opt.stale_run)),
        )
}

/// The preview response body: the header, the parsed rows (at most
/// `max_rows`, as decimal text), and what the sample's quality report and
/// rejected rows look like.
pub fn preview_json(p: &CsvPreview, opt: &CsvOptions, max_rows: usize) -> Json {
    let d = &p.dataset;
    let mut rows = Json::array();
    for b in d.bars.iter().take(max_rows) {
        let _ = rows.push(
            Json::object()
                .with("timestamp", Json::string(format_timestamp(b.ts_ns)))
                .with("symbol", Json::string(d.symbols.name(b.instrument).unwrap_or_default()))
                .with("open", Json::string(decimal(b.open.raw())))
                .with("high", Json::string(decimal(b.high.raw())))
                .with("low", Json::string(decimal(b.low.raw())))
                .with("close", Json::string(decimal(b.close.raw())))
                .with("volume", Json::string(decimal(b.volume.raw()))),
        );
    }
    Json::object()
        .with("header", strings(&p.header))
        .with("rows", rows)
        .with("rows_not_read", count(p.rows_not_read))
        .with("would_abort", p.would_abort.clone().map_or(Json::Null, Json::string))
        .with("quality", quality_report_json(d, opt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{load_csv, preview_csv, InvalidRowPolicy};

    const CSV: &str = "date,open,high,low,close,volume\n\
        2024-01-02,470.10,472,469,471.5,100\n\
        2024-01-03,471.5,473,470,472.25,0\n";

    #[test]
    fn the_report_keeps_the_command_line_keys() {
        let d = load_csv(CSV.as_bytes(), &CsvOptions::default()).unwrap();
        let j = quality_report_json(&d, &CsvOptions::default());
        for key in [
            "rows_total",
            "rows_accepted",
            "rows_rejected",
            "warnings",
            "first_ts_ns",
            "last_ts_ns",
            "bars_sha256",
        ] {
            assert!(j.at(key).is_ok(), "missing {key}");
        }
        assert_eq!(j.at("symbols").unwrap().items().unwrap()[0].as_str().unwrap(), "DEFAULT");
        let stats = &j.at("symbol_stats").unwrap().items().unwrap()[0];
        assert_eq!(stats.at("zero_volume_bars").unwrap().as_int().unwrap(), 1);
        assert_eq!(j.at("bars_sha256").unwrap().as_str().unwrap(), d.info.bars_sha256);
    }

    #[test]
    fn a_preview_shows_rows_and_every_bad_row() {
        let csv = format!("{CSV}2024-01-04,abc,1,1,1,1\n2024-01-05,1,1,1,1,-1\n2024-01-06,1,1,1,1,1\n");
        let opt = CsvOptions { default_symbol: "SPY".into(), ..CsvOptions::default() };
        let p = preview_csv(csv.as_bytes(), &opt, 4).unwrap();
        assert_eq!(p.rows_not_read, 1);
        assert_eq!(p.dataset.quality.rows_rejected, 2, "both bad rows are reported, not just the first");
        assert!(p.would_abort.as_deref().unwrap().starts_with("line 4 [bad_number]"));
        let j = preview_json(&p, &opt, 200);
        let rows = j.at("rows").unwrap().items().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].at("open").unwrap().as_str().unwrap(), "470.1");
        assert_eq!(rows[0].at("symbol").unwrap().as_str().unwrap(), "SPY");
        assert_eq!(rows[0].at("timestamp").unwrap().as_str().unwrap(), "2024-01-02T00:00:00Z");

        let skipping = CsvOptions { on_invalid: InvalidRowPolicy::Skip, ..opt };
        assert_eq!(preview_csv(csv.as_bytes(), &skipping, 4).unwrap().would_abort, None);
    }

    #[test]
    fn progress_is_reported_in_bytes_and_can_cancel() {
        use crate::dataset::{load_csv_reader, PROGRESS_EVERY_ROWS};
        let mut csv = String::from("date,open,high,low,close\n");
        let start = 1_700_000_000i64;
        for i in 0..(2 * PROGRESS_EVERY_ROWS + 5) {
            csv += &format!("{},1,2,0.5,1.5\n", start + i as i64 * 60);
        }
        let opt = CsvOptions {
            time: crate::TimeOptions { format: crate::TimeFormat::EpochSeconds, naive_utc_offset_minutes: 0 },
            ..CsvOptions::default()
        };
        let mut seen = Vec::new();
        let d = load_csv_reader(
            csv.as_bytes(),
            &opt,
            Some(&mut |bytes| {
                seen.push(bytes);
                true
            }),
        )
        .unwrap();
        assert_eq!(seen.len(), 3, "every 10k rows, then once at the end");
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(*seen.last().unwrap(), csv.len() as u64);
        assert_eq!(d, load_csv(csv.as_bytes(), &opt).unwrap(), "same result as load_csv");
        let err = load_csv_reader(csv.as_bytes(), &opt, Some(&mut |_| false)).unwrap_err();
        assert_eq!(err.to_string(), "cancelled");
    }

    #[test]
    fn a_preview_of_only_bad_rows_still_reports_them() {
        let p = preview_csv(b"date,open,high,low,close\nx,1,1,1,1\n", &CsvOptions::default(), 200).unwrap();
        assert!(p.dataset.bars.is_empty());
        assert_eq!(p.dataset.quality.rows_rejected, 1);
        assert!(p.would_abort.is_some());
        assert!(preview_csv(b"a,b\n1,2\n", &CsvOptions::default(), 200).is_err(), "missing columns");
    }
}
