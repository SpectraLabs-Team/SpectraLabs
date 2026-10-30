//! Postgres access for the Spectra Labs services: the connection pool, the
//! embedded migrations in `migrations/`, the development seed and typed
//! queries.
//!
//! Schema conventions (see the first migration): UUIDv7 ids, `timestamptz`
//! in UTC, and a `workspace_id` on every table that holds user data. Every
//! query on such a table takes the workspace id and filters by it.
//!
//! Queries use sqlx's runtime-checked API (`sqlx::query_as`), so building
//! needs no database and no offline query cache. The schema itself is checked
//! by `tests/schema_checks.sql` and by the tests in `tests/`, which run when
//! `DATABASE_URL` is set.

use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
pub use sqlx::PgPool;
use uuid::Uuid;

pub mod activity;
pub mod artifacts;
pub mod corporate_actions;
pub mod datasets;
pub mod gap;
pub mod identity;
pub mod instruments;
pub mod jobs;
pub mod lifecycle;
pub mod optimization;
pub mod paper;
pub mod providers;
pub mod reports;
pub mod risk_center;
pub mod roles;
pub mod strategies;
pub mod uploads;
pub mod vault;
pub mod web_migrations;

/// The migrations in `crates/spectra-db/migrations`, embedded at build time.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// The development seed: an admin user and a `dev` workspace.
const DEV_SEED: &str = include_str!("../seeds/dev.sql");

/// A database error.
pub type Error = sqlx::Error;

/// Pool settings.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// `postgres://user:password@host:port/database`.
    pub url: String,
    /// Largest number of open connections.
    pub max_connections: u32,
    /// How long to keep retrying the first connection (Postgres may still be
    /// starting when the service starts, as under docker compose).
    pub connect_timeout: Duration,
}

impl PoolConfig {
    /// Defaults for `url`: 10 connections, 30 s to connect.
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), max_connections: 10, connect_timeout: Duration::from_secs(30) }
    }
}

/// Opens a pool, retrying until Postgres answers or `connect_timeout` passes.
///
/// # Errors
/// The last connection error once the timeout has passed, or at once for an
/// invalid URL.
pub async fn connect(cfg: &PoolConfig) -> Result<PgPool, Error> {
    let deadline = tokio::time::Instant::now() + cfg.connect_timeout;
    loop {
        let attempt = PgPoolOptions::new()
            .max_connections(cfg.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&cfg.url)
            .await;
        match attempt {
            Ok(pool) => return Ok(pool),
            Err(e @ sqlx::Error::Configuration(_)) => return Err(e),
            Err(e) if tokio::time::Instant::now() >= deadline => return Err(e),
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

/// Applies every pending migration of the shared database: the dashboard's
/// (identity and tenancy, see [`web_migrations`]) first, then the engine's.
///
/// # Errors
/// A migration that fails, or an engine migration already applied whose file has changed.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    web_migrations::apply(pool).await?;
    MIGRATOR.run(pool).await
}

/// Checks the database answers (`SELECT 1`).
///
/// # Errors
/// Any connection or query error.
pub async fn ping(pool: &PgPool) -> Result<(), Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

/// Versions of the embedded engine migrations that the database has not
/// applied (see [`web_migrations::pending`] for the dashboard's).
///
/// # Errors
/// Any query error. A database that has never been migrated reports every
/// migration as pending.
pub async fn pending_migrations(pool: &PgPool) -> Result<Vec<i64>, Error> {
    let applied: Vec<i64> = match sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success")
        .fetch_all(pool)
        .await
    {
        Ok(v) => v,
        // 42P01: undefined_table. Never migrated.
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("42P01") => Vec::new(),
        Err(e) => return Err(e),
    };
    Ok(MIGRATOR.iter().map(|m| m.version).filter(|v| !applied.contains(v)).collect())
}

/// What [`seed_dev`] left in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seeded {
    /// The admin user.
    pub admin_id: Uuid,
    /// The `dev` workspace.
    pub workspace_id: Uuid,
}

/// The statements of `seeds/dev.sql`, without comment-only chunks.
fn seed_statements() -> impl Iterator<Item = &'static str> {
    DEV_SEED
        .split(';')
        .map(str::trim)
        .filter(|s| s.lines().any(|l| !l.trim().is_empty() && !l.trim_start().starts_with("--")))
}

/// Seeds an admin user (`admin_email`, no usable password) with a personal
/// workspace, and a `dev` workspace they own. Safe to run repeatedly.
///
/// # Errors
/// Any query error, including an invalid email.
pub async fn seed_dev(pool: &PgPool, admin_email: &str) -> Result<Seeded, Error> {
    let mut tx = pool.begin().await?;
    for stmt in seed_statements() {
        sqlx::query(stmt).bind(admin_email).execute(&mut *tx).await?;
    }
    let (admin_id, workspace_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT u.id, w.id FROM users u, workspaces w WHERE u.email = lower(btrim($1)) AND w.slug = 'dev'",
    )
    .bind(admin_email)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Seeded { admin_id, workspace_id })
}

/// A new UUIDv7 id (what the application uses for every primary key).
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_has_five_statements_all_taking_the_email() {
        let stmts: Vec<_> = seed_statements().collect();
        assert_eq!(stmts.len(), 5, "{stmts:#?}");
        assert!(stmts.iter().all(|s| s.contains("$1")));
    }

    #[test]
    fn ids_are_version_7() {
        assert_eq!(new_id().get_version_num(), 7);
    }

    #[test]
    fn migrations_are_embedded() {
        assert!(MIGRATOR.iter().count() >= 1);
    }
}
