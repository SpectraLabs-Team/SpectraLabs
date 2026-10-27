//! The dashboard's migrations (`web/frontend/db/migrations`), embedded so that
//! `spectra-db migrate` brings the whole shared database up to date in order:
//! identity and tenancy first (users, workspaces, sessions, ...), then the engine.
//!
//! Applied exactly as `web/frontend/scripts/db-migrate.mjs` applies them, so the two
//! runners interoperate on one database: the same `schema_migrations` table, the same
//! SHA-256 checksum of each file, one transaction per file, and the same advisory lock.
//! Whichever runs first applies a file; the other sees it recorded and skips it.

use sqlx::{Connection, PgPool};

/// `db-migrate.mjs` LOCK_ID: only one migrator at a time, whichever tool it is.
const LOCK_ID: i64 = 7_312_019;

/// (file name, SQL) in apply order. A test checks this matches the directory.
pub static WEB_MIGRATIONS: &[(&str, &str)] = &[
    ("001_auth.sql", include_str!("../../../web/frontend/db/migrations/001_auth.sql")),
    ("002_sessions.sql", include_str!("../../../web/frontend/db/migrations/002_sessions.sql")),
    ("003_workspaces.sql", include_str!("../../../web/frontend/db/migrations/003_workspaces.sql")),
    ("004_rate_limits.sql", include_str!("../../../web/frontend/db/migrations/004_rate_limits.sql")),
    ("005_secrets.sql", include_str!("../../../web/frontend/db/migrations/005_secrets.sql")),
    ("006_audit.sql", include_str!("../../../web/frontend/db/migrations/006_audit.sql")),
    ("007_mfa.sql", include_str!("../../../web/frontend/db/migrations/007_mfa.sql")),
    ("008_devices.sql", include_str!("../../../web/frontend/db/migrations/008_devices.sql")),
    ("009_invites.sql", include_str!("../../../web/frontend/db/migrations/009_invites.sql")),
    ("010_api_tokens.sql", include_str!("../../../web/frontend/db/migrations/010_api_tokens.sql")),
    (
        "011_workspace_layouts.sql",
        include_str!("../../../web/frontend/db/migrations/011_workspace_layouts.sql"),
    ),
    ("012_onboarding.sql", include_str!("../../../web/frontend/db/migrations/012_onboarding.sql")),
    ("013_admin_activity.sql", include_str!("../../../web/frontend/db/migrations/013_admin_activity.sql")),
];

fn sha256_hex(text: &str) -> String {
    spectra_core::sha256_hex(text.as_bytes())
}

/// A dashboard migration whose file changed after it was applied (the runner
/// only warns, as `db-migrate.mjs` does: add a new file instead of editing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edited(pub String);

const TRACKING: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
      name       text PRIMARY KEY,
      checksum   text NOT NULL,
      applied_at timestamptz NOT NULL DEFAULT now()
    )";

/// Applies pending dashboard migrations. Returns the names applied and any
/// applied files whose checksum no longer matches.
///
/// # Errors
/// A migration that fails (its transaction is rolled back) or a connection error.
pub async fn apply(pool: &PgPool) -> Result<(Vec<&'static str>, Vec<Edited>), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)").bind(LOCK_ID).execute(&mut *conn).await?;
    let result = async {
        sqlx::query(TRACKING).execute(&mut *conn).await?;
        let done: Vec<(String, String)> =
            sqlx::query_as("SELECT name, checksum FROM schema_migrations").fetch_all(&mut *conn).await?;
        let (mut applied, mut edited) = (Vec::new(), Vec::new());
        for (name, sql) in WEB_MIGRATIONS {
            let checksum = sha256_hex(sql);
            if let Some((_, recorded)) = done.iter().find(|(n, _)| n == name) {
                if *recorded != checksum {
                    edited.push(Edited((*name).to_owned()));
                }
                continue;
            }
            let mut tx = conn.begin().await?;
            // Simple-query protocol: a file holds several statements.
            sqlx::raw_sql(sql).execute(&mut *tx).await.map_err(|e| with_file(name, &e))?;
            sqlx::query("INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)")
                .bind(name)
                .bind(&checksum)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            applied.push(*name);
        }
        Ok((applied, edited))
    }
    .await;
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)").bind(LOCK_ID).execute(&mut *conn).await;
    result
}

/// Names of dashboard migrations not yet applied.
///
/// # Errors
/// Any query error.
pub async fn pending(pool: &PgPool) -> Result<Vec<&'static str>, sqlx::Error> {
    let done: Vec<String> =
        match sqlx::query_scalar("SELECT name FROM schema_migrations").fetch_all(pool).await {
            Ok(v) => v,
            // 42P01: undefined_table. Never migrated.
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("42P01") => Vec::new(),
            Err(e) => return Err(e),
        };
    Ok(WEB_MIGRATIONS.iter().map(|(n, _)| *n).filter(|n| !done.iter().any(|d| d == n)).collect())
}

fn with_file(name: &str, error: &sqlx::Error) -> sqlx::Error {
    sqlx::Error::Protocol(format!("dashboard migration {name}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adding a file to web/frontend/db/migrations without listing it here fails CI.
    #[test]
    fn embedded_list_matches_the_dashboard_directory() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/frontend/db/migrations");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .expect("dashboard migrations directory")
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            // db-migrate.mjs: /^\d+_.+\.sql$/
            .filter(|n| {
                let digits = n.chars().take_while(char::is_ascii_digit).count();
                digits > 0
                    && n[digits..].starts_with('_')
                    && std::path::Path::new(n).extension().is_some_and(|e| e == "sql")
            })
            .collect();
        on_disk.sort();
        let embedded: Vec<&str> = WEB_MIGRATIONS.iter().map(|(n, _)| *n).collect();
        assert_eq!(embedded, on_disk, "list every dashboard migration in web_migrations.rs, in name order");
    }

    /// The checksum db-migrate.mjs records for 001_auth.sql (Node's sha256 of the file text).
    #[test]
    fn checksum_matches_db_migrate_mjs() {
        let (name, sql) = WEB_MIGRATIONS[0];
        assert_eq!(name, "001_auth.sql");
        assert_eq!(sha256_hex(sql), "7619e4d6a8e1bb21cbbd2d0259a73e9ecd94764f49aeb3b885a9fd5ec62d21db");
    }
}
