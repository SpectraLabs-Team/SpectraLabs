//! Fundamentals refresh from SEC EDGAR (M34a).
//!
//! A `fundamentals_refresh` job fetches EDGAR's ticker map once, then each symbol's Company Facts,
//! parses them with [`spectra_data::edgar`], runs the store's quality checks and records the load
//! with [`spectra_db::fundamentals::record_load`]: new values and new revisions are added, stored
//! ones are never changed, and failed rows are quarantined. Jobs come from
//! `POST /api/v1/fundamentals/refresh` or from a workspace's refresh schedule, which the worker
//! sweeps with [`sweep_due_schedules`].
//!
//! SEC requires a descriptive `User-Agent` on every request, so a refresh fails until
//! `SEC_USER_AGENT` is set (see [`EdgarConfig::from_env`]).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use spectra_data::edgar::{company_facts_path, parse_company_facts, TickerMap};
use spectra_data::fundamentals::PointInTimeFundamentalsStore;
use spectra_db::fundamentals::{record_load, NewLoad};
use spectra_db::jobs::Job;
use spectra_db::PgPool;
use tokio::sync::Mutex;
use tracing::{error, info};
use uuid::Uuid;

/// Job kind of a fundamentals refresh.
pub const FUNDAMENTALS_REFRESH_JOB_KIND: &str = "fundamentals_refresh";
/// Source name stored with every EDGAR load and fact.
pub const EDGAR_SOURCE: &str = "edgar";
/// Where Company Facts are served.
pub const EDGAR_DATA_URL: &str = "https://data.sec.gov";
/// Where the ticker map is served.
pub const EDGAR_WWW_URL: &str = "https://www.sec.gov";
/// Path of the ticker to CIK map on [`EDGAR_WWW_URL`].
pub const TICKER_MAP_PATH: &str = "/files/company_tickers.json";
/// Time between requests: SEC asks clients to stay under 10 requests per second.
pub const MIN_REQUEST_GAP: Duration = Duration::from_millis(125);
/// Limit for one request, including reading the body.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest response read (Company Facts of the largest filers are tens of MB).
pub const MAX_RESPONSE_BYTES: usize = 128 * 1024 * 1024;
/// Most symbols in one refresh (and in a schedule).
pub const MAX_SYMBOLS: usize = 100;
/// Longest symbol accepted.
pub const MAX_SYMBOL_LEN: usize = 12;
/// Schedules claimed per sweep.
const SWEEP_BATCH: i64 = 50;

/// How to reach EDGAR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgarConfig {
    /// Sent as `User-Agent`: SEC asks for a name and a contact email.
    pub user_agent: String,
    /// Base of the Company Facts API, [`EDGAR_DATA_URL`] unless overridden.
    pub data_url: String,
    /// Base of the ticker map, [`EDGAR_WWW_URL`] unless overridden.
    pub www_url: String,
}

impl EdgarConfig {
    /// From `SEC_USER_AGENT` (required), and `SEC_EDGAR_DATA_URL` and `SEC_EDGAR_WWW_URL`
    /// (optional, for a mirror or a test server).
    ///
    /// # Errors
    /// A message naming `SEC_USER_AGENT` if it is unset or blank.
    pub fn from_env() -> Result<Self, String> {
        let var =
            |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let user_agent = var("SEC_USER_AGENT").ok_or_else(|| {
            "SEC_USER_AGENT is not set: SEC requires a User-Agent naming the company and a contact \
             email (e.g. \"SpectraLabs ops@example.com\") before fundamentals can be refreshed"
                .to_string()
        })?;
        Ok(Self {
            user_agent,
            data_url: var("SEC_EDGAR_DATA_URL").unwrap_or_else(|| EDGAR_DATA_URL.to_string()),
            www_url: var("SEC_EDGAR_WWW_URL").unwrap_or_else(|| EDGAR_WWW_URL.to_string()),
        })
    }
}

/// Why a request to EDGAR failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// 404: EDGAR has no such document.
    NotFound,
    /// Rate limited, a server error, or the network: worth retrying later.
    Retryable(String),
    /// Anything else (refused, malformed, too large).
    Permanent(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "EDGAR has no such document"),
            Self::Retryable(why) | Self::Permanent(why) => write!(f, "{why}"),
        }
    }
}

/// An EDGAR client that keeps [`MIN_REQUEST_GAP`] between its requests.
///
/// Outbound requests share a process-wide rate limiter by default so that concurrent refresh
/// jobs within the same process do not exceed SEC's limit of 10 requests per second.
///
/// # Multi-worker behavior
///
/// - **Within a single process** (e.g. concurrent jobs on the same tokio runtime):
///   All [`EdgarClient`] instances share a process-wide mutex limiter. Outbound HTTP requests
///   across all concurrent refresh jobs are strictly serialized and spaced by at least
///   [`MIN_REQUEST_GAP`] (125 ms, i.e. at most 8 req/s).
///
/// - **Across multiple worker processes** (separate OS processes or containers):
///   Separate OS address spaces have their own in-process limiters. To enforce a global rate
///   limit across multiple independent worker processes, an external coordination mechanism
///   is required (such as a shared egress proxy, a distributed token bucket, or PostgreSQL
///   advisory locks). In SpectraLabs, schedule sweeps claim due schedules using
///   `FOR UPDATE SKIP LOCKED`, ensuring that distinct worker processes handle disjoint schedules
///   and do not duplicate refreshes.
static PROCESS_EDGAR_LIMITER: std::sync::LazyLock<Arc<Mutex<Option<tokio::time::Instant>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));

/// An EDGAR client that keeps [`MIN_REQUEST_GAP`] between its requests.
#[derive(Debug, Clone)]
pub struct EdgarClient {
    http: reqwest::Client,
    cfg: EdgarConfig,
    limiter: Arc<Mutex<Option<tokio::time::Instant>>>,
}

impl EdgarClient {
    /// A client for `cfg` using the shared in-process rate limiter.
    ///
    /// # Errors
    /// If the HTTP client cannot be built.
    pub fn new(cfg: EdgarConfig) -> Result<Self, FetchError> {
        Self::with_limiter(cfg, PROCESS_EDGAR_LIMITER.clone())
    }

    /// A client for `cfg` with an explicit rate limiter (e.g. for testing isolated pacing).
    ///
    /// # Errors
    /// If the HTTP client cannot be built.
    pub fn with_limiter(
        cfg: EdgarConfig,
        limiter: Arc<Mutex<Option<tokio::time::Instant>>>,
    ) -> Result<Self, FetchError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(cfg.user_agent.clone())
            .no_proxy()
            .build()
            .map_err(|e| FetchError::Permanent(format!("cannot build the HTTP client: {e}")))?;
        Ok(Self { http, cfg, limiter })
    }

    async fn get(&self, url: &str) -> Result<String, FetchError> {
        {
            let mut last = self.limiter.lock().await;
            if let Some(t) = *last {
                tokio::time::sleep_until(t + MIN_REQUEST_GAP).await;
            }
            *last = Some(tokio::time::Instant::now());
        }
        let resp =
            self.http.get(url).send().await.map_err(|e| FetchError::Retryable(format!("{url}: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(FetchError::NotFound);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(FetchError::Retryable(format!("{url}: EDGAR answered {status}")));
        }
        if !status.is_success() {
            return Err(FetchError::Permanent(format!(
                "{url}: EDGAR answered {status} (SEC refuses requests without a valid SEC_USER_AGENT)"
            )));
        }
        let body = resp.bytes().await.map_err(|e| FetchError::Retryable(format!("{url}: {e}")))?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(FetchError::Permanent(format!(
                "{url}: response larger than {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        String::from_utf8(body.to_vec())
            .map_err(|_| FetchError::Permanent(format!("{url}: response is not UTF-8")))
    }

    /// EDGAR's ticker to CIK map.
    ///
    /// # Errors
    /// [`FetchError`] if it cannot be fetched or read.
    pub async fn ticker_map(&self) -> Result<TickerMap, FetchError> {
        let url = format!("{}{TICKER_MAP_PATH}", self.cfg.www_url.trim_end_matches('/'));
        let text = self.get(&url).await?;
        TickerMap::parse(&text).map_err(|e| FetchError::Permanent(e.to_string()))
    }

    /// The Company Facts JSON of `cik`.
    ///
    /// # Errors
    /// [`FetchError`] if it cannot be fetched.
    pub async fn company_facts(&self, cik: u64) -> Result<String, FetchError> {
        self.get(&format!("{}{}", self.cfg.data_url.trim_end_matches('/'), company_facts_path(cik))).await
    }
}

/// Symbols as stored: trimmed, upper-case, duplicates removed (first kept).
///
/// # Errors
/// A message if there are none, more than [`MAX_SYMBOLS`], or one that is empty, longer than
/// [`MAX_SYMBOL_LEN`] or not made of letters, digits, `.` and `-`.
pub fn validate_symbols(symbols: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::with_capacity(symbols.len());
    for raw in symbols {
        let s = raw.trim().to_ascii_uppercase();
        if s.is_empty()
            || s.len() > MAX_SYMBOL_LEN
            || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return Err(format!(
                "symbol {raw:?} is not a ticker: use 1 to {MAX_SYMBOL_LEN} letters, digits, '.' or '-'"
            ));
        }
        if !out.contains(&s) {
            out.push(s);
        }
    }
    if out.is_empty() || out.len() > MAX_SYMBOLS {
        return Err(format!("give 1 to {MAX_SYMBOLS} symbols, got {}", out.len()));
    }
    Ok(out)
}

/// Payload of a `fundamentals_refresh` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshPayload {
    /// Symbols to refresh.
    pub symbols: Vec<String>,
    /// The schedule that queued it, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_id: Option<Uuid>,
}

/// How a refresh ended.
#[derive(Debug)]
pub enum RefreshOutcome {
    /// At least one symbol loaded; the result lists every symbol.
    Done(Value),
    /// Cancelled between symbols.
    Cancelled,
    /// Nothing loaded.
    Failed {
        /// What went wrong.
        message: String,
        /// Worth trying again (rate limits, EDGAR or database outages).
        retryable: bool,
    },
}

/// Runs a `fundamentals_refresh` job. `edgar` is the EDGAR configuration, or why there is none
/// (see [`EdgarConfig::from_env`]), which fails the job permanently with that explanation.
pub async fn run_refresh(
    pool: &PgPool,
    edgar: Result<&EdgarConfig, &str>,
    job: &Job,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
) -> RefreshOutcome {
    let set_progress = |p: f32| progress.store(p.to_bits(), Ordering::Relaxed);
    let failed = |message: String, retryable: bool| RefreshOutcome::Failed { message, retryable };
    let payload: RefreshPayload = match serde_json::from_value(job.payload.clone()) {
        Ok(p) => p,
        Err(e) => return failed(format!("invalid fundamentals_refresh payload: {e}"), false),
    };
    let symbols = match validate_symbols(&payload.symbols) {
        Ok(s) => s,
        Err(e) => return failed(e, false),
    };
    let cfg = match edgar {
        Ok(cfg) => cfg.clone(),
        Err(why) => return failed(why.to_string(), false),
    };
    let client = match EdgarClient::new(cfg) {
        Ok(c) => c,
        Err(e) => return failed(e.to_string(), false),
    };
    let map = match client.ticker_map().await {
        Ok(m) => m,
        Err(FetchError::Retryable(why)) => return failed(format!("EDGAR ticker map: {why}"), true),
        Err(e) => return failed(format!("EDGAR ticker map: {e}"), false),
    };
    set_progress(0.05);

    let (mut results, mut loaded, mut retryable) = (Vec::with_capacity(symbols.len()), 0usize, false);
    for (i, symbol) in symbols.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return RefreshOutcome::Cancelled;
        }
        let step = refresh_symbol(pool, &client, &map, job, symbol).await;
        match step {
            Ok(entry) => {
                loaded += 1;
                results.push(entry);
            }
            Err((why, again)) => {
                retryable |= again;
                results.push(json!({ "symbol": symbol, "error": why }));
            }
        }
        #[allow(clippy::cast_precision_loss)] // at most MAX_SYMBOLS
        set_progress(0.05 + 0.95 * (i + 1) as f32 / symbols.len() as f32);
    }
    if loaded == 0 {
        let first = results.first().and_then(|r| r["error"].as_str()).map(str::to_string);
        return failed(format!("no symbol loaded: {}", first.as_deref().unwrap_or("no symbols")), retryable);
    }
    info!(job_id = %job.id, loaded, failed = symbols.len() - loaded, "fundamentals refresh finished");
    RefreshOutcome::Done(json!({
        "source": EDGAR_SOURCE,
        "loaded": loaded,
        "failed": symbols.len() - loaded,
        "symbols": results,
    }))
}

/// Refreshes one symbol: its load summary, or why it failed and whether that is worth retrying.
async fn refresh_symbol(
    pool: &PgPool,
    client: &EdgarClient,
    map: &TickerMap,
    job: &Job,
    symbol: &str,
) -> Result<Value, (String, bool)> {
    let cik = map.cik(symbol).ok_or_else(|| (format!("{symbol} is not in EDGAR's ticker list"), false))?;
    let text = match client.company_facts(cik).await {
        Ok(t) => t,
        Err(FetchError::NotFound) => return Err((format!("EDGAR has no Company Facts for {symbol}"), false)),
        Err(FetchError::Retryable(why)) => return Err((why, true)),
        Err(FetchError::Permanent(why)) => return Err((why, false)),
    };
    let records = parse_company_facts(&text, symbol).map_err(|e| (e.to_string(), false))?;
    let mut store = PointInTimeFundamentalsStore::new();
    store.ingest(records);
    let version_id = store.dataset_version_id();
    let row = record_load(
        pool,
        &NewLoad {
            workspace_id: job.workspace_id,
            symbol,
            source: EDGAR_SOURCE,
            version_id: &version_id,
            accepted: store.records(),
            quarantined: store.quarantined_records(),
            job_id: Some(job.id),
            created_by: job.created_by,
        },
    )
    .await
    .map_err(|e| (format!("could not store {symbol}: {e}"), true))?;
    Ok(json!({
        "symbol": symbol,
        "cik": cik,
        "load_id": row.id,
        "version_id": row.version_id,
        "accepted": row.accepted,
        "new_rows": row.new_rows,
        "quarantined": row.quarantined,
    }))
}

/// Enqueues a refresh for every schedule that is due, moving each one's next run on by its
/// interval (see [`spectra_db::fundamentals::claim_due_schedules`]).
pub async fn sweep_due_schedules(pool: &PgPool) {
    let due = match spectra_db::fundamentals::claim_due_schedules(pool, Utc::now(), SWEEP_BATCH).await {
        Ok(d) => d,
        Err(e) => {
            error!("could not claim due fundamentals schedules: {e}");
            return;
        }
    };
    for s in due {
        let job = spectra_db::jobs::NewJob {
            workspace_id: s.workspace_id,
            kind: FUNDAMENTALS_REFRESH_JOB_KIND.to_string(),
            payload: json!(RefreshPayload { symbols: s.symbols.clone(), schedule_id: Some(s.id) }),
            priority: 5,
            max_attempts: 3,
            created_by: s.created_by,
        };
        match spectra_db::jobs::enqueue(pool, &job).await {
            Ok(j) => info!(schedule_id = %s.id, job_id = %j.id, "enqueued scheduled fundamentals refresh"),
            Err(e) => error!(schedule_id = %s.id, "could not enqueue scheduled fundamentals refresh: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_normalized_and_checked() {
        let s = |v: &[&str]| validate_symbols(&v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>());
        assert_eq!(s(&[" aapl", "MSFT", "AAPL", "brk.b"]).unwrap(), ["AAPL", "MSFT", "BRK.B"]);
        assert!(s(&[]).is_err());
        assert!(s(&[""]).is_err());
        assert!(s(&["AAPL;DROP"]).is_err());
        assert!(s(&["ABCDEFGHIJKLM"]).is_err());
        let many: Vec<String> = (0..=MAX_SYMBOLS).map(|i| format!("S{i}")).collect();
        assert!(validate_symbols(&many).is_err());
    }
}
