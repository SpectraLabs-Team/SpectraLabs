//! Per-user saved state (M25.1): portfolio, paper account, risk parameters,
//! sandbox, watchlist, sandbox orders, and the one-time browser import.
//!
//! Every function takes `user_id` and `workspace_id` from the signed session.
//! Callers never pass them from a request body.

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value as JsonValue};
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// The per-user state keys stored on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKey {
    Portfolio,
    PaperAccount,
    RiskParams,
    Sandbox,
    Watchlist,
}

impl StateKey {
    /// Every key, in a fixed order.
    pub const ALL: [StateKey; 5] = [
        StateKey::Portfolio,
        StateKey::PaperAccount,
        StateKey::RiskParams,
        StateKey::Sandbox,
        StateKey::Watchlist,
    ];

    /// Parse the API path segment.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "portfolio" => Some(Self::Portfolio),
            "paper-account" => Some(Self::PaperAccount),
            "risk-params" => Some(Self::RiskParams),
            "sandbox" => Some(Self::Sandbox),
            "watchlist" => Some(Self::Watchlist),
            _ => None,
        }
    }

    /// The API name, also used as the import data key.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Portfolio => "portfolio",
            Self::PaperAccount => "paper-account",
            Self::RiskParams => "risk-params",
            Self::Sandbox => "sandbox",
            Self::Watchlist => "watchlist",
        }
    }

    /// Whether the value must be a JSON object (true) or an array (false).
    #[must_use]
    pub fn expects_object(self) -> bool {
        !matches!(self, Self::Watchlist)
    }

    fn table(self) -> &'static str {
        match self {
            Self::Portfolio => "user_portfolio",
            Self::PaperAccount => "user_paper_account",
            Self::RiskParams => "user_risk_params",
            Self::Sandbox => "user_paper_sandbox",
            Self::Watchlist => "user_paper_watchlist",
        }
    }

    /// SQL expression that reads the key's value as jsonb.
    fn payload(self) -> &'static str {
        match self {
            Self::Portfolio => "jsonb_build_object('csv', csv, 'source', source)",
            Self::PaperAccount => "state",
            Self::RiskParams => "params",
            Self::Sandbox => "jsonb_build_object('cash', cash, 'positions', positions)",
            Self::Watchlist => "symbols",
        }
    }

    /// SQL assignments that write the value bound as `$3`.
    fn assign(self) -> &'static str {
        match self {
            Self::Portfolio => "csv = COALESCE($3::jsonb->>'csv', ''), source = $3::jsonb->>'source'",
            Self::PaperAccount => "state = $3::jsonb",
            Self::RiskParams => "params = $3::jsonb",
            Self::Sandbox => {
                "cash = COALESCE(($3::jsonb->>'cash')::double precision, 0), \
                 positions = CASE WHEN jsonb_typeof($3::jsonb->'positions') = 'array' THEN $3::jsonb->'positions' ELSE '[]'::jsonb END"
            }
            Self::Watchlist => "symbols = $3::jsonb",
        }
    }

    /// The value returned when the user has no row yet.
    #[must_use]
    pub fn empty(self) -> JsonValue {
        match self {
            Self::Portfolio => json!({"csv": "", "source": null}),
            Self::PaperAccount | Self::RiskParams => json!({}),
            Self::Sandbox => json!({"cash": 0.0, "positions": []}),
            Self::Watchlist => json!([]),
        }
    }
}

/// A stored value with its optimistic-lock version.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StateValue {
    pub payload: JsonValue,
    pub version: i32,
    pub updated_at: DateTime<Utc>,
}

/// Result of a versioned write.
#[derive(Debug)]
pub enum PutOutcome {
    Saved(StateValue),
    /// The expected version did not match. Carries the current row, if any.
    Conflict(Option<StateValue>),
}

/// A sandbox order row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SandboxOrder {
    pub id: Uuid,
    pub payload: JsonValue,
    pub created_at: DateTime<Utc>,
}

/// Outcome of a one-time import.
#[derive(Debug)]
pub struct ImportReport {
    /// True when this import key was already applied, so nothing was written.
    pub noop: bool,
    pub imported: Vec<&'static str>,
    pub skipped: Vec<&'static str>,
}

/// Read one key. `None` means the user has no row yet.
///
/// # Errors
/// Any database error.
pub async fn get(pool: &PgPool, user: Uuid, ws: Uuid, key: StateKey) -> Result<Option<StateValue>, Error> {
    let sql = format!(
        "SELECT {} AS payload, version, updated_at FROM {} WHERE user_id = $1 AND workspace_id = $2",
        key.payload(),
        key.table()
    );
    sqlx::query_as(&sql).bind(user).bind(ws).fetch_optional(pool).await
}

/// Write one key if the stored version equals `expected_version`.
///
/// A user with no row has version 0. A successful write bumps the version by one.
///
/// # Errors
/// Any database error.
pub async fn put(
    pool: &PgPool,
    user: Uuid,
    ws: Uuid,
    key: StateKey,
    expected_version: i32,
    value: &JsonValue,
) -> Result<PutOutcome, Error> {
    let mut tx = pool.begin().await?;
    let insert = format!(
        "INSERT INTO {} (user_id, workspace_id, version) VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
        key.table()
    );
    sqlx::query(&insert).bind(user).bind(ws).execute(&mut *tx).await?;

    let update = format!(
        "UPDATE {table} SET {assign}, version = version + 1, updated_at = now() \
         WHERE user_id = $1 AND workspace_id = $2 AND version = $4 \
         RETURNING {payload} AS payload, version, updated_at",
        table = key.table(),
        assign = key.assign(),
        payload = key.payload(),
    );
    let saved: Option<StateValue> = sqlx::query_as(&update)
        .bind(user)
        .bind(ws)
        .bind(value)
        .bind(expected_version)
        .fetch_optional(&mut *tx)
        .await?;

    if let Some(row) = saved {
        tx.commit().await?;
        Ok(PutOutcome::Saved(row))
    } else {
        tx.rollback().await?;
        Ok(PutOutcome::Conflict(get(pool, user, ws, key).await?))
    }
}

/// Latest sandbox orders for the user in the workspace, newest first, at most 500.
///
/// # Errors
/// Any database error.
pub async fn list_sandbox_orders(pool: &PgPool, user: Uuid, ws: Uuid) -> Result<Vec<SandboxOrder>, Error> {
    sqlx::query_as(
        "SELECT id, payload, created_at FROM user_sandbox_orders \
         WHERE user_id = $1 AND workspace_id = $2 ORDER BY created_at DESC LIMIT 500",
    )
    .bind(user)
    .bind(ws)
    .fetch_all(pool)
    .await
}

/// Append a sandbox order. Orders are append-only.
///
/// # Errors
/// Any database error.
pub async fn append_sandbox_order(
    pool: &PgPool,
    user: Uuid,
    ws: Uuid,
    payload: &JsonValue,
) -> Result<SandboxOrder, Error> {
    sqlx::query_as(
        "INSERT INTO user_sandbox_orders (id, user_id, workspace_id, payload) \
         VALUES ($1, $2, $3, $4) RETURNING id, payload, created_at",
    )
    .bind(crate::new_id())
    .bind(user)
    .bind(ws)
    .bind(payload)
    .fetch_one(pool)
    .await
}

/// Replace one sandbox order's payload. Returns `None` when the order is not the user's in this workspace.
///
/// # Errors
/// Any database error.
pub async fn update_sandbox_order(
    pool: &PgPool,
    user: Uuid,
    ws: Uuid,
    order_id: Uuid,
    payload: &JsonValue,
) -> Result<Option<SandboxOrder>, Error> {
    sqlx::query_as(
        "UPDATE user_sandbox_orders SET payload = $4 \
         WHERE id = $1 AND user_id = $2 AND workspace_id = $3 \
         RETURNING id, payload, created_at",
    )
    .bind(order_id)
    .bind(user)
    .bind(ws)
    .bind(payload)
    .fetch_optional(pool)
    .await
}

/// Delete all of the user's sandbox orders in the workspace (used by sandbox reset).
///
/// # Errors
/// Any database error.
pub async fn clear_sandbox_orders(pool: &PgPool, user: Uuid, ws: Uuid) -> Result<u64, Error> {
    let res = sqlx::query("DELETE FROM user_sandbox_orders WHERE user_id = $1 AND workspace_id = $2")
        .bind(user)
        .bind(ws)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// One-time import from browser storage (Decision 3).
///
/// Server data wins: a key whose row already has a version above 0 is skipped.
/// A repeat call with the same `import_key` is a no-op. Everything runs in one
/// transaction, so a failed import writes nothing and can be retried.
///
/// # Errors
/// Any database error.
pub async fn import(
    pool: &PgPool,
    user: Uuid,
    ws: Uuid,
    import_key: &str,
    data: &Map<String, JsonValue>,
) -> Result<ImportReport, Error> {
    let mut tx = pool.begin().await?;

    let seen: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM user_state_imports WHERE user_id = $1 AND workspace_id = $2 AND import_key = $3",
    )
    .bind(user)
    .bind(ws)
    .bind(import_key)
    .fetch_optional(&mut *tx)
    .await?;
    if seen.is_some() {
        tx.rollback().await?;
        return Ok(ImportReport { noop: true, imported: Vec::new(), skipped: Vec::new() });
    }

    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for key in StateKey::ALL {
        let Some(value) = data.get(key.name()) else { continue };

        let existing: Option<(i32,)> = sqlx::query_as(&format!(
            "SELECT version FROM {} WHERE user_id = $1 AND workspace_id = $2",
            key.table()
        ))
        .bind(user)
        .bind(ws)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.is_some_and(|(v,)| v > 0) {
            skipped.push(key.name());
            continue;
        }

        let insert = format!(
            "INSERT INTO {} (user_id, workspace_id, version) VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
            key.table()
        );
        sqlx::query(&insert).bind(user).bind(ws).execute(&mut *tx).await?;
        let update = format!(
            "UPDATE {} SET {}, version = 1, updated_at = now() WHERE user_id = $1 AND workspace_id = $2",
            key.table(),
            key.assign()
        );
        sqlx::query(&update).bind(user).bind(ws).bind(value).execute(&mut *tx).await?;
        imported.push(key.name());
    }

    // Sandbox orders live in their own table. Import them only when none exist yet.
    if let Some(orders) = data.get("sandbox").and_then(|s| s.get("orders")).and_then(JsonValue::as_array) {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM user_sandbox_orders WHERE user_id = $1 AND workspace_id = $2",
        )
        .bind(user)
        .bind(ws)
        .fetch_one(&mut *tx)
        .await?;
        if count == 0 {
            for order in orders {
                sqlx::query(
                    "INSERT INTO user_sandbox_orders (id, user_id, workspace_id, payload) VALUES ($1, $2, $3, $4)",
                )
                .bind(crate::new_id())
                .bind(user)
                .bind(ws)
                .bind(order)
                .execute(&mut *tx)
                .await?;
            }
            imported.push("sandbox_orders");
        } else {
            skipped.push("sandbox_orders");
        }
    }

    sqlx::query("INSERT INTO user_state_imports (user_id, workspace_id, import_key) VALUES ($1, $2, $3)")
        .bind(user)
        .bind(ws)
        .bind(import_key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(ImportReport { noop: false, imported, skipped })
}
