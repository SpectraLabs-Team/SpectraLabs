//! spectra-ingest: load a CSV into a content-addressed dataset store and
//! print its quality report.
//!
//! ```text
//! spectra-ingest FILE.csv --store DIR [--skip-invalid] [--time-format iso|s|ms|us|ns]
//!                         [--utc-offset-min N] [--symbol NAME] [--delimiter C]
//! spectra-ingest --list --store DIR
//! spectra-ingest --show ID --store DIR
//! ```
//!
//! Exit codes: 0 success, 1 data error, 2 usage error.

use std::process::ExitCode;

use spectra_data::{
    format_quality_report, load_csv_file, CsvOptions, DataStore, InvalidRowPolicy, TimeFormat,
};

const USAGE: &str = "usage:
  spectra-ingest FILE.csv --store DIR [--skip-invalid] [--time-format iso|s|ms|us|ns]
                          [--utc-offset-min N] [--symbol NAME] [--delimiter C]
  spectra-ingest --list --store DIR
  spectra-ingest --show ID --store DIR
";

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut file, mut store_dir, mut show_id, mut list) =
        (None::<String>, None::<String>, None::<String>, false);
    let mut opt = CsvOptions::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = || {
            i += 1;
            args.get(i).cloned()
        };
        match a {
            "--store" => store_dir = value(),
            "--skip-invalid" => opt.on_invalid = InvalidRowPolicy::Skip,
            "--list" => list = true,
            "--show" => show_id = value(),
            "--symbol" => match value() {
                Some(v) => opt.default_symbol = v,
                None => return usage(),
            },
            "--delimiter" => match value() {
                Some(d) if d.len() == 1 => opt.delimiter = d.as_bytes()[0],
                _ => return usage(),
            },
            "--utc-offset-min" => match value().and_then(|v| v.parse().ok()) {
                Some(m) => opt.time.naive_utc_offset_minutes = m,
                None => return usage(),
            },
            "--time-format" => {
                opt.time.format = match value().as_deref() {
                    Some("iso") => TimeFormat::Iso8601,
                    Some("s") => TimeFormat::EpochSeconds,
                    Some("ms") => TimeFormat::EpochMillis,
                    Some("us") => TimeFormat::EpochMicros,
                    Some("ns") => TimeFormat::EpochNanos,
                    _ => return usage(),
                }
            }
            _ if !a.starts_with('-') && file.is_none() => file = Some(a.to_string()),
            _ => return usage(),
        }
        i += 1;
    }
    let Some(store_dir) = store_dir else { return usage() };

    let run = || -> spectra_core::Result<ExitCode> {
        let store = DataStore::new(&store_dir)?;
        if list {
            for id in store.list() {
                println!("{id}");
            }
            return Ok(ExitCode::SUCCESS);
        }
        if let Some(id) = &show_id {
            print!("{}", format_quality_report(&store.get(id)?));
            return Ok(ExitCode::SUCCESS);
        }
        let Some(file) = &file else { return Ok(usage()) };
        let d = load_csv_file(file, &opt)?;
        let existed = store.contains(&d.info.id);
        store.put(&d)?;
        print!("{}", format_quality_report(&d));
        println!("\n{} {}", if existed { "already stored:" } else { "stored:" }, d.info.id);
        Ok(ExitCode::SUCCESS)
    };
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
