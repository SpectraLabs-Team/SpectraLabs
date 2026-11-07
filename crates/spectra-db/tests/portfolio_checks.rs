//! Saved-portfolio tests (M15 Task 1). They need a Postgres: set `DATABASE_URL`. Without it they
//! pass without doing anything.

use spectra_db::portfolios::{create_portfolio, get_portfolio, list_portfolios};
use spectra_db::{connect, migrate, seed_dev, PoolConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

static COUNTER: AtomicU64 = AtomicU64::new(0);

async fn pool() -> Option<spectra_db::PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn name() -> String {
    format!(
        "portfolio-{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn holdings(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
    pairs.iter().map(|(s, w)| ((*s).to_string(), *w)).collect()
}

#[tokio::test]
async fn saved_portfolio_reloads_with_identical_weights() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-roundtrip@example.com").await.unwrap();
    let input = holdings(&[("AAA", 1.0 / 3.0), ("BBB", 1.0 / 3.0), ("CCC", 1.0 / 3.0)]);

    let saved =
        create_portfolio(&pool, seeded.workspace_id, &name(), &input, Some(seeded.admin_id)).await.unwrap();
    let loaded = get_portfolio(&pool, seeded.workspace_id, saved.portfolio.id).await.unwrap().unwrap();

    assert_eq!(loaded, saved);
    let got: Vec<(String, f64)> = loaded.holdings.iter().map(|h| (h.symbol.clone(), h.weight)).collect();
    assert_eq!(got, input, "weights must come back bit-for-bit identical");
}

#[tokio::test]
async fn weights_that_do_not_sum_to_one_are_refused_and_nothing_is_stored() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-badsum@example.com").await.unwrap();
    let n = name();
    let bad = holdings(&[("AAA", 0.5), ("BBB", 0.4)]);

    let err = create_portfolio(&pool, seeded.workspace_id, &n, &bad, None).await.unwrap_err();
    assert!(err.to_string().contains("sum to"), "unexpected error: {err}");

    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM portfolios WHERE workspace_id = $1 AND name = $2")
            .bind(seeded.workspace_id)
            .bind(&n)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 0, "a refused save must leave no portfolio row behind");
}

#[tokio::test]
async fn a_portfolio_is_not_visible_from_another_workspace() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-scope-a@example.com").await.unwrap();
    let other = seed_dev(&pool, "portfolio-scope-b@example.com").await.unwrap();
    // The other user's personal workspace is a different workspace from `dev`.
    let other_ws: Uuid = sqlx::query_scalar("SELECT id FROM workspaces WHERE personal_owner_id = $1")
        .bind(other.admin_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(other_ws, seeded.workspace_id);

    let saved = create_portfolio(
        &pool,
        seeded.workspace_id,
        &name(),
        &holdings(&[("AAA", 1.0)]),
        Some(seeded.admin_id),
    )
    .await
    .unwrap();

    assert!(get_portfolio(&pool, other_ws, saved.portfolio.id).await.unwrap().is_none());
    let listed = list_portfolios(&pool, other_ws).await.unwrap();
    assert!(listed.iter().all(|p| p.id != saved.portfolio.id));
}

#[tokio::test]
async fn duplicate_name_in_a_workspace_is_refused() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-dupe@example.com").await.unwrap();
    let n = name();
    create_portfolio(&pool, seeded.workspace_id, &n, &holdings(&[("AAA", 1.0)]), None).await.unwrap();
    assert!(create_portfolio(&pool, seeded.workspace_id, &n, &holdings(&[("BBB", 1.0)]), None)
        .await
        .is_err());
}
