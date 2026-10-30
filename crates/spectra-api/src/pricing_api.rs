//! Synchronous pricing with the M24 models: bonds (accrued interest, clean
//! and dirty prices, yield, duration, z-spread over a curve), European
//! options (Black-Scholes-Merton price, Greeks, implied volatility), discount
//! curves (bootstrapped from deposits and par swaps) and interest-rate swaps.
//! Nothing is stored; the answer depends only on the request.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use spectra_core::{DayCount, OptionRight, COUPON_FREQUENCIES};
use spectra_engine::{BondTerms, Curve, CurveQuote, OptionInputs, Swap};
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
    /// A discount curve to measure the bond's z-spread over (optional).
    #[serde(default)]
    pub curve: Option<CurveInput>,
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
    /// Continuously compounded spread over `curve` that reprices the bond, when a curve was given.
    pub z_spread: Option<f64>,
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
    let z_spread = match req.curve.as_ref().map(CurveInput::build) {
        None => None,
        Some(Err(e)) => return invalid_fields(&prefix("curve", e)),
        Some(Ok(curve)) => match spectra_engine::z_spread(&terms.cash_flows(as_of), &curve, dirty) {
            Ok(z) => Some(z),
            Err(e) => return invalid_fields(&[("curve".into(), e.to_string())]),
        },
    };
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
            z_spread,
        },
    )
}

/// `POST /api/v1/pricing/option` body. Give `volatility` for a price and the
/// Greeks, or `price` for the implied volatility (and the Greeks at it).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionPricingRequest {
    /// `call` or `put`.
    pub right: String,
    /// Underlying price.
    pub spot: f64,
    /// Strike.
    pub strike: f64,
    /// Time to expiry in years.
    pub years: f64,
    /// Continuously compounded risk-free rate.
    pub rate: f64,
    /// Continuous dividend (or carry) yield. Default 0.
    #[serde(default)]
    pub dividend_yield: f64,
    /// Annual volatility.
    #[serde(default)]
    pub volatility: Option<f64>,
    /// Option price, to solve for the implied volatility.
    #[serde(default)]
    pub price: Option<f64>,
}

/// An option priced by Black-Scholes-Merton.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OptionPricingView {
    /// Theoretical price.
    pub price: f64,
    /// The volatility used (given, or implied by `price`).
    pub volatility: f64,
    /// dPrice/dSpot.
    pub delta: f64,
    /// d²Price/dSpot².
    pub gamma: f64,
    /// Per 1.00 of volatility.
    pub vega: f64,
    /// Per year.
    pub theta: f64,
    /// Per 1.00 of rate.
    pub rho: f64,
    /// The model's review status: it awaits the M26 correctness audit.
    pub model_status: String,
}

/// Price a European option (Black-Scholes-Merton with a dividend yield), or
/// find its implied volatility from a price.
#[utoipa::path(post, path = "/api/v1/pricing/option", tag = "pricing", request_body = OptionPricingRequest,
    responses(
        (status = 200, body = OptionPricingView),
        (status = 422, description = "Every problem with the request", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn price_option(body: Result<Bytes, BytesRejection>) -> Response {
    let req: OptionPricingRequest = match read_body(body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let mut errors: Vec<(String, String)> = Vec::new();
    let right = OptionRight::parse(req.right.trim());
    if right.is_none() {
        errors.push(("right".into(), "must be \"call\" or \"put\"".into()));
    }
    if req.volatility.is_some() == req.price.is_some() {
        errors.push((String::new(), "give exactly one of volatility or price".into()));
    }
    let Some(right) = right else { return invalid_fields(&errors) };
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    let base = OptionInputs {
        right,
        spot: req.spot,
        strike: req.strike,
        years: req.years,
        rate: req.rate,
        dividend_yield: req.dividend_yield,
        volatility: req.volatility.unwrap_or(0.2),
    };
    let vol = match req.price {
        Some(p) => match base.implied_volatility(p) {
            Ok(v) => v,
            Err(e) => return invalid_fields(&[("price".into(), e.to_string())]),
        },
        None => base.volatility,
    };
    match (OptionInputs { volatility: vol, ..base }).value() {
        Ok(v) => to_json(
            200,
            &OptionPricingView {
                price: v.price,
                volatility: vol,
                delta: v.delta,
                gamma: v.gamma,
                vega: v.vega,
                theta: v.theta,
                rho: v.rho,
                model_status: "awaiting M26 review".into(),
            },
        ),
        Err(e) => invalid_fields(&[(String::new(), e.to_string())]),
    }
}

/// One market quote for a curve bootstrap.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveQuoteInput {
    /// `deposit` (a simple rate to `years`) or `swap` (a par swap rate).
    #[serde(rename = "type")]
    pub kind: String,
    /// Maturity, years.
    pub years: f64,
    /// The rate, as a fraction (0.04 is 4%).
    pub rate: f64,
    /// Fixed payments a year (swaps): 1, 2, 4 or 12.
    #[serde(default)]
    pub frequency: Option<u32>,
}

/// A discount curve: market quotes to bootstrap, or one flat rate.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveInput {
    /// Deposits and par swaps, by increasing maturity.
    #[serde(default)]
    pub quotes: Option<Vec<CurveQuoteInput>>,
    /// A flat continuously compounded rate instead of quotes.
    #[serde(default)]
    pub flat_rate: Option<f64>,
}

fn prefix(p: &str, errors: Vec<(String, String)>) -> Vec<(String, String)> {
    errors
        .into_iter()
        .map(|(f, m)| (if f.is_empty() { p.to_string() } else { format!("{p}.{f}") }, m))
        .collect()
}

impl CurveInput {
    fn build(&self) -> Result<Curve, Vec<(String, String)>> {
        match (&self.quotes, self.flat_rate) {
            (Some(q), None) => {
                let mut errors = Vec::new();
                let mut quotes = Vec::with_capacity(q.len());
                for (i, x) in q.iter().enumerate() {
                    match (x.kind.as_str(), x.frequency) {
                        ("deposit", None) => {
                            quotes.push(CurveQuote::Deposit { years: x.years, rate: x.rate });
                        }
                        ("swap", Some(frequency)) => {
                            quotes.push(CurveQuote::Swap { years: x.years, rate: x.rate, frequency });
                        }
                        ("deposit", Some(_)) => errors
                            .push((format!("quotes[{i}].frequency"), "a deposit has no frequency".into())),
                        ("swap", None) => {
                            errors
                                .push((format!("quotes[{i}].frequency"), "a swap needs a frequency".into()));
                        }
                        _ => errors.push((format!("quotes[{i}].type"), "must be deposit or swap".into())),
                    }
                }
                if !errors.is_empty() {
                    return Err(errors);
                }
                Curve::bootstrap(&quotes).map_err(|e| vec![("quotes".into(), e.to_string())])
            }
            (None, Some(r)) => Curve::flat(r, 50.0).map_err(|e| vec![("flat_rate".into(), e.to_string())]),
            _ => Err(vec![(String::new(), "give either quotes or flat_rate".into())]),
        }
    }
}

/// `POST /api/v1/pricing/curve` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveRequest {
    /// The curve.
    pub curve: CurveInput,
    /// Extra maturities (years) to report besides the nodes.
    #[serde(default)]
    pub at: Vec<f64>,
}

/// One point of a curve.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CurvePointView {
    /// Years.
    pub years: f64,
    /// Discount factor.
    pub discount_factor: f64,
    /// Continuously compounded zero rate.
    pub zero_rate: f64,
}

/// A bootstrapped curve.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CurveView {
    /// The curve's nodes (log-linear discount factors between them).
    pub nodes: Vec<CurvePointView>,
    /// The maturities asked for in `at`.
    pub points: Vec<CurvePointView>,
}

fn point(c: &Curve, t: f64) -> CurvePointView {
    CurvePointView { years: t, discount_factor: c.df(t), zero_rate: c.zero_rate(t) }
}

/// Bootstrap a discount curve from deposits and par swaps (or build a flat
/// one); every input quote reprices exactly.
#[utoipa::path(post, path = "/api/v1/pricing/curve", tag = "pricing", request_body = CurveRequest,
    responses(
        (status = 200, body = CurveView),
        (status = 422, description = "Every problem with the request", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn build_curve(body: Result<Bytes, BytesRejection>) -> Response {
    let req: CurveRequest = match read_body(body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let curve = match req.curve.build() {
        Ok(c) => c,
        Err(e) => return invalid_fields(&prefix("curve", e)),
    };
    if req.at.iter().any(|t| !t.is_finite() || *t < 0.0) {
        return invalid_fields(&[("at".into(), "maturities must be finite and not negative".into())]);
    }
    to_json(
        200,
        &CurveView {
            nodes: curve.nodes().iter().skip(1).map(|&(t, _)| point(&curve, t)).collect(),
            points: req.at.iter().map(|&t| point(&curve, t)).collect(),
        },
    )
}

/// `POST /api/v1/pricing/swap` body: a fixed-for-floating swap from today.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SwapRequest {
    /// The discount curve.
    pub curve: CurveInput,
    /// Notional.
    pub notional: f64,
    /// Fixed rate, as a fraction.
    pub fixed_rate: f64,
    /// Maturity, years.
    pub years: f64,
    /// Fixed payments a year: 1, 2, 4 or 12.
    pub frequency: u32,
    /// True for the side that pays fixed and receives floating.
    pub pay_fixed: bool,
}

/// A swap valued.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SwapView {
    /// Present value to the holder.
    pub pv: f64,
    /// The fixed rate at which the swap is worth zero.
    pub par_rate: f64,
    /// PV change for a one basis point parallel rise in zero rates.
    pub dv01: f64,
    /// PV of 1 a year on the fixed schedule.
    pub annuity: f64,
}

/// Value a fixed-for-floating interest-rate swap on a curve (single-curve).
#[utoipa::path(post, path = "/api/v1/pricing/swap", tag = "pricing", request_body = SwapRequest,
    responses(
        (status = 200, body = SwapView),
        (status = 422, description = "Every problem with the request", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn price_swap(body: Result<Bytes, BytesRejection>) -> Response {
    let req: SwapRequest = match read_body(body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let curve = match req.curve.build() {
        Ok(c) => c,
        Err(e) => return invalid_fields(&prefix("curve", e)),
    };
    let swap = Swap {
        notional: req.notional,
        fixed_rate: req.fixed_rate,
        years: req.years,
        frequency: req.frequency,
        pay_fixed: req.pay_fixed,
    };
    match swap.value(&curve) {
        Ok(v) => to_json(200, &SwapView { pv: v.pv, par_rate: v.par_rate, dv01: v.dv01, annuity: v.annuity }),
        Err(e) => invalid_fields(&[(String::new(), e.to_string())]),
    }
}
