//! Synchronous pricing with the M24 models: bonds (accrued interest, clean
//! and dirty prices, yield, duration). Nothing is stored; the answer depends
//! only on the request.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use spectra_core::{DayCount, COUPON_FREQUENCIES};
use spectra_engine::BondTerms;
use utoipa::ToSchema;

use crate::jobs_api::to_json;
use crate::{error_response, invalid_fields};

/// `POST /api/v1/pricing/bond` body. Give exactly one of `clean_price`,
/// `dirty_price` or `yield`; the rest are worked out from it.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BondPricingRequest {
    /// Maturity date (RFC 3339 or YYYY-MM-DD).
    pub maturity: String,
    /// Annual coupon rate as a fraction (0.05 is 5%).
    pub coupon_rate: f64,
    /// Coupons a year: 1, 2, 4 or 12.
    pub coupon_frequency: u32,
    /// act_act_icma, 30_360, act_360 or act_365f.
    pub day_count: String,
    /// The valuation date (RFC 3339 or YYYY-MM-DD).
    pub as_of: String,
    /// Price per 100 face without accrued interest.
    #[serde(default)]
    pub clean_price: Option<f64>,
    /// Price per 100 face with accrued interest.
    #[serde(default)]
    pub dirty_price: Option<f64>,
    /// Annual yield, compounded `coupon_frequency` times a year.
    #[serde(default, rename = "yield")]
    pub yield_rate: Option<f64>,
}

/// A bond priced.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct BondPricingView {
    /// Per 100 face.
    pub clean_price: f64,
    /// Per 100 face.
    pub dirty_price: f64,
    /// Per 100 face.
    pub accrued_interest: f64,
    /// Annual, compounded `coupon_frequency` times a year.
    #[serde(rename = "yield")]
    pub yield_rate: f64,
    /// Years.
    pub macaulay_duration: f64,
    /// Years.
    pub modified_duration: f64,
    /// Years squared.
    pub convexity: f64,
    /// Price change per 100 face for a one basis point move in yield.
    pub dv01: f64,
}

pub(crate) fn parse_date_ns(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.timestamp_nanos_opt();
    }
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|dt| dt.and_utc().timestamp_nanos_opt())
}

#[allow(clippy::result_large_err)] // the error is the finished response
pub(crate) fn read_body<T: for<'de> Deserialize<'de>>(
    body: Result<Bytes, BytesRejection>,
) -> Result<T, Response> {
    let Ok(bytes) = body else { return Err(error_response(400, "Could not read the request body.")) };
    serde_json::from_slice(&bytes).map_err(|e| error_response(422, format!("Invalid request: {e}")))
}

/// Price a fixed-coupon bond: accrued interest, clean and dirty price, yield,
/// duration, convexity and DV01, from any one of clean price, dirty price or yield.
#[utoipa::path(post, path = "/api/v1/pricing/bond", tag = "pricing", request_body = BondPricingRequest,
    responses(
        (status = 200, body = BondPricingView),
        (status = 422, description = "Every problem with the request", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn price_bond(body: Result<Bytes, BytesRejection>) -> Response {
    let req: BondPricingRequest = match read_body(body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let mut errors: Vec<(String, String)> = Vec::new();
    let maturity = parse_date_ns(&req.maturity);
    if maturity.is_none() {
        errors.push(("maturity".into(), "must be an RFC 3339 time or YYYY-MM-DD date".into()));
    }
    let as_of = parse_date_ns(&req.as_of);
    if as_of.is_none() {
        errors.push(("as_of".into(), "must be an RFC 3339 time or YYYY-MM-DD date".into()));
    }
    let day_count = DayCount::parse(req.day_count.trim());
    if day_count.is_none() {
        errors.push(("day_count".into(), "must be act_act_icma, 30_360, act_360 or act_365f".into()));
    }
    if !COUPON_FREQUENCIES.contains(&req.coupon_frequency) {
        errors.push(("coupon_frequency".into(), "must be 1, 2, 4 or 12".into()));
    }
    let given = [req.clean_price.is_some(), req.dirty_price.is_some(), req.yield_rate.is_some()];
    if given.iter().filter(|g| **g).count() != 1 {
        errors.push((String::new(), "give exactly one of clean_price, dirty_price or yield".into()));
    }
    let (Some(maturity), Some(as_of), Some(day_count)) = (maturity, as_of, day_count) else {
        return invalid_fields(&errors);
    };
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    let terms = BondTerms {
        maturity_ns: maturity,
        coupon_rate: req.coupon_rate,
        frequency: req.coupon_frequency,
        day_count,
    };
    if let Err(e) = terms.validate() {
        return invalid_fields(&[("coupon_rate".into(), e.to_string())]);
    }
    if as_of >= maturity {
        return invalid_fields(&[("as_of".into(), "must be before maturity".into())]);
    }
    let accrued = terms.accrued(as_of);
    let (dirty, y) = if let Some(y) = req.yield_rate {
        if !y.is_finite() {
            return invalid_fields(&[("yield".into(), "must be a finite number".into())]);
        }
        (terms.price_from_yield(y, as_of), y)
    } else {
        let dirty = req.dirty_price.unwrap_or_else(|| req.clean_price.unwrap_or(0.0) + accrued);
        match terms.yield_from_price(dirty, as_of) {
            Ok(y) => (dirty, y),
            Err(e) => {
                let field = if req.dirty_price.is_some() { "dirty_price" } else { "clean_price" };
                return invalid_fields(&[(field.into(), e.to_string())]);
            }
        }
    };
    let r = terms.risk(y, as_of);
    to_json(
        200,
        &BondPricingView {
            clean_price: dirty - accrued,
            dirty_price: dirty,
            accrued_interest: accrued,
            yield_rate: y,
            macaulay_duration: r.macaulay,
            modified_duration: r.modified,
            convexity: r.convexity,
            dv01: r.dv01,
        },
    )
}
