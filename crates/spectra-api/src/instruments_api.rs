//! Instrument records, the workspace base currency and FX rates (M24).
//!
//! Instruments are validated by the same parser backtest requests use
//! ([`spectra_backtest::parse_instrument`]), so a record the API accepts is a
//! record a backtest accepts. FX rates are stored exactly (12 decimals) and
//! per base currency.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use serde::{Deserialize, Serialize};
use spectra_core::{is_currency_code, Instrument, Json};
use spectra_engine::FxRate;
use utoipa::{IntoParams, ToSchema};

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, invalid_fields, AppState, Identity};

/// One instrument record.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InstrumentView {
    /// Symbol, unique in the workspace.
    pub symbol: String,
    /// `equity`, `future`, `bond`, `option` or `swap`.
    #[serde(rename = "type")]
    pub kind: String,
    /// ISO currency the price is quoted in.
    pub currency: String,
    /// Contract multiplier, exact decimal text.
    pub multiplier: String,
    /// Smallest price increment, exact decimal text.
    pub tick_size: String,
    /// Smallest tradable quantity, exact decimal text.
    pub lot_size: String,
    /// Expiry (futures, options), nanoseconds since the epoch.
    pub expiry: Option<i64>,
    /// Strike (options), exact decimal text.
    pub strike: Option<String>,
    /// Maturity (bonds), nanoseconds since the epoch.
    pub maturity: Option<i64>,
    /// Annual coupon rate as a fraction (bonds), exact decimal text.
    pub coupon_rate: Option<String>,
    /// A future's chain or an option's underlying symbol.
    pub underlying: Option<String>,
    /// Coupons a year (bonds).
    pub coupon_frequency: Option<u32>,
    /// Day count for accrued interest (bonds): act_act_icma, 30_360, act_360 or act_365f.
    pub day_count: Option<String>,
}

impl From<&Instrument> for InstrumentView {
    fn from(i: &Instrument) -> Self {
        Self {
            symbol: i.symbol.clone(),
            kind: i.kind.as_str().into(),
            currency: i.currency.clone(),
            multiplier: i.multiplier.to_string(),
            tick_size: i.tick_size.to_string(),
            lot_size: i.lot_size.to_string(),
            expiry: i.expiry_ns,
            strike: i.strike.map(|p| p.to_string()),
            maturity: i.maturity_ns,
            coupon_rate: i.coupon_rate.map(|r| r.to_string()),
            underlying: i.underlying.clone(),
            coupon_frequency: i.coupon_frequency,
            day_count: i.day_count.map(|d| d.as_str().to_string()),
        }
    }
}

/// The workspace's instruments.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InstrumentListView {
    /// The base currency records default to.
    pub base_currency: String,
    /// By symbol.
    pub items: Vec<InstrumentView>,
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn read_json(body: Result<Bytes, BytesRejection>) -> Result<Json, Response> {
    let Ok(bytes) = body else { return Err(error_response(400, "Could not read the request body.")) };
    let text =
        std::str::from_utf8(&bytes).map_err(|_| error_response(422, "The body must be UTF-8 JSON."))?;
    Json::parse(text).map_err(|e| error_response(422, format!("Invalid JSON: {e}")))
}

/// List the workspace's instrument records.
#[utoipa::path(get, path = "/api/v1/instruments", tag = "instruments",
    responses((status = 200, body = InstrumentListView)))]
pub(crate) async fn list_instruments(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let base = match spectra_db::instruments::base_currency(&pool, ws).await {
        Ok(b) => b,
        Err(e) => return db_error(&e),
    };
    match spectra_db::instruments::list_instruments(&pool, ws).await {
        Ok(rows) => to_json(
            200,
            &InstrumentListView {
                base_currency: base,
                items: rows.iter().map(|r| InstrumentView::from(&r.instrument)).collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Add an instrument record. The body has the shape of a backtest request's
/// `instruments` entry, named by `symbol`.
#[utoipa::path(post, path = "/api/v1/instruments", tag = "instruments",
    responses(
        (status = 201, body = InstrumentView),
        (status = 409, description = "The symbol already has a record", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every problem with the record", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_instrument(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let doc = match read_json(body) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let base = match spectra_db::instruments::base_currency(&pool, ws).await {
        Ok(b) => b,
        Err(e) => return db_error(&e),
    };
    let inst = match spectra_backtest::parse_instrument(&doc, "", &base) {
        Ok((spectra_backtest::InstrumentRef::Symbol(_), inst)) => inst,
        Ok((spectra_backtest::InstrumentRef::Id(_), _)) => {
            return invalid_fields(&[("symbol".into(), "a stored record is named by symbol".into())])
        }
        Err(errs) => {
            return invalid_fields(&errs.into_iter().map(|e| (e.field, e.message)).collect::<Vec<_>>())
        }
    };
    match spectra_db::instruments::create_instrument(&pool, ws, &inst).await {
        Ok(_) => to_json(201, &InstrumentView::from(&inst)),
        Err(e) if e.as_database_error().is_some_and(|d| d.code().as_deref() == Some("23505")) => {
            error_response(409, format!("{} already has an instrument record.", inst.symbol))
        }
        Err(e) => db_error(&e),
    }
}

/// Result of a delete.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeletedView {
    /// True when something was deleted.
    pub deleted: bool,
}

/// Delete an instrument record. Runs that used it keep their stored copy.
#[utoipa::path(delete, path = "/api/v1/instruments/{symbol}", tag = "instruments",
    params(("symbol" = String, Path, description = "Symbol")),
    responses((status = 200, body = DeletedView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn delete_instrument(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(symbol): Path<String>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::instruments::delete_instrument(&pool, ws, &symbol).await {
        Ok(true) => to_json(200, &DeletedView { deleted: true }),
        Ok(false) => error_response(404, "No such instrument."),
        Err(e) => db_error(&e),
    }
}

/// The workspace's base currency.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BaseCurrencyView {
    /// ISO code, e.g. `USD`.
    pub base_currency: String,
}

/// The currency the workspace keeps its books in.
#[utoipa::path(get, path = "/api/v1/base-currency", tag = "instruments",
    responses((status = 200, body = BaseCurrencyView)))]
pub(crate) async fn get_base_currency(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::instruments::base_currency(&pool, ws).await {
        Ok(b) => to_json(200, &BaseCurrencyView { base_currency: b }),
        Err(e) => db_error(&e),
    }
}

/// Change the base currency. Earlier runs keep the currency they ran in;
/// FX rates are kept per base, so rates against the old base are not reused.
#[utoipa::path(put, path = "/api/v1/base-currency", tag = "instruments", request_body = BaseCurrencyView,
    responses((status = 200, body = BaseCurrencyView), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn put_base_currency(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: BaseCurrencyView = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid request: {e}")),
    };
    let code = req.base_currency.trim().to_string();
    if !is_currency_code(&code) {
        return invalid_fields(&[(
            "base_currency".into(),
            "must be a three-letter ISO code such as USD".into(),
        )]);
    }
    match spectra_db::instruments::set_base_currency(&pool, ws, &code).await {
        Ok(()) => to_json(200, &BaseCurrencyView { base_currency: code }),
        Err(e) => db_error(&e),
    }
}

/// One FX observation.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct FxRateView {
    /// Foreign currency.
    pub currency: String,
    /// When the rate became known, nanoseconds since the epoch.
    pub as_of: i64,
    /// Base-currency units per unit of `currency`, exact decimal text.
    pub rate: String,
}

/// FX rates against the workspace's base currency.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct FxRateListView {
    /// The base the rates are quoted against.
    pub base_currency: String,
    /// Oldest first, by currency.
    pub items: Vec<FxRateView>,
}

/// `GET /api/v1/fx-rates` query.
#[derive(Debug, Deserialize, IntoParams)]
pub struct FxQuery {
    /// Only this currency.
    pub currency: Option<String>,
}

/// List FX rates against the base currency.
#[utoipa::path(get, path = "/api/v1/fx-rates", tag = "instruments", params(FxQuery),
    responses((status = 200, body = FxRateListView)))]
pub(crate) async fn list_fx_rates(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Query(q): Query<FxQuery>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let base = match spectra_db::instruments::base_currency(&pool, ws).await {
        Ok(b) => b,
        Err(e) => return db_error(&e),
    };
    match spectra_db::instruments::list_fx_rates(&pool, ws, &base, q.currency.as_deref()).await {
        Ok(rows) => to_json(
            200,
            &FxRateListView {
                base_currency: base,
                items: rows
                    .into_iter()
                    .map(|r| FxRateView { currency: r.currency, as_of: r.as_of_ns, rate: r.rate })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Store FX rates for one currency against the base currency:
/// `{"currency": "EUR", "rates": [{"as_of": "2026-01-02", "rate": "1.0845"}]}`.
/// A rate at an existing time replaces it. All or nothing.
#[utoipa::path(post, path = "/api/v1/fx-rates", tag = "instruments",
    responses((status = 200, body = FxRateListView), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn put_fx_rates(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let doc = match read_json(body) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let base = match spectra_db::instruments::base_currency(&pool, ws).await {
        Ok(b) => b,
        Err(e) => return db_error(&e),
    };
    let mut errors: Vec<(String, String)> = Vec::new();
    let currency = match doc.find("currency").map(Json::as_str) {
        Some(Ok(c)) if is_currency_code(c.trim()) && c.trim() != base => c.trim().to_string(),
        Some(Ok(c)) if c.trim() == base => {
            errors.push(("currency".into(), format!("{base} is the base currency; it needs no rates")));
            String::new()
        }
        _ => {
            errors.push(("currency".into(), "must be a three-letter ISO code".into()));
            String::new()
        }
    };
    // Parse through the backtest request reader, so the rules are the same.
    let wrapped = Json::object().with(
        "fx_rates",
        Json::object().with(
            if currency.is_empty() { "XXX" } else { currency.as_str() },
            doc.find("rates").cloned().unwrap_or(Json::Null),
        ),
    );
    let mut ferrs = Vec::new();
    let parsed = spectra_backtest::RequestInstruments::parse(&wrapped, &mut ferrs);
    errors.extend(ferrs.into_iter().map(|e| {
        (
            e.field.replacen("fx_rates.XXX", "rates", 1).replacen(
                &format!("fx_rates.{currency}"),
                "rates",
                1,
            ),
            e.message,
        )
    }));
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    let Some(series) = parsed.and_then(|p| p.fx.get(&currency).cloned()) else {
        return invalid_fields(&[("rates".into(), "must be a non-empty list of {as_of, rate}".into())]);
    };
    if series.is_empty() {
        return invalid_fields(&[("rates".into(), "must be a non-empty list of {as_of, rate}".into())]);
    }
    let rows: Vec<(i64, String)> = series.iter().map(|r| (r.as_of_ns, FxRate::rate_text(r.rate))).collect();
    if let Err(e) = spectra_db::instruments::upsert_fx_rates(&pool, ws, &base, &currency, &rows).await {
        return db_error(&e);
    }
    match spectra_db::instruments::list_fx_rates(&pool, ws, &base, Some(&currency)).await {
        Ok(rows) => to_json(
            200,
            &FxRateListView {
                base_currency: base,
                items: rows
                    .into_iter()
                    .map(|r| FxRateView { currency: r.currency, as_of: r.as_of_ns, rate: r.rate })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// What a backtest submitted in this workspace needs from the instrument
/// tables, added to its payload unless the caller gave it: the base currency,
/// every record that changes a run, and the FX series those records need.
/// Stored with the job, so a run always reproduces with the data it used.
///
/// # Errors
/// A database error.
pub(crate) async fn embed_instruments(
    pool: &spectra_db::PgPool,
    ws: uuid::Uuid,
    payload: &mut serde_json::Value,
) -> Result<(), spectra_db::Error> {
    let base = if let Some(b) = payload.get("base_currency").and_then(serde_json::Value::as_str) {
        b.to_string()
    } else {
        let b = spectra_db::instruments::base_currency(pool, ws).await?;
        payload["base_currency"] = serde_json::Value::String(b.clone());
        b
    };
    if payload.get("instruments").is_none() {
        let rows = spectra_db::instruments::list_instruments(pool, ws).await?;
        let effective: Vec<&Instrument> =
            rows.iter().map(|r| &r.instrument).filter(|i| !i.is_plain_equity(&base)).collect();
        if !effective.is_empty() {
            payload["instruments"] = serde_json::Value::Array(
                effective
                    .iter()
                    .filter_map(|i| serde_json::to_value(InstrumentView::from(*i)).ok())
                    .map(|mut v| {
                        // The request names `expiry` and `maturity` as timestamps; null fields are dropped.
                        if let Some(o) = v.as_object_mut() {
                            o.retain(|_, x| !x.is_null());
                        }
                        v
                    })
                    .collect(),
            );
        }
    }
    if payload.get("fx_rates").is_none() {
        let mut currencies: Vec<String> = payload
            .get("instruments")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|i| i.get("currency").and_then(serde_json::Value::as_str))
                    .filter(|c| *c != base)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        currencies.sort();
        currencies.dedup();
        if !currencies.is_empty() {
            let mut fx = serde_json::Map::new();
            for c in currencies {
                let rows = spectra_db::instruments::list_fx_rates(pool, ws, &base, Some(&c)).await?;
                if rows.is_empty() {
                    continue; // the run refuses the instrument and says why
                }
                fx.insert(
                    c,
                    serde_json::Value::Array(
                        rows.into_iter()
                            .map(|r| serde_json::json!({ "as_of": r.as_of_ns, "rate": r.rate }))
                            .collect(),
                    ),
                );
            }
            if !fx.is_empty() {
                payload["fx_rates"] = serde_json::Value::Object(fx);
            }
        }
    }
    Ok(())
}
