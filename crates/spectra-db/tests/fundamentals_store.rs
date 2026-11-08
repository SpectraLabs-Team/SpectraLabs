//! Point-in-time fundamentals storage (M34). They need a Postgres: set `DATABASE_URL`. Without it
//! they pass without doing anything.

use chrono::{DateTime, Duration, Utc};
use spectra_data::fundamentals::{FundamentalRecord, PointInTimeFundamentalsStore};
use spectra_db::fundamentals::{
    claim_due_schedules, delete_schedule, facts_as_of, facts_known_by, get_schedule, list_loads,
    list_quarantine, record_load, upsert_schedule, NewLoad,
};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

/// A new workspace of its own, so tests do not see each other's rows.
async fn workspace(pool: &PgPool) -> Uuid {
    let slug = format!("m34-{}", Uuid::now_v7().simple());
    sqlx::query_scalar("INSERT INTO workspaces (slug, name) VALUES ($1, $1) RETURNING id")
        .bind(&slug)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// 2024-02-03 12:00 UTC (a filing of 2024-02-02), and 2024-03-16 12:00 UTC (one of 2024-03-15).
const FILED_FEB: i64 = 1_706_961_600_000_000_000;
const FILED_MAR: i64 = 1_710_590_400_000_000_000;

fn record(metric: &str, value: f64, revision: u32, known_at: i64, filing: &str) -> FundamentalRecord {
    FundamentalRecord {
        symbol: "EXAMPLE".into(),
        period: "2024-Q1".into(),
        fiscal_year: 2024,
        fiscal_quarter: 1,
        metric: metric.into(),
        value,
        currency: "USD".into(),
        publication_ts_ns: known_at,
        restatement_index: revision,
        filing_ref: filing.into(),
    }
}

fn at(ns: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_nanos(ns)
}

/// Runs records through the store's quality checks and records them as one load.
async fn load(pool: &PgPool, ws: Uuid, records: Vec<FundamentalRecord>) -> spectra_db::fundamentals::LoadRow {
    let mut store = PointInTimeFundamentalsStore::new();
    store.ingest(records);
    record_load(
        pool,
        &NewLoad {
            workspace_id: ws,
            symbol: "example",
            source: "edgar",
            version_id: &store.dataset_version_id(),
            accepted: store.records(),
            quarantined: store.quarantined_records(),
            job_id: None,
            created_by: None,
        },
    )
    .await
    .unwrap()
}

fn revenue(rows: &[spectra_db::fundamentals::FactRow]) -> Option<f64> {
    rows.iter().find(|r| r.metric == "revenue").map(|r| r.value)
}

#[tokio::test]
async fn a_refresh_adds_revisions_and_keeps_the_old_ones() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;

    let first = load(&pool, ws, vec![record("revenue", 100.0, 0, FILED_FEB, "A1")]).await;
    assert_eq!((first.symbol.as_str(), first.accepted, first.new_rows), ("EXAMPLE", 1, 1));
    assert!(first.version_id.starts_with("fund_v_"));

    // The provider now also reports a restatement: only the restatement is new.
    let second = load(
        &pool,
        ws,
        vec![
            record("revenue", 100.0, 0, FILED_FEB, "A1"),
            record("revenue", 105.0, 1, FILED_MAR, "A1-AMEND"),
        ],
    )
    .await;
    assert_eq!((second.accepted, second.new_rows), (2, 1));
    assert_ne!(first.version_id, second.version_id);

    // Reloading the same thing adds nothing, and the stored rows are unchanged.
    let third = load(
        &pool,
        ws,
        vec![
            record("revenue", 100.0, 0, FILED_FEB, "A1"),
            record("revenue", 105.0, 1, FILED_MAR, "A1-AMEND"),
        ],
    )
    .await;
    assert_eq!(third.new_rows, 0);
    assert_eq!(third.version_id, second.version_id);

    let all = facts_known_by(&pool, ws, &["EXAMPLE".into()], at(FILED_MAR)).await.unwrap();
    assert_eq!(all.iter().map(|r| (r.revision, r.value)).collect::<Vec<_>>(), [(0, 100.0), (1, 105.0)]);
    assert_eq!(list_loads(&pool, ws, Some("example"), 10).await.unwrap().len(), 3);
}

#[tokio::test]
async fn reads_see_only_what_was_known_at_the_time() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    load(
        &pool,
        ws,
        vec![
            record("revenue", 100.0, 0, FILED_FEB, "A1"),
            record("revenue", 105.0, 1, FILED_MAR, "A1-AMEND"),
        ],
    )
    .await;

    let as_of = |ns| facts_as_of(&pool, ws, "EXAMPLE", at(ns), None);
    // One nanosecond before the original was known: nothing.
    assert_eq!(as_of(FILED_FEB - 1).await.unwrap(), vec![]);
    assert_eq!(revenue(&as_of(FILED_FEB).await.unwrap()), Some(100.0));
    assert_eq!(revenue(&as_of(FILED_MAR - 1).await.unwrap()), Some(100.0));
    assert_eq!(revenue(&as_of(FILED_MAR).await.unwrap()), Some(105.0));
    // A metric filter, and the revision that won.
    let rows = facts_as_of(&pool, ws, "example", at(FILED_MAR), Some("revenue")).await.unwrap();
    assert_eq!((rows.len(), rows[0].revision, rows[0].filing_ref.as_str()), (1, 1, "A1-AMEND"));

    // A store built from what was known in February answers like the database.
    let mut store = PointInTimeFundamentalsStore::new();
    let feb = facts_known_by(&pool, ws, &["EXAMPLE".into()], at(FILED_FEB)).await.unwrap();
    store.ingest(feb.iter().map(|r| r.to_record().unwrap()).collect());
    assert_eq!(
        store.query_point_in_time("EXAMPLE", "2024-Q1", "revenue", i64::MAX).map(|r| r.value),
        Some(100.0)
    );
}

#[tokio::test]
async fn quarantined_rows_are_kept_apart_from_what_readers_see() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    let loaded = load(
        &pool,
        ws,
        vec![
            record("revenue", -5.0, 0, FILED_FEB, "A1"),
            record("net_income", f64::NAN, 0, FILED_FEB, "A1"),
            record("net_income", -2.0, 0, FILED_FEB, "A2"),
        ],
    )
    .await;
    assert_eq!((loaded.accepted, loaded.new_rows, loaded.quarantined), (1, 1, 2));

    let known = facts_known_by(&pool, ws, &["EXAMPLE".into()], at(i64::MAX)).await.unwrap();
    assert_eq!(
        known.iter().map(|r| (r.metric.as_str(), r.value)).collect::<Vec<_>>(),
        [("net_income", -2.0)]
    );
    let quarantine = list_quarantine(&pool, ws, None, 10).await.unwrap();
    let mut categories: Vec<&str> = quarantine.iter().map(|q| q.category.as_str()).collect();
    categories.sort_unstable();
    assert_eq!(categories, ["IMPOSSIBLE_NEGATIVE", "NON_FINITE"]);
    assert!(quarantine.iter().any(|q| q.value.is_nan()), "the NaN is kept as sent, for inspection");
}

#[tokio::test]
async fn stored_values_cannot_be_changed() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    load(&pool, ws, vec![record("revenue", 100.0, 0, FILED_FEB, "A1")]).await;
    let err = sqlx::query("UPDATE fundamental_facts SET value = 1 WHERE workspace_id = $1")
        .bind(ws)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("append-only"), "{err}");
    // Deleting the workspace still removes its facts.
    sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(ws).execute(&pool).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM fundamental_facts WHERE workspace_id = $1")
        .bind(ws)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn workspaces_do_not_see_each_others_fundamentals() {
    let Some(pool) = pool().await else { return };
    let (mine, theirs) = (workspace(&pool).await, workspace(&pool).await);
    load(&pool, mine, vec![record("revenue", 100.0, 0, FILED_FEB, "A1")]).await;
    assert_eq!(facts_as_of(&pool, theirs, "EXAMPLE", at(i64::MAX), None).await.unwrap(), vec![]);
    assert_eq!(list_loads(&pool, theirs, None, 10).await.unwrap(), vec![]);
}

#[tokio::test]
async fn a_due_schedule_is_claimed_once_per_interval() {
    let Some(pool) = pool().await else { return };
    let ws = workspace(&pool).await;
    // Postgres keeps microseconds.
    let now = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap();
    let s = upsert_schedule(&pool, ws, &[" aapl ".into(), "msft".into()], 86_400, now, None).await.unwrap();
    assert_eq!(s.symbols, ["AAPL", "MSFT"]);

    let mine = |rows: Vec<spectra_db::fundamentals::ScheduleRow>| {
        rows.into_iter().filter(|r| r.workspace_id == ws).count()
    };
    assert_eq!(mine(claim_due_schedules(&pool, now, 1000).await.unwrap()), 1);
    // Claiming moved next_run_at a day on, so a second claim now finds nothing.
    assert_eq!(mine(claim_due_schedules(&pool, now, 1000).await.unwrap()), 0);
    let after = get_schedule(&pool, ws).await.unwrap().unwrap();
    assert_eq!(after.next_run_at, now + Duration::days(1));
    assert_eq!(mine(claim_due_schedules(&pool, now + Duration::days(1), 1000).await.unwrap()), 1);

    // Out-of-range intervals are refused by the table.
    assert!(upsert_schedule(&pool, ws, &["AAPL".into()], 60, now, None).await.is_err());
    assert!(delete_schedule(&pool, ws).await.unwrap());
    assert!(!delete_schedule(&pool, ws).await.unwrap());
}
