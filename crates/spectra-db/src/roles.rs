//! Postgres role separation and permissions management (M20 hardening).
//!
//! Provides separation between the migration owner (schema DDL) and the
//! application runtime role (`spectra_app`) used by `spectra-api` and `spectra-worker`.

use sqlx::PgPool;

/// Result of provisioning service roles.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoleProvisionOutcome {
    pub role_name: String,
    pub created: bool,
    pub grants_applied: bool,
}

/// Provision or update the runtime application role (`spectra_app`).
///
/// The application role is granted:
/// - `CONNECT` on the current database
/// - `USAGE` on schema `public`
/// - `SELECT, INSERT, UPDATE, DELETE` on engine tables
/// - `SELECT` only on sensitive dashboard tables (`users`, `workspaces`, `workspace_members`, `schema_migrations`, `_sqlx_migrations`)
/// - `USAGE, SELECT, UPDATE` on all sequences
/// - `DEFAULT PRIVILEGES` for future tables and sequences created by the owner
///
/// # Errors
/// Fails if the current connection lacks `CREATEROLE` or table grant privileges.
pub async fn provision_service_roles(
    pool: &PgPool,
    role_name: &str,
    password: Option<&str>,
) -> Result<RoleProvisionOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;

    // 1. Check if role exists
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)")
            .bind(role_name)
            .fetch_one(&mut *tx)
            .await?;

    let mut created = false;
    if !exists {
        let create_sql = match password {
            Some(pw) if !pw.trim().is_empty() => {
                format!("CREATE ROLE \"{role_name}\" WITH LOGIN PASSWORD '{pw}'")
            }
            _ => format!("CREATE ROLE \"{role_name}\" WITH LOGIN"),
        };
        sqlx::query(&create_sql).execute(&mut *tx).await?;
        created = true;
    } else if let Some(pw) = password {
        if !pw.trim().is_empty() {
            let alter_sql = format!("ALTER ROLE \"{role_name}\" WITH PASSWORD '{pw}'");
            sqlx::query(&alter_sql).execute(&mut *tx).await?;
        }
    }

    // 2. Grant schema usage and connect
    let db_name: String = sqlx::query_scalar("SELECT current_database()").fetch_one(&mut *tx).await?;
    let grant_db = format!("GRANT CONNECT ON DATABASE \"{db_name}\" TO \"{role_name}\"");
    sqlx::query(&grant_db).execute(&mut *tx).await?;

    let grant_schema = format!("GRANT USAGE ON SCHEMA public TO \"{role_name}\"");
    sqlx::query(&grant_schema).execute(&mut *tx).await?;

    // 3. Grant table permissions
    let grant_all_tables =
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO \"{role_name}\"");
    sqlx::query(&grant_all_tables).execute(&mut *tx).await?;

    // 4. Grant sequences
    let grant_seqs =
        format!("GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO \"{role_name}\"");
    sqlx::query(&grant_seqs).execute(&mut *tx).await?;

    // 5. Enforce read-only access on sensitive dashboard & migration tables if they exist
    let read_only_tables =
        ["users", "workspaces", "workspace_members", "schema_migrations", "_sqlx_migrations"];

    for tbl in read_only_tables {
        let tbl_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = $1)",
        )
        .bind(tbl)
        .fetch_one(&mut *tx)
        .await?;

        if tbl_exists {
            let revoke_write =
                format!("REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON \"{tbl}\" FROM \"{role_name}\"");
            sqlx::query(&revoke_write).execute(&mut *tx).await?;

            let grant_read = format!("GRANT SELECT ON \"{tbl}\" TO \"{role_name}\"");
            sqlx::query(&grant_read).execute(&mut *tx).await?;
        }
    }

    // 6. Set default privileges for future objects created by the current user
    let default_tables = format!("ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"{role_name}\"");
    sqlx::query(&default_tables).execute(&mut *tx).await?;

    let default_seqs = format!("ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO \"{role_name}\"");
    sqlx::query(&default_seqs).execute(&mut *tx).await?;

    tx.commit().await?;

    Ok(RoleProvisionOutcome { role_name: role_name.to_string(), created, grants_applied: true })
}

/// Verify permissions of the application role against the schema.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoleVerification {
    pub role_name: String,
    pub can_connect: bool,
    pub can_read_workspaces: bool,
    pub can_write_jobs: bool,
    pub cannot_write_users: bool,
}

/// Check that the role has expected read and write boundaries.
///
/// # Errors
/// Returns an error if querying PostgreSQL role privileges fails.
pub async fn verify_service_role(pool: &PgPool, role_name: &str) -> Result<RoleVerification, sqlx::Error> {
    let can_connect: bool =
        sqlx::query_scalar("SELECT has_database_privilege($1, current_database(), 'CONNECT')")
            .bind(role_name)
            .fetch_one(pool)
            .await?;

    let can_read_workspaces: bool =
        sqlx::query_scalar("SELECT has_table_privilege($1, 'workspaces', 'SELECT')")
            .bind(role_name)
            .fetch_one(pool)
            .await
            .unwrap_or(false);

    let can_write_jobs: bool = sqlx::query_scalar("SELECT has_table_privilege($1, 'jobs', 'INSERT')")
        .bind(role_name)
        .fetch_one(pool)
        .await
        .unwrap_or(false);

    let can_write_users: bool = sqlx::query_scalar("SELECT has_table_privilege($1, 'users', 'INSERT')")
        .bind(role_name)
        .fetch_one(pool)
        .await
        .unwrap_or(false);

    Ok(RoleVerification {
        role_name: role_name.to_string(),
        can_connect,
        can_read_workspaces,
        can_write_jobs,
        cannot_write_users: !can_write_users,
    })
}
