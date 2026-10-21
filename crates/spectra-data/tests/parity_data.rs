//! C++ <-> Rust parity for the Data Studio: every case in `parity/data` must
//! produce the same quality report, bars, errors and parse results as the
//! frozen C++ engine (`parity/golden/data/data.golden`), and the binary store
//! format must be byte-identical in both directions.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use spectra_data::datastore::{decode_dataset, encode_dataset};
use spectra_data::*;

fn parity() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity")
}

fn options(path: &PathBuf) -> CsvOptions {
    let text = fs::read_to_string(path).unwrap();
    let m: HashMap<&str, &str> = text.lines().filter_map(|l| l.split_once('=')).collect();
    let tf = match m["tf"] {
        "s" => TimeFormat::EpochSeconds,
        "ms" => TimeFormat::EpochMillis,
        "us" => TimeFormat::EpochMicros,
        "ns" => TimeFormat::EpochNanos,
        _ => TimeFormat::Iso8601,
    };
    CsvOptions {
        on_invalid: if m["skip"] == "1" { InvalidRowPolicy::Skip } else { InvalidRowPolicy::Reject },
        delimiter: if m["delim"] == "TAB" { b'\t' } else { m["delim"].as_bytes()[0] },
        time: TimeOptions {
            format: tf,
            naive_utc_offset_minutes: m["offset"].parse().unwrap(),
            timezone: None,
        },
        default_symbol: m["symbol"].into(),
        timestamp_column: m["ts"].into(),
        open_column: m["open"].into(),
        high_column: m["high"].into(),
        low_column: m["low"].into(),
        close_column: m["close"].into(),
        volume_column: m["volume"].into(),
        symbol_column: m["sym"].into(),
        ..CsvOptions::default()
    }
}

/// The same bytes the C++ store would have written (it records engine "0.6.0").
fn as_cpp(d: &Dataset) -> Dataset {
    let mut c = d.clone();
    c.info.engine_version = "0.6.0".into();
    c
}

#[test]
fn data_studio_matches_cpp_reference() {
    let dir = parity().join("data");
    let golden_dir = parity().join("golden/data");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension().is_some_and(|x| x == "csv"))
                .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    assert!(names.len() >= 31);

    let tmp = std::env::temp_dir().join(format!("spectra_data_parity_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let store = DataStore::new(&tmp).unwrap();
    let mut out = String::new();
    let mut loaded = 0;
    for name in &names {
        let opt = options(&dir.join(format!("{name}.opts")));
        writeln!(out, "=== {name}").unwrap();
        match load_csv(&fs::read(dir.join(format!("{name}.csv"))).unwrap(), &opt) {
            Ok(d) => {
                loaded += 1;
                out += &format_quality_report(&d);
                writeln!(out, "source {}", d.info.source_sha256).unwrap();
                for b in d.bars.iter().take(5) {
                    writeln!(
                        out,
                        "bar {} {} {} {} {} {} {}",
                        b.ts_ns,
                        b.instrument,
                        b.open.raw(),
                        b.high.raw(),
                        b.low.raw(),
                        b.close.raw(),
                        b.volume.raw()
                    )
                    .unwrap();
                }
                // The binary image and metadata are byte-identical to the C++ store's.
                let cpp_qfb = fs::read(golden_dir.join(format!("{name}.qfb"))).unwrap();
                assert_eq!(encode_dataset(&as_cpp(&d)), cpp_qfb, "{name}: bars.qfb differs from C++");
                store.put(&as_cpp(&d)).unwrap();
                let meta = fs::read(tmp.join(&d.info.id).join("meta.json")).unwrap();
                let cpp_meta = fs::read(golden_dir.join(format!("{name}.meta.json"))).unwrap();
                assert_eq!(meta, cpp_meta, "{name}: meta.json differs from C++");
                // And the C++ file decodes in Rust.
                let from_cpp = decode_dataset(&cpp_qfb, &d.info.id).unwrap();
                assert_eq!(from_cpp.bars, d.bars);
                let back = store.get(&d.info.id).unwrap();
                writeln!(
                    out,
                    "reloaded {} {} {}",
                    back.info.id, back.quality.rows_total, back.quality.rows_rejected
                )
                .unwrap();
                out += &format_quality_report(&back);
            }
            Err(e) => writeln!(out, "ERROR {e}").unwrap(),
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    assert!(loaded >= 15);

    for line in fs::read_to_string(dir.join("parse_fixed.txt")).unwrap().lines() {
        let v = parse_fixed(line).map_or_else(|| "none".to_string(), |v| v.to_string());
        writeln!(out, "F [{line}] {v}").unwrap();
    }
    let fmts = [
        ("iso", TimeFormat::Iso8601),
        ("s", TimeFormat::EpochSeconds),
        ("ms", TimeFormat::EpochMillis),
        ("us", TimeFormat::EpochMicros),
        ("ns", TimeFormat::EpochNanos),
    ];
    for line in fs::read_to_string(dir.join("parse_time.txt")).unwrap().lines() {
        for (fname, fmt) in fmts {
            for off in [0, 330, -90] {
                let t = TimeOptions { format: fmt, naive_utc_offset_minutes: off, timezone: None };
                write!(out, "T {fname} {off} [{line}] ").unwrap();
                match parse_timestamp(line, &t) {
                    Some(p) => writeln!(
                        out,
                        "{}{} {}",
                        p.ts_ns,
                        if p.naive { " naive" } else { " zoned" },
                        format_timestamp(p.ts_ns)
                    )
                    .unwrap(),
                    None => out += "none\n",
                }
            }
        }
    }

    let golden = fs::read_to_string(golden_dir.join("data.golden")).unwrap();
    if out != golden {
        let (n, (a, g)) = out
            .lines()
            .zip(golden.lines())
            .enumerate()
            .find(|(_, (a, g))| a != g)
            .unwrap_or((0, ("<length>", "")));
        panic!("data parity: first difference at line {}\n  rust: {a}\n  c++ : {g}", n + 1);
    }
}
