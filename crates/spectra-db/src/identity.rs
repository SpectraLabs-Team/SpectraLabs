//! Signed-in identities mirrored from the frontend's database (M1 task 1).
//!
//! The Next.js app keeps its own `users` and `workspaces` (M19) and signs the
//! caller's user id and active workspace id into every engine request. Every
//! engine table points at the engine's own copies of those tables, so before a
//! request touches anything the API calls [`ensure_identity`]: it creates the
//! user and workspace rows with the same ids if they are missing, and never
//! changes rows that exist. Existing accounts are backfilled the first time
//! they call the engine.
//!
//! The engine is never told the user's email or the workspace's name, so the
//! mirrored rows carry placeholders (`u-<id>@mirror.invalid`, no usable
//! password, slug `ws-<id>`). Nothing in the engine reads them; the frontend's
//! database stays the source of truth for accounts, names and membership.

use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// Creates the engine's rows for `user_id` (when there is one) and
/// `workspace_id` if they do not exist yet. Existing rows are left as they are.
///
/// # Errors
/// Any database error.
pub async fn ensure_identity(pool: &PgPool, user_id: Option<Uuid>, workspace_id: Uuid) -> Result<(), Error> {
    sqlx::query(include_str!("../queries/identity_mirror.sql"))
        .bind(user_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .map(|_| ())
}
