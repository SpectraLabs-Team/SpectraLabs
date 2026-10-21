//! Calendar-aware quality checks on daily bars (M1 task 10).

#[test]
fn daily_bars_are_judged_by_trading_date_against_a_calendar() {
    // Daily bars stamped at midnight UTC, Mon 1 to Mon 8 July 2024, every calendar day.
    // With XNYS: Thu 4 (Independence Day), Sat 6 and Sun 7 are not trading days; the rest are,
    // even though 00:00 UTC is 20:00 the evening before in New York.
    let mut csv = String::from("timestamp,symbol,open,high,low,close,volume\n");
    for d in 1..=8 {
        csv.push_str(&format!("2024-07-{d:02}T00:00:00Z,SPY,100,101,99,100.5,1000\n"));
    }
    let opts = spectra_data::CsvOptions {
        exchange: Some(spectra_data::Exchange::Xnys),
        ..spectra_data::CsvOptions::default()
    };
    let d = spectra_data::load_csv(csv.as_bytes(), &opts).unwrap();
    let s = &d.quality.symbols[0];
    assert_eq!((s.out_of_session_bars, s.non_trading_day_bars), (3, 3));

    // Without a calendar nothing is flagged.
    let plain = spectra_data::load_csv(csv.as_bytes(), &spectra_data::CsvOptions::default()).unwrap();
    assert_eq!(
        (plain.quality.symbols[0].out_of_session_bars, plain.quality.symbols[0].non_trading_day_bars),
        (0, 0)
    );
}
