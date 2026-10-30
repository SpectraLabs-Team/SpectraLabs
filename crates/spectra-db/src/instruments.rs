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
             lot_size, expiry, strike, maturity, coupon_rate)
         VALUES ($1, $2, $3, $4,
             ($5::bigint)::numeric / 100000000, ($6::bigint)::numeric / 100000000, ($7::bigint)::numeric / 100000000,
             to_timestamp(($8::bigint / 1000)::double precision / 1000000), ($9::bigint)::numeric / 100000000,
             to_timestamp(($10::bigint / 1000)::double precision / 1000000), ($11::bigint)::numeric / 100000000)
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
    .bind(inst.strike.map(|p| p.raw()))
    .bind(inst.maturity_ns)
    .bind(inst.coupon_rate.map(|r| r.raw()))
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
