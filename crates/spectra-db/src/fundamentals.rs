//! Fundamentals loaded from a provider, stored point in time (M34).
//!
//! A refresh records one [`LoadRow`] per symbol, adds the facts it has not seen before to
//! `fundamental_facts` (append-only: a stored value is never changed) and keeps rows that failed
//! the quality checks in `fundamental_quarantine`, which readers never query. Readers ask for what
//! was known at a time: [`facts_as_of`] for the latest revision of each value, [`facts_known_by`]
//! for every revision (to build a [`PointInTimeFundamentalsStore`]).
//!
//! [`PointInTimeFundamentalsStore`]: spectra_data::fundamentals::PointInTimeFundamentalsStore

use chrono::{DateTime, Utc};
use spectra_data::fundamentals::{FundamentalRecord, QuarantinedFundamental};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::Error;

/// One stored revision of one value.
#[derive(Debug, Clone, PartialEq, FromRow, serde::Serialize)]
pub struct FactRow {
    pub symbol: String,
    pub period: String,
    pub fiscal_year: i16,
    pub fiscal_quarter: i16,
    pub metric: String,
    pub value: f64,
    pub currency: String,
    /// When the value became public.
    pub known_at: DateTime<Utc>,
    pub revision: i32,
    pub filing_ref: String,
    pub source: String,
    pub load_id: Uuid,
    pub loaded_at: DateTime<Utc>,
}

impl FactRow {
    /// The row as a store record; `None` only for a `known_at` outside the nanosecond range
    /// (years 1677 to 2262) or a negative year, quarter or revision, which the table does not hold.
    pub fn to_record(&self) -> Option<FundamentalRecord> {
        Some(FundamentalRecord {
            symbol: self.symbol.clone(),
            period: self.period.clone(),
            fiscal_year: u16::try_from(self.fiscal_year).ok()?,
            fiscal_quarter: u8::try_from(self.fiscal_quarter).ok()?,
            metric: self.metric.clone(),
            value: self.value,
            currency: self.currency.clone(),
            publication_ts_ns: self.known_at.timestamp_nanos_opt()?,
            restatement_index: u32::try_from(self.revision).ok()?,
            filing_ref: self.filing_ref.clone(),
        })
    }
}

/// One refresh of one symbol.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, serde::Serialize)]
pub struct LoadRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub symbol: String,
    pub source: String,
    /// Content-addressed version (`fund_v_…`) of the accepted records.
    pub version_id: String,
    /// Records that passed the checks.
    pub accepted: i32,
    /// Accepted records not stored before (new values and new revisions).
    pub new_rows: i32,
    /// Records that failed the checks.
    pub quarantined: i32,
    pub job_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub loaded_at: DateTime<Utc>,
}

/// A row that failed the quality checks.
#[derive(Debug, Clone, PartialEq, FromRow, serde::Serialize)]
pub struct QuarantineRow {
    pub symbol: String,
    pub period: String,
    pub metric: String,
    /// As the provider sent it (may be NaN or infinite).
    pub value: f64,
    pub known_at: DateTime<Utc>,
    pub filing_ref: String,
    pub category: String,
    pub reason: String,
    pub load_id: Uuid,
    pub recorded_at: DateTime<Utc>,
}

/// A workspace's refresh schedule.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, serde::Serialize)]
pub struct ScheduleRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub symbols: Vec<String>,
    pub interval_seconds: i32,
    pub next_run_at: DateTime<Utc>,
    pub last_enqueued_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What one refresh of one symbol produced.
#[derive(Debug, Clone, Copy)]
pub struct NewLoad<'a> {
    pub workspace_id: Uuid,
    /// The symbol every record belongs to.
    pub symbol: &'a str,
    /// Provider name, e.g. `edgar`.
    pub source: &'a str,
    /// Content-addressed version of `accepted` (`PointInTimeFundamentalsStore::dataset_version_id`).
    pub version_id: &'a str,
    pub accepted: &'a [FundamentalRecord],
    pub quarantined: &'a [QuarantinedFundamental],
    pub job_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

const FACT_COLUMNS: &str = "symbol, period, fiscal_year, fiscal_quarter, metric, value, currency, \
     known_at, revision, filing_ref, source, load_id, loaded_at";

/// A symbol as stored: trimmed and upper-case.
fn normalize(symbol: &str) -> String {
    symbol.trim().to_ascii_uppercase()
}

fn known_at(ns: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_nanos(ns)
}

fn encode<T, E: std::error::Error + Send + Sync + 'static>(r: Result<T, E>) -> Result<T, Error> {
    r.map_err(|e| Error::Encode(Box::new(e)))
}

/// Records one refresh: the load, the facts not stored before, and the quarantined rows, in one
/// transaction. Facts already stored (same symbol, period, metric and filing) are left as they are.
///
/// # Errors
/// Database errors, or [`Error::Encode`] for a fiscal year or revision too large for its column.
pub async fn record_load(pool: &PgPool, load: &NewLoad<'_>) -> Result<LoadRow, Error> {
    let symbol = normalize(load.symbol);
    let accepted = encode(i32::try_from(load.accepted.len()))?;
    let quarantined = encode(i32::try_from(load.quarantined.len()))?;
    let mut tx = pool.begin().await?;

    let (load_id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO fundamental_loads \
             (workspace_id, symbol, source, version_id, accepted, new_rows, quarantined, job_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, 0, $6, $7, $8) RETURNING id",
    )
    .bind(load.workspace_id)
    .bind(&symbol)
    .bind(load.source)
    .bind(load.version_id)
    .bind(accepted)
    .bind(quarantined)
    .bind(load.job_id)
    .bind(load.created_by)
    .fetch_one(&mut *tx)
    .await?;

    let n = load.accepted.len();
    let (mut periods, mut years, mut quarters, mut metrics, mut values) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    let (mut currencies, mut known, mut revisions, mut filings) =
        (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for r in load.accepted {
        periods.push(r.period.clone());
        years.push(encode(i16::try_from(r.fiscal_year))?);
        quarters.push(i16::from(r.fiscal_quarter));
        metrics.push(r.metric.clone());
        values.push(r.value);
        currencies.push(r.currency.clone());
        known.push(known_at(r.publication_ts_ns));
        revisions.push(encode(i32::try_from(r.restatement_index))?);
        filings.push(r.filing_ref.clone());
    }
    let new_rows = sqlx::query(
        "INSERT INTO fundamental_facts \
             (workspace_id, load_id, symbol, source, period, fiscal_year, fiscal_quarter, metric, value, \
              currency, known_at, revision, filing_ref) \
         SELECT $1, $2, $3, $4, * FROM UNNEST($5::text[], $6::smallint[], $7::smallint[], $8::text[], \
              $9::float8[], $10::text[], $11::timestamptz[], $12::int[], $13::text[]) \
         ON CONFLICT ON CONSTRAINT uq_fundamental_facts_filing DO NOTHING",
    )
    .bind(load.workspace_id)
    .bind(load_id)
    .bind(&symbol)
    .bind(load.source)
    .bind(&periods)
    .bind(&years)
    .bind(&quarters)
    .bind(&metrics)
    .bind(&values)
    .bind(&currencies)
    .bind(&known)
    .bind(&revisions)
    .bind(&filings)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let q = load.quarantined;
    sqlx::query(
        "INSERT INTO fundamental_quarantine \
             (workspace_id, load_id, symbol, period, metric, value, known_at, filing_ref, category, reason) \
         SELECT $1, $2, $3, * FROM UNNEST($4::text[], $5::text[], $6::float8[], $7::timestamptz[], \
              $8::text[], $9::text[], $10::text[]) \
         ON CONFLICT ON CONSTRAINT uq_fundamental_quarantine_filing DO NOTHING",
    )
    .bind(load.workspace_id)
    .bind(load_id)
    .bind(&symbol)
    .bind(q.iter().map(|x| x.record.period.clone()).collect::<Vec<_>>())
    .bind(q.iter().map(|x| x.record.metric.clone()).collect::<Vec<_>>())
    .bind(q.iter().map(|x| x.record.value).collect::<Vec<_>>())
    .bind(q.iter().map(|x| known_at(x.record.publication_ts_ns)).collect::<Vec<_>>())
    .bind(q.iter().map(|x| x.record.filing_ref.clone()).collect::<Vec<_>>())
    .bind(q.iter().map(|x| x.category.clone()).collect::<Vec<_>>())
    .bind(q.iter().map(|x| x.reason.clone()).collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;

    let row: LoadRow = sqlx::query_as("UPDATE fundamental_loads SET new_rows = $2 WHERE id = $1 RETURNING *")
        .bind(load_id)
        .bind(encode(i32::try_from(new_rows))?)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(row)
}

/// The latest revision of each value of `symbol` known at `as_of` (optionally one metric only),
/// sorted by period and metric. Values first published after `as_of` are not returned, and a
/// revision published after `as_of` is replaced by the one before it.
///
/// # Errors
/// Database error.
pub async fn facts_as_of(
    pool: &PgPool,
    workspace_id: Uuid,
    symbol: &str,
    as_of: DateTime<Utc>,
    metric: Option<&str>,
) -> Result<Vec<FactRow>, Error> {
    sqlx::query_as(&format!(
        "SELECT DISTINCT ON (period, metric) {FACT_COLUMNS} FROM fundamental_facts \
         WHERE workspace_id = $1 AND symbol = $2 AND known_at <= $3 AND ($4::text IS NULL OR metric = $4) \
         ORDER BY period, metric, known_at DESC, revision DESC"
    ))
    .bind(workspace_id)
    .bind(normalize(symbol))
    .bind(as_of)
    .bind(metric)
    .fetch_all(pool)
    .await
}

/// Every revision of `symbols` known at `as_of`, for loading a point-in-time store.
///
/// # Errors
/// Database error.
pub async fn facts_known_by(
    pool: &PgPool,
    workspace_id: Uuid,
    symbols: &[String],
    as_of: DateTime<Utc>,
) -> Result<Vec<FactRow>, Error> {
    let symbols: Vec<String> = symbols.iter().map(|s| normalize(s)).collect();
    sqlx::query_as(&format!(
        "SELECT {FACT_COLUMNS} FROM fundamental_facts \
         WHERE workspace_id = $1 AND symbol = ANY($2) AND known_at <= $3 \
         ORDER BY symbol, period, metric, known_at, revision"
    ))
    .bind(workspace_id)
    .bind(&symbols)
    .bind(as_of)
    .fetch_all(pool)
    .await
}

/// Recent loads, newest first, optionally for one symbol.
///
/// # Errors
/// Database error.
pub async fn list_loads(
    pool: &PgPool,
    workspace_id: Uuid,
    symbol: Option<&str>,
    limit: i64,
) -> Result<Vec<LoadRow>, Error> {
    sqlx::query_as(
        "SELECT * FROM fundamental_loads WHERE workspace_id = $1 AND ($2::text IS NULL OR symbol = $2) \
         ORDER BY loaded_at DESC, id DESC LIMIT $3",
    )
    .bind(workspace_id)
    .bind(symbol.map(normalize))
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Quarantined rows, newest first, optionally for one symbol.
///
/// # Errors
/// Database error.
pub async fn list_quarantine(
    pool: &PgPool,
    workspace_id: Uuid,
    symbol: Option<&str>,
    limit: i64,
) -> Result<Vec<QuarantineRow>, Error> {
    sqlx::query_as(
        "SELECT symbol, period, metric, value, known_at, filing_ref, category, reason, load_id, recorded_at \
         FROM fundamental_quarantine WHERE workspace_id = $1 AND ($2::text IS NULL OR symbol = $2) \
         ORDER BY recorded_at DESC, id DESC LIMIT $3",
    )
    .bind(workspace_id)
    .bind(symbol.map(normalize))
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Creates or replaces the workspace's refresh schedule; the first refresh is due at `next_run_at`.
///
/// # Errors
/// Database error, including a check violation for no symbols, more than 100, or an interval
/// outside one hour to 30 days.
pub async fn upsert_schedule(
    pool: &PgPool,
    workspace_id: Uuid,
    symbols: &[String],
    interval_seconds: i32,
    next_run_at: DateTime<Utc>,
    created_by: Option<Uuid>,
) -> Result<ScheduleRow, Error> {
    let symbols: Vec<String> = symbols.iter().map(|s| normalize(s)).collect();
    sqlx::query_as(
        "INSERT INTO fundamental_refresh_schedules (workspace_id, symbols, interval_seconds, next_run_at, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (workspace_id) DO UPDATE SET symbols = EXCLUDED.symbols, \
             interval_seconds = EXCLUDED.interval_seconds, next_run_at = EXCLUDED.next_run_at, \
             updated_at = clock_timestamp() \
         RETURNING *",
    )
    .bind(workspace_id)
    .bind(&symbols)
    .bind(interval_seconds)
    .bind(next_run_at)
    .bind(created_by)
    .fetch_one(pool)
    .await
}

/// The workspace's refresh schedule, if it has one.
///
/// # Errors
/// Database error.
pub async fn get_schedule(pool: &PgPool, workspace_id: Uuid) -> Result<Option<ScheduleRow>, Error> {
    sqlx::query_as("SELECT * FROM fundamental_refresh_schedules WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
}

/// Removes the workspace's refresh schedule; `false` if it had none.
///
/// # Errors
/// Database error.
pub async fn delete_schedule(pool: &PgPool, workspace_id: Uuid) -> Result<bool, Error> {
    let done = sqlx::query("DELETE FROM fundamental_refresh_schedules WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(pool)
        .await?;
    Ok(done.rows_affected() > 0)
}

/// Claims up to `limit` schedules due at `now`: each one's `next_run_at` moves on by its interval
/// in the same statement, so concurrent workers never claim the same run. The caller enqueues a
/// refresh for each returned schedule.
///
/// # Errors
/// Database error.
pub async fn claim_due_schedules(
    pool: &PgPool,
    now: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<ScheduleRow>, Error> {
    sqlx::query_as(
        "UPDATE fundamental_refresh_schedules s \
         SET next_run_at = $1 + make_interval(secs => s.interval_seconds), last_enqueued_at = $1 \
         WHERE s.id IN (SELECT id FROM fundamental_refresh_schedules WHERE next_run_at <= $1 \
                        ORDER BY next_run_at LIMIT $2 FOR UPDATE SKIP LOCKED) \
         RETURNING s.*",
    )
    .bind(now)
    .bind(limit)
    .fetch_all(pool)
    .await
}
