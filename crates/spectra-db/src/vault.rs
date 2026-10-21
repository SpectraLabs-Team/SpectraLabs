//! Vault secret and workspace key queries for credential resolution.
//!
//! All queries live in `queries/` and are exercised by `tests/provider_checks.sql`.

use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

const SECRET_GET_SQL: &str = include_str!("../queries/vault_secret_get.sql");
const WORKSPACE_KEY_GET_SQL: &str = include_str!("../queries/vault_workspace_key_get.sql");
const SECRET_MARK_USED_SQL: &str = include_str!("../queries/vault_secret_mark_used.sql");

/// A stored encrypted secret row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SecretRow {
    /// Secret id.
    pub id: Uuid,
    /// Workspace id.
    pub workspace_id: Uuid,
    /// Kind (e.g. "alpaca_paper").
    pub kind: String,
    /// Encrypted ciphertext (format byte, 12-byte IV, 16-byte tag, ciphertext).
    pub ciphertext: Vec<u8>,
    /// Hint (last 4 characters of key ID).
    pub hint: String,
}

/// A stored wrapped workspace data key row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct WorkspaceKeyRow {
    /// Wrapped key bytes (format byte, 12-byte IV, 16-byte tag, encrypted data key).
    pub wrapped_key: Vec<u8>,
    /// Version of master key that wrapped it.
    pub master_version: i32,
}

/// Retrieves a secret by its specific id or by workspace and kind.
///
/// # Errors
/// Returns [`Error`] if database query fails.
pub async fn get_secret(
    pool: &PgPool,
    workspace_id: Uuid,
    secret_id: Option<Uuid>,
    kind: Option<&str>,
) -> Result<Option<SecretRow>, Error> {
    sqlx::query_as::<_, SecretRow>(SECRET_GET_SQL)
        .bind(workspace_id)
        .bind(secret_id)
        .bind(kind)
        .fetch_optional(pool)
        .await
}

/// Retrieves the wrapped data key for a workspace.
///
/// # Errors
/// Returns [`Error`] if database query fails.
pub async fn get_workspace_key(pool: &PgPool, workspace_id: Uuid) -> Result<Option<WorkspaceKeyRow>, Error> {
    sqlx::query_as::<_, WorkspaceKeyRow>(WORKSPACE_KEY_GET_SQL).bind(workspace_id).fetch_optional(pool).await
}

/// Marks a secret as recently used by updating its `last_used_at`.
///
/// # Errors
/// Returns [`Error`] if database query fails.
pub async fn mark_secret_used(pool: &PgPool, workspace_id: Uuid, secret_id: Uuid) -> Result<(), Error> {
    sqlx::query(SECRET_MARK_USED_SQL).bind(workspace_id).bind(secret_id).execute(pool).await?;
    Ok(())
}
