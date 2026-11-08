//! M41 Events, Macro & News Reference Tests.

use spectra_data::events::*;

#[test]
fn test_m41_earnings_announcement_date_isolation() {
    let mut store = EventStore::new();
    let announcement_ts = 1_715_000_000_000_000_000i64;

    store.add_earnings(EarningsEvent {
        symbol: "MSFT".into(),
        fiscal_period: "2024-Q3".into(),
        announcement_ts_ns: announcement_ts,
        consensus_eps: Some(2.80),
        actual_eps: Some(2.94), // +5% surprise
        consensus_revenue: Some(60_000_000_000.0),
        actual_revenue: Some(61_860_000_000.0),
    });

    // 1. One minute before announcement: event must not exist in point-in-time query
    let before = store.get_announced_earnings("MSFT", announcement_ts - 60_000_000_000);
    assert_eq!(before.len(), 0);

    // 2. Exactly at announcement: event and surprise are unlocked
    let at_time = store.get_announced_earnings("MSFT", announcement_ts);
    assert_eq!(at_time.len(), 1);
    assert_eq!(at_time[0].actual_eps, Some(2.94));
    let surprise = at_time[0].eps_surprise_pct().unwrap();
    assert!((surprise - 0.05).abs() < 1e-6);
}

#[test]
fn test_m41_economic_macro_publisher_metadata() {
    let mut store = EventStore::new();

    store.add_macro_release(MacroRelease {
        series_id: "FEDFUNDS".into(),
        indicator_name: "Federal Funds Target Rate".into(),
        publisher: "Federal Open Market Committee (FOMC)".into(),
        release_ts_ns: 10_000,
        period_covered: "2024-05".into(),
        consensus: Some(5.25),
        actual: 5.25,
        prior: Some(5.25),
    });

    let releases = store.get_macro_series("FEDFUNDS", 20_000);
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].publisher, "Federal Open Market Committee (FOMC)");
    assert_eq!(releases[0].actual, 5.25);
}

#[test]
fn test_m41_news_feed_source_attribution() {
    let mut store = EventStore::new();

    store.add_news(NewsItem {
        symbol: "NVDA".into(),
        headline: "NVIDIA Announces Next-Generation Architecture".into(),
        source: "Reuters".into(),
        published_ts_ns: 50_000,
        url: "https://reuters.com/nvda".into(),
    });

    let news = store.get_news_as_of("NVDA", 60_000);
    assert_eq!(news.len(), 1);
    assert_eq!(news[0].source, "Reuters");
    assert_eq!(news[0].headline, "NVIDIA Announces Next-Generation Architecture");
}
