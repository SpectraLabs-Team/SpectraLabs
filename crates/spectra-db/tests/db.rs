//! Database tests. They need a Postgres to own: set `DATABASE_URL` (CI runs a
//! Postgres service). Without it they pass without doing anything, so
//! `cargo test --workspace` still works on a laptop with no database.

use spectra_db::{connect, migrate, pending_migrations, ping, seed_dev, PoolConfig};

async fn pool() -> Option<spectra_db::PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

#[tokio::test]
async fn migrations_apply_and_leave_nothing_pending() {
    let Some(pool) = pool().await else { return };
    ping(&pool).await.unwrap();
    assert!(pending_migrations(&pool).await.unwrap().is_empty());
    assert!(
        spectra_db::web_migrations::pending(&pool).await.unwrap().is_empty(),
        "dashboard migrations applied first"
    );
    // Running them again is a no-op.
    migrate(&pool).await.unwrap();
}

#[tokio::test]
async fn the_dev_seed_is_idempotent() {
    let Some(pool) = pool().await else { return };
    let a = seed_dev(&pool, " Seed-Test@Example.com ").await.unwrap();
    let b = seed_dev(&pool, "seed-test@example.com").await.unwrap();
    assert_eq!(a, b);
    let role: String =
        sqlx::query_scalar("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
            .bind(a.workspace_id)
            .bind(a.admin_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(role, "owner");
    // Like a dashboard sign-up: the admin also has exactly one personal workspace they own.
    let personal: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE w.personal_owner_id = $1 AND m.user_id = $1 AND m.role = 'owner'",
    )
    .bind(a.admin_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(personal, 1);
}

#[tokio::test]
async fn database_ids_are_uuid_v7() {
    let Some(pool) = pool().await else { return };
    let id: uuid::Uuid = sqlx::query_scalar("SELECT spectra_uuidv7()").fetch_one(&pool).await.unwrap();
    assert_eq!(id.get_version_num(), 7);
}

#[tokio::test]
async fn queue_depth_counts_newly_enqueued_work() {
    let Some(pool) = pool().await else { return };
    let seed = seed_dev(&pool, "queue-metrics@example.test").await.unwrap();
    let before = spectra_db::jobs::queue_depth(&pool).await.unwrap();
    let job = spectra_db::jobs::enqueue(
        &pool,
        &spectra_db::jobs::NewJob {
            workspace_id: seed.workspace_id,
            kind: "risk".into(),
            payload: serde_json::json!({}),
            priority: 0,
            max_attempts: 1,
            created_by: Some(seed.admin_id),
        },
    )
    .await
    .unwrap();
    let after = spectra_db::jobs::queue_depth(&pool).await.unwrap();
    sqlx::query("DELETE FROM jobs WHERE id = $1").bind(job.id).execute(&pool).await.unwrap();
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.1, before.1);
}
