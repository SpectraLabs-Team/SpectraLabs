//! Instrument records (M24.1). Numbers cross the database as exact
//! fixed-point integers (raw × 10⁻⁸), so no decimal type is needed.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use spectra_core::{Instrument, InstrumentType, Price, Qty, Ratio};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::Error;

/// One stored instrument, as the API and the broker layer read it.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentRow {
    /// Row id.
    pub id: Uuid,
    /// The record.
    pub instrument: Instrument,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

/// Stores a validated instrument. The caller validates against the base currency first.
///
/// # Errors
/// A database error, including a duplicate symbol in the workspace.
pub async fn create_instrument(pool: &PgPool, workspace_id: Uuid, inst: &Instrument) -> Result<Uuid, Error> {
    let row = sqlx::query(
        "INSERT INTO instruments (workspace_id, symbol, instrument_type, currency, multiplier, tick_size,
             lot_size, expiry, strike, maturity, coupon_rate, underlying)
         VALUES ($1, $2, $3, $4,
             ($5::bigint)::numeric / 100000000, ($6::bigint)::numeric / 100000000, ($7::bigint)::numeric / 100000000,
             to_timestamp(($8::bigint / 1000)::double precision / 1000000), ($9::bigint)::numeric / 100000000,
             to_timestamp(($10::bigint / 1000)::double precision / 1000000), ($11::bigint)::numeric / 100000000,
             $12)
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(inst.symbol.trim())
    .bind(inst.kind.as_str())
    .bind(&inst.currency)
    .bind(inst.multiplier.raw())
    .bind(inst.tick_size.raw())
    .bind(inst.lot_size.raw())
    .bind(inst.expiry_ns)
    .bind(inst.strike.map(spectra_core::Fixed::raw))
    .bind(inst.maturity_ns)
    .bind(inst.coupon_rate.map(spectra_core::Fixed::raw))
    .bind(inst.underlying.as_deref())
    .fetch_one(pool)
    .await?;
    Ok(row.get("id"))
}

/// Every instrument of the workspace, by symbol.
///
/// # Errors
/// A database error.
pub async fn list_instruments(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<InstrumentRow>, Error> {
    let rows = sqlx::query(
        "SELECT id, symbol, instrument_type, currency,
                (multiplier * 100000000)::bigint AS multiplier,
                (tick_size * 100000000)::bigint AS tick_size,
                (lot_size * 100000000)::bigint AS lot_size,
                (extract(epoch from expiry) * 1000000000)::bigint AS expiry_ns,
                (strike * 100000000)::bigint AS strike,
                (extract(epoch from maturity) * 1000000000)::bigint AS maturity_ns,
                (coupon_rate * 100000000)::bigint AS coupon_rate,
                underlying,
                created_at
         FROM instruments WHERE workspace_id = $1 ORDER BY symbol",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(|r| row_to_instrument(&r)).collect()
}

fn row_to_instrument(r: &sqlx::postgres::PgRow) -> Result<InstrumentRow, Error> {
    let bad = |what: &str| sqlx::Error::Protocol(format!("stored instrument has an invalid {what}"));
    let kind = InstrumentType::parse(r.get::<&str, _>("instrument_type")).ok_or_else(|| bad("type"))?;
    let instrument = Instrument {
        symbol: r.get("symbol"),
        kind,
        currency: r.get("currency"),
        multiplier: Ratio::from_raw(r.get("multiplier")),
        tick_size: Price::from_raw(r.get("tick_size")),
        lot_size: Qty::from_raw(r.get("lot_size")),
        expiry_ns: r.get("expiry_ns"),
        strike: r.get::<Option<i64>, _>("strike").map(Price::from_raw),
        maturity_ns: r.get("maturity_ns"),
        coupon_rate: r.get::<Option<i64>, _>("coupon_rate").map(Ratio::from_raw),
        underlying: r.get("underlying"),
    };
    Ok(InstrumentRow { id: r.get("id"), instrument, created_at: r.get("created_at") })
}

/// Instrument type by symbol, for the symbols the workspace has stored. A
/// symbol with no record is not in the map; callers treat it as an equity.
///
/// # Errors
/// A database error.
pub async fn instrument_types(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<HashMap<String, InstrumentType>, Error> {
    let rows = sqlx::query("SELECT symbol, instrument_type FROM instruments WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let kind = InstrumentType::parse(r.get::<&str, _>("instrument_type"))?;
            Some((r.get::<String, _>("symbol"), kind))
        })
        .collect())
}

/// Deletes the instrument `symbol` of the workspace. True when one was deleted.
///
/// # Errors
/// A database error.
pub async fn delete_instrument(pool: &PgPool, workspace_id: Uuid, symbol: &str) -> Result<bool, Error> {
    let r = sqlx::query("DELETE FROM instruments WHERE workspace_id = $1 AND symbol = $2")
        .bind(workspace_id)
        .bind(symbol.trim())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// The workspace's base currency (the default when it never chose one).
///
/// # Errors
/// A database error.
pub async fn base_currency(pool: &PgPool, workspace_id: Uuid) -> Result<String, Error> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT base_currency::text FROM workspace_currency WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_optional(pool)
            .await?;
    Ok(v.unwrap_or_else(|| spectra_core::DEFAULT_BASE_CURRENCY.to_string()))
}

/// Sets the workspace's base currency. The caller validates the code.
///
/// # Errors
/// A database error.
pub async fn set_base_currency(pool: &PgPool, workspace_id: Uuid, currency: &str) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO workspace_currency (workspace_id, base_currency) VALUES ($1, $2)
         ON CONFLICT (workspace_id) DO UPDATE SET base_currency = EXCLUDED.base_currency, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(currency)
    .execute(pool)
    .await?;
    Ok(())
}

/// One stored FX observation. `rate` is exact decimal text (12 fractional digits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxRow {
    /// Foreign currency.
    pub currency: String,
    /// When the rate became known, nanoseconds since the epoch (microsecond precision).
    pub as_of_ns: i64,
    /// Base-currency units per unit of `currency`, as decimal text.
    pub rate: String,
}

/// Stores FX observations against `base`, replacing any at the same time.
/// Rates are exact decimal text, validated by the caller. All or nothing.
///
/// # Errors
/// A database error.
pub async fn upsert_fx_rates(
    pool: &PgPool,
    workspace_id: Uuid,
    base: &str,
    currency: &str,
    rates: &[(i64, String)],
) -> Result<u64, Error> {
    let mut tx = pool.begin().await?;
    let mut n = 0;
    for (ts, rate) in rates {
        n += sqlx::query(
            "INSERT INTO fx_rates (workspace_id, currency, base_currency, as_of, rate)
             VALUES ($1, $2, $3, to_timestamp(($4::bigint / 1000)::double precision / 1000000), $5::numeric)
             ON CONFLICT (workspace_id, base_currency, currency, as_of) DO UPDATE SET rate = EXCLUDED.rate",
        )
        .bind(workspace_id)
        .bind(currency)
        .bind(base)
        .bind(ts)
        .bind(rate)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    tx.commit().await?;
    Ok(n)
}

/// FX observations against `base`, oldest first: of `currency` when given, else of every currency.
///
/// # Errors
/// A database error.
pub async fn list_fx_rates(
    pool: &PgPool,
    workspace_id: Uuid,
    base: &str,
    currency: Option<&str>,
) -> Result<Vec<FxRow>, Error> {
    let rows = sqlx::query(
        "SELECT currency::text AS currency,
                (extract(epoch from as_of) * 1000000000)::bigint AS as_of_ns,
                rate::text AS rate
         FROM fx_rates
         WHERE workspace_id = $1 AND base_currency = $2 AND ($3::text IS NULL OR currency = $3)
         ORDER BY currency, as_of",
    )
    .bind(workspace_id)
    .bind(base)
    .bind(currency)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| FxRow { currency: r.get("currency"), as_of_ns: r.get("as_of_ns"), rate: r.get("rate") })
        .collect())
}
