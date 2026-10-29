//! Activity retention (M21): removes daily active-time rows older than the retention window.
//!
//! The dashboard stores per-user daily totals in `user_activity_daily` (web migration 013) and
//! tells users they are deleted after [`ACTIVITY_RETENTION_DAYS`] days (`AccountPage.tsx`). The
//! worker runs [`purge_activity`] on a timer; `npm run activity:purge` does the same by hand.
//! Audit rows are never touched here: triggers in web migration 006 refuse UPDATE, DELETE and
//! TRUNCATE on the audit table.

use sqlx::PgPool;

/// Days of daily activity kept. Must match `ACTIVITY_RETENTION_DAYS` in
/// `web/frontend/lib/server/activity.ts` (checked by `web/frontend/scripts/test-activity.mjs`).
pub const ACTIVITY_RETENTION_DAYS: i32 = 400;

/// Advisory-lock name that keeps purges from overlapping, across worker replicas and the
/// `activity-purge.mjs` script (which uses the same name).
pub const PURGE_LOCK: &str = "spectra:activity-purge";

/// What one purge call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeOutcome {
    /// Rows removed, or for a dry run, rows that would be removed.
    Done {
        /// Row count.
        rows: u64,
        /// `true` if nothing was deleted.
        dry_run: bool,
    },
    /// Another purge holds the lock, so nothing was done.
    Busy,
}

/// Deletes `user_activity_daily` rows older than `days` (by UTC calendar day), or with
/// `dry_run` only counts them. Safe to run repeatedly: a second run finds nothing to delete,
/// and a run that finds another purge in progress returns [`PurgeOutcome::Busy`].
///
/// # Errors
/// `days` below 1, or any database error.
pub async fn purge_activity(pool: &PgPool, days: i32, dry_run: bool) -> Result<PurgeOutcome, sqlx::Error> {
    if days < 1 {
        return Err(sqlx::Error::Protocol("retention must be a whole number of days, at least 1".into()));
    }
    let mut tx = pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtext($1))")
        .bind(PURGE_LOCK)
        .fetch_one(&mut *tx)
        .await?;
    if !locked {
        tx.rollback().await?;
        return Ok(PurgeOutcome::Busy);
    }
    let rows = if dry_run {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM user_activity_daily WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int",
        )
        .bind(days)
        .fetch_one(&mut *tx)
        .await?;
        u64::try_from(n).unwrap_or(0)
    } else {
        sqlx::query("DELETE FROM user_activity_daily WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int")
            .bind(days)
            .execute(&mut *tx)
            .await?
            .rows_affected()
    };
    tx.commit().await?;
    Ok(PurgeOutcome::Done { rows, dry_run })
}
