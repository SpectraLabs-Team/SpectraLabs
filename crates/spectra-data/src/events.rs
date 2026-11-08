//! Events, Macro Data & Economic Calendar Engine (M41).
//!
//! Provides earnings calendars with consensus/surprise metrics, economic releases
//! (CPI, GDP, unemployment, interest rates), publisher citations, and event-aware
//! query interfaces that protect backtests from look-ahead bias prior to public announcement.

/// An earnings release event for a public equity instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct EarningsEvent {
    pub symbol: String,
    pub fiscal_period: String,
    /// When the earnings release became public (nanoseconds UTC).
    pub announcement_ts_ns: i64,
    pub consensus_eps: Option<f64>,
    pub actual_eps: Option<f64>,
    pub consensus_revenue: Option<f64>,
    pub actual_revenue: Option<f64>,
}

impl EarningsEvent {
    /// Calculate EPS surprise percentage: `(actual - consensus) / |consensus|`.
    pub fn eps_surprise_pct(&self) -> Option<f64> {
        if let (Some(act), Some(cons)) = (self.actual_eps, self.consensus_eps) {
            if cons.abs() > 1e-6 {
                Some((act - cons) / cons.abs())
            } else {
                Some(0.0)
            }
        } else {
            None
        }
    }

    /// True if the event was already public as of `as_of_ts_ns`.
    pub fn is_public_as_of(&self, as_of_ts_ns: i64) -> bool {
        self.announcement_ts_ns <= as_of_ts_ns
    }
}

/// A macroeconomic release (e.g. CPI, Non-Farm Payrolls, FOMC Rate Decision).
#[derive(Debug, Clone, PartialEq)]
pub struct MacroRelease {
    pub series_id: String,
    pub indicator_name: String,
    pub publisher: String,
    pub release_ts_ns: i64,
    pub period_covered: String,
    pub consensus: Option<f64>,
    pub actual: f64,
    pub prior: Option<f64>,
}

/// A syndicated financial headline with source attribution and publication timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItem {
    pub symbol: String,
    pub headline: String,
    pub source: String,
    pub published_ts_ns: i64,
    pub url: String,
}

/// Store and query manager for market events and macroeconomic series.
#[derive(Debug, Clone, Default)]
pub struct EventStore {
    earnings: Vec<EarningsEvent>,
    macro_releases: Vec<MacroRelease>,
    news: Vec<NewsItem>,
}

impl EventStore {
    pub fn new() -> Self {
        Self { earnings: Vec::new(), macro_releases: Vec::new(), news: Vec::new() }
    }

    pub fn add_earnings(&mut self, event: EarningsEvent) {
        self.earnings.push(event);
    }

    pub fn add_macro_release(&mut self, release: MacroRelease) {
        self.macro_releases.push(release);
    }

    pub fn add_news(&mut self, item: NewsItem) {
        self.news.push(item);
    }

    /// Queries earnings events known/announced as of `as_of_ts_ns`.
    /// Guarantees that unannounced earnings and surprises are hidden from the caller.
    pub fn get_announced_earnings(&self, symbol: &str, as_of_ts_ns: i64) -> Vec<&EarningsEvent> {
        self.earnings
            .iter()
            .filter(|e| e.symbol.eq_ignore_ascii_case(symbol) && e.is_public_as_of(as_of_ts_ns))
            .collect()
    }

    /// Checks whether an earnings release is scheduled within `[from_ts_ns, to_ts_ns]`.
    /// Used by risk strategies to avoid holding equity over binary earnings gaps.
    pub fn has_earnings_between(&self, symbol: &str, from_ts_ns: i64, to_ts_ns: i64) -> bool {
        self.earnings.iter().any(|e| {
            e.symbol.eq_ignore_ascii_case(symbol)
                && e.announcement_ts_ns >= from_ts_ns
                && e.announcement_ts_ns <= to_ts_ns
        })
    }

    /// Look up macroeconomic releases as of `as_of_ts_ns`.
    pub fn get_macro_series(&self, series_id: &str, as_of_ts_ns: i64) -> Vec<&MacroRelease> {
        self.macro_releases
            .iter()
            .filter(|m| m.series_id.eq_ignore_ascii_case(series_id) && m.release_ts_ns <= as_of_ts_ns)
            .collect()
    }

    /// Retrieve news items published on or before `as_of_ts_ns`.
    pub fn get_news_as_of(&self, symbol: &str, as_of_ts_ns: i64) -> Vec<&NewsItem> {
        self.news
            .iter()
            .filter(|n| n.symbol.eq_ignore_ascii_case(symbol) && n.published_ts_ns <= as_of_ts_ns)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_earnings_lookahead_protection() {
        let mut store = EventStore::new();
        let announcement = 1_700_000_000_000_000_000i64;

        store.add_earnings(EarningsEvent {
            symbol: "AAPL".into(),
            fiscal_period: "2024-Q3".into(),
            announcement_ts_ns: announcement,
            consensus_eps: Some(1.35),
            actual_eps: Some(1.40),
            consensus_revenue: Some(89_000_000_000.0),
            actual_revenue: Some(90_000_000_000.0),
        });

        // 1 hour before announcement: must NOT see the release or the surprise!
        let before = store.get_announced_earnings("AAPL", announcement - 3_600_000_000_000);
        assert_eq!(before.len(), 0);

        // 1 hour after announcement: available with positive surprise
        let after = store.get_announced_earnings("AAPL", announcement + 3_600_000_000_000);
        assert_eq!(after.len(), 1);
        let ev = after[0];
        assert_eq!(ev.actual_eps, Some(1.40));
        let surprise = ev.eps_surprise_pct().unwrap();
        assert!((surprise - ((1.40 - 1.35) / 1.35)).abs() < 1e-6);
    }

    #[test]
    fn test_macro_calendar_publisher_attribution() {
        let mut store = EventStore::new();
        store.add_macro_release(MacroRelease {
            series_id: "CPI_YOY".into(),
            indicator_name: "Consumer Price Index (YoY)".into(),
            publisher: "US Bureau of Labor Statistics".into(),
            release_ts_ns: 1000,
            period_covered: "2024-05".into(),
            consensus: Some(3.4),
            actual: 3.3,
            prior: Some(3.4),
        });

        let releases = store.get_macro_series("CPI_YOY", 2000);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].publisher, "US Bureau of Labor Statistics");
        assert_eq!(releases[0].actual, 3.3);
    }

    #[test]
    fn test_event_aware_backtest_filter() {
        let mut store = EventStore::new();
        store.add_earnings(EarningsEvent {
            symbol: "TSLA".into(),
            fiscal_period: "2024-Q2".into(),
            announcement_ts_ns: 5000,
            consensus_eps: None,
            actual_eps: None,
            consensus_revenue: None,
            actual_revenue: None,
        });

        // Strategy checking for upcoming earnings risk within [4000, 6000]
        assert!(store.has_earnings_between("TSLA", 4000, 6000));
        // No earnings between [1000, 3000]
        assert!(!store.has_earnings_between("TSLA", 1000, 3000));
    }
}
