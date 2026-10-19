//! Backtest configuration as data.
//!
//! Two JSON forms exist, on purpose:
//!  * the **user form** (this module): what the Studio form and the API send.
//!    Human units (cash 100000, slippage 2 bps, participation 0.1), every
//!    field optional, missing fields taken from a named preset, unknown fields
//!    refused, and every problem reported at once with its field name;
//!  * the **stored form** ([`stored_config_json`], which is
//!    [`spectra_registry::config_to_json`]): exact raw fixed-point integers,
//!    the form the registry already records and hashes.
//!
//! Both produce the same [`BacktestConfig`], so a run started from the API and
//! the same run started from Rust or the CLI have the same `config_sha256`.

use std::fmt;

use spectra_core::{Bps, Json, Money, Qty, Result};
use spectra_engine::{BacktestConfig, CommissionModel, CostModel, ExecutionConfig};

/// One problem with one field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Field name (as in the user form), or `""` for the document itself.
    pub field: String,
    /// What is wrong, in words a user can act on.
    pub message: String,
}

/// Every problem found in a configuration document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// One entry per problem, in field order.
    pub errors: Vec<FieldError>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> =
            self.errors
                .iter()
                .map(|e| {
                    if e.field.is_empty() {
                        e.message.clone()
                    } else {
                        format!("{}: {}", e.field, e.message)
                    }
                })
                .collect();
        write!(f, "invalid backtest configuration: {}", parts.join("; "))
    }
}

impl std::error::Error for ConfigError {}

impl ConfigError {
    /// The errors as a JSON array of `{field, message}` objects, for API responses.
    pub fn to_json(&self) -> Json {
        let mut out = Json::array();
        for e in &self.errors {
            // Pushing onto an array cannot fail.
            let _ = out.push(
                Json::object()
                    .with("field", Json::string(&e.field))
                    .with("message", Json::string(&e.message)),
            );
        }
        out
    }
}

/// Named starting points for a configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Long-only, no leverage, light costs: 1 bp commission, 2 bps slippage.
    Default,
    /// The engine's own defaults: no commission, no slippage. For comparing
    /// a strategy's edge before and after costs.
    ZeroCost,
    /// Heavier costs for a robustness check: 5 bps commission with a 1.00
    /// minimum fee, 10 bps slippage, and at most 10% of a bar's volume.
    ConservativeCosts,
}

impl Preset {
    /// Every preset, in display order.
    pub const ALL: [Self; 3] = [Self::Default, Self::ZeroCost, Self::ConservativeCosts];

    /// Name used in JSON (`"preset": "default"`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::ZeroCost => "zero_cost",
            Self::ConservativeCosts => "conservative_costs",
        }
    }

    /// Label for a form.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::ZeroCost => "Zero cost",
            Self::ConservativeCosts => "Conservative costs",
        }
    }

    /// The preset named `name`, if any.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The configuration this preset stands for.
    pub fn config(self) -> BacktestConfig {
        let base = BacktestConfig::default();
        match self {
            Self::ZeroCost => base,
            Self::Default => BacktestConfig {
                costs: CostModel {
                    commission: CommissionModel { bps_of_notional: bps(1), ..CommissionModel::default() },
                    slippage: bps(2),
                },
                ..base
            },
            Self::ConservativeCosts => BacktestConfig {
                costs: CostModel {
                    commission: CommissionModel {
                        bps_of_notional: bps(5),
                        min_fee: Money::ONE,
                        ..CommissionModel::default()
                    },
                    slippage: bps(10),
                },
                exec: ExecutionConfig {
                    max_participation: Qty::from_raw(spectra_core::SCALE / 10),
                    ..base.exec
                },
                ..base
            },
        }
    }
}

const fn bps(whole: i64) -> Bps {
    Bps::from_raw(whole * spectra_core::SCALE)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Number { min: f64, max: f64, min_exclusive: bool },
    Integer { min: i64, max: i64 },
    Bool,
    Seed,
    Leverage,
    Preset,
}

/// A field of the user form.
#[derive(Debug, Clone, Copy)]
struct Field {
    name: &'static str,
    kind: Kind,
    description: &'static str,
}

const fn num(min: f64, max: f64) -> Kind {
    Kind::Number { min, max, min_exclusive: false }
}
const fn num_positive(max: f64) -> Kind {
    Kind::Number { min: 0.0, max, min_exclusive: true }
}

/// Largest starting cash accepted: well inside the fixed-point range, so a
/// strategy can grow it a hundredfold without overflow.
const MAX_CASH: f64 = 1e9;

const FIELDS: [Field; 14] = [
    Field { name: "preset", kind: Kind::Preset, description: "Starting point for every field not given" },
    Field { name: "initial_cash", kind: num_positive(MAX_CASH), description: "Starting cash" },
    Field { name: "allow_short", kind: Kind::Bool, description: "Allow negative positions" },
    Field { name: "allow_negative_cash", kind: Kind::Bool, description: "Allow borrowing cash (margin)" },
    Field {
        name: "max_gross_leverage",
        kind: Kind::Leverage,
        description: "Gross exposure limit as a multiple of equity; null disables the check",
    },
    Field { name: "commission_per_unit", kind: num(0.0, 1e6), description: "Commission per unit traded" },
    Field {
        name: "commission_bps",
        kind: num(0.0, 1000.0),
        description: "Commission in basis points of notional",
    },
    Field { name: "commission_min", kind: num(0.0, 1e6), description: "Minimum commission per fill" },
    Field {
        name: "slippage_bps",
        kind: num(0.0, 1000.0),
        description: "Market-order slippage in basis points",
    },
    Field {
        name: "delay_bars",
        kind: Kind::Integer { min: 0, max: 1000 },
        description: "Extra bars an order waits before it can fill",
    },
    Field {
        name: "max_participation",
        kind: num(0.0, 1.0),
        description: "Largest share of a bar's volume one order may take (0 = no cap)",
    },
    Field { name: "seed", kind: Kind::Seed, description: "Seed passed to the strategy" },
    Field {
        name: "periods_per_year",
        kind: num_positive(1e8),
        description: "Bars per year, for annualizing",
    },
    Field {
        name: "risk_free_rate",
        kind: num(-1.0, 1.0),
        description: "Annual risk-free rate for the Sharpe ratio",
    },
];

#[derive(Default)]
struct Errors(Vec<FieldError>);

impl Errors {
    fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0.push(FieldError { field: field.into(), message: message.into() });
    }
}

fn read_number(
    j: &Json,
    f: &Field,
    min: f64,
    max: f64,
    min_exclusive: bool,
    errs: &mut Errors,
) -> Option<f64> {
    let Ok(v) = j.as_double() else {
        errs.add(f.name, "must be a number");
        return None;
    };
    let low_ok = if min_exclusive { v > min } else { v >= min };
    if !low_ok || v > max {
        let lower = if min_exclusive { format!("greater than {min}") } else { format!("at least {min}") };
        errs.add(f.name, format!("must be {lower} and at most {max} (got {v})"));
        return None;
    }
    Some(v)
}

fn read_seed(j: &Json, errs: &mut Errors) -> Option<u64> {
    let parsed = match j {
        Json::Int(i) => u64::try_from(*i).ok(),
        Json::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    };
    if parsed.is_none() {
        errs.add("seed", "must be a whole number from 0 to 18446744073709551615 (as a number or a string)");
    }
    parsed
}

/// Fixed-point value of a checked number; the range checks above keep it in range.
fn fixed<U: spectra_core::fixed::Unit>(
    name: &str,
    v: f64,
    errs: &mut Errors,
) -> Option<spectra_core::Fixed<U>> {
    match spectra_core::Fixed::<U>::from_f64(v) {
        Ok(x) => Some(x),
        Err(e) => {
            errs.add(name, e.to_string());
            None
        }
    }
}

/// Parses the user form into a configuration.
///
/// Missing fields come from `preset` (default `"default"`). Unknown fields,
/// wrong types and out-of-range values are all reported, not just the first.
///
/// # Errors
/// [`ConfigError`] listing every problem.
pub fn parse_config(doc: &Json) -> std::result::Result<BacktestConfig, ConfigError> {
    let mut errs = Errors::default();
    let Ok(entries) = doc.entries() else {
        errs.add("", "configuration must be a JSON object");
        return Err(ConfigError { errors: errs.0 });
    };

    for (k, _) in entries {
        if !FIELDS.iter().any(|f| f.name == k) {
            errs.add(k, "unknown field");
        }
    }

    let preset = match doc.find("preset") {
        None | Some(Json::Null) => Preset::Default,
        Some(Json::String(s)) => Preset::from_name(s).unwrap_or_else(|| {
            let names: Vec<&str> = Preset::ALL.iter().map(|p| p.name()).collect();
            errs.add("preset", format!("unknown preset \"{s}\"; expected one of {}", names.join(", ")));
            Preset::Default
        }),
        Some(_) => {
            errs.add("preset", "must be a string");
            Preset::Default
        }
    };
    let mut c = preset.config();

    for f in &FIELDS {
        let Some(v) = doc.find(f.name) else { continue };
        match (f.name, f.kind) {
            ("preset", _) => {}
            (_, Kind::Bool) => match v.as_bool() {
                Ok(b) if f.name == "allow_short" => c.ledger.allow_short = b,
                Ok(b) => c.ledger.allow_negative_cash = b,
                Err(_) => errs.add(f.name, "must be true or false"),
            },
            (_, Kind::Leverage) => {
                if v.is_null() {
                    c.ledger.max_gross_leverage = None;
                } else if let Some(x) = read_number(v, f, 0.0, 100.0, true, &mut errs) {
                    c.ledger.max_gross_leverage =
                        fixed::<spectra_core::fixed::RatioUnit>(f.name, x, &mut errs);
                }
            }
            (_, Kind::Seed) => {
                if let Some(s) = read_seed(v, &mut errs) {
                    c.seed = s;
                }
            }
            (_, Kind::Integer { min, max }) => match v.as_int() {
                Ok(n) if (min..=max).contains(&n) => c.exec.delay_bars = n as usize,
                _ => errs.add(f.name, format!("must be a whole number from {min} to {max}")),
            },
            (name, Kind::Number { min, max, min_exclusive }) => {
                let Some(x) = read_number(v, f, min, max, min_exclusive, &mut errs) else { continue };
                match name {
                    "initial_cash" => {
                        if let Some(m) = fixed(name, x, &mut errs) {
                            c.initial_cash = m;
                        }
                    }
                    "commission_per_unit" => {
                        if let Some(m) = fixed(name, x, &mut errs) {
                            c.costs.commission.per_unit = m;
                        }
                    }
                    "commission_bps" => {
                        if let Some(b) = fixed(name, x, &mut errs) {
                            c.costs.commission.bps_of_notional = b;
                        }
                    }
                    "commission_min" => {
                        if let Some(m) = fixed(name, x, &mut errs) {
                            c.costs.commission.min_fee = m;
                        }
                    }
                    "slippage_bps" => {
                        if let Some(b) = fixed(name, x, &mut errs) {
                            c.costs.slippage = b;
                        }
                    }
                    "max_participation" => {
                        if let Some(q) = fixed(name, x, &mut errs) {
                            c.exec.max_participation = q;
                        }
                    }
                    "periods_per_year" => c.periods_per_year = x,
                    "risk_free_rate" => c.risk_free_rate = x,
                    _ => errs.add(name, "internal: field has no target"),
                }
            }
            (name, Kind::Preset) => errs.add(name, "internal: unexpected preset field"),
        }
    }

    if errs.0.is_empty() {
        Ok(c)
    } else {
        Err(ConfigError { errors: errs.0 })
    }
}

/// The user form of a configuration: every field, in human units. Parsing the
/// output with [`parse_config`] gives back the same configuration for any value
/// a form would hold (fixed-point values pass through `f64`).
///
/// # Errors
/// If a value is not finite (impossible for a configuration that ran).
pub fn config_to_user_json(c: &BacktestConfig) -> Result<Json> {
    let seed = i64::try_from(c.seed).map_or_else(|_| Json::string(c.seed.to_string()), Json::integer);
    let leverage = match c.ledger.max_gross_leverage {
        Some(l) => Json::number(l.to_f64())?,
        None => Json::Null,
    };
    Ok(Json::object()
        .with("initial_cash", Json::number(c.initial_cash.to_f64())?)
        .with("allow_short", Json::boolean(c.ledger.allow_short))
        .with("allow_negative_cash", Json::boolean(c.ledger.allow_negative_cash))
        .with("max_gross_leverage", leverage)
        .with("commission_per_unit", Json::number(c.costs.commission.per_unit.to_f64())?)
        .with("commission_bps", Json::number(c.costs.commission.bps_of_notional.to_f64())?)
        .with("commission_min", Json::number(c.costs.commission.min_fee.to_f64())?)
        .with("slippage_bps", Json::number(c.costs.slippage.to_f64())?)
        .with("delay_bars", Json::integer(c.exec.delay_bars as i64))
        .with("max_participation", Json::number(c.exec.max_participation.to_f64())?)
        .with("seed", seed)
        .with("periods_per_year", Json::number(c.periods_per_year)?)
        .with("risk_free_rate", Json::number(c.risk_free_rate)?))
}

/// The exact stored form recorded with a run (raw fixed-point integers);
/// the same document the registry writes.
///
/// # Errors
/// If a value is not finite.
pub fn stored_config_json(c: &BacktestConfig) -> Result<Json> {
    spectra_registry::config_to_json(c)
}

/// Parses [`stored_config_json`] output.
///
/// # Errors
/// [`spectra_core::Error::Registry`] for missing or malformed fields.
pub fn config_from_stored_json(j: &Json) -> Result<BacktestConfig> {
    spectra_registry::config_from_json(j)
}

/// A JSON Schema (draft 2020-12) of the user form, with the default preset's
/// values as defaults, for building the Studio form.
///
/// # Errors
/// Never in practice (all bounds are finite).
pub fn config_schema() -> Result<Json> {
    let defaults = config_to_user_json(&Preset::Default.config())?;
    let mut props = Json::object();
    for f in &FIELDS {
        let mut p = Json::object().with("description", Json::string(f.description));
        p = match f.kind {
            Kind::Number { min, max, min_exclusive } => {
                let lower = if min_exclusive { "exclusiveMinimum" } else { "minimum" };
                p.with("type", Json::string("number"))
                    .with(lower, Json::number(min)?)
                    .with("maximum", Json::number(max)?)
            }
            Kind::Integer { min, max } => p
                .with("type", Json::string("integer"))
                .with("minimum", Json::integer(min))
                .with("maximum", Json::integer(max)),
            Kind::Bool => p.with("type", Json::string("boolean")),
            Kind::Leverage => {
                let mut types = Json::array();
                types.push(Json::string("number"))?;
                types.push(Json::string("null"))?;
                p.with("type", types)
                    .with("exclusiveMinimum", Json::integer(0))
                    .with("maximum", Json::integer(100))
            }
            Kind::Seed => {
                let mut types = Json::array();
                types.push(Json::string("integer"))?;
                types.push(Json::string("string"))?;
                p.with("type", types)
                    .with("minimum", Json::integer(0))
                    .with("pattern", Json::string("^[0-9]{1,20}$"))
            }
            Kind::Preset => {
                let mut names = Json::array();
                for preset in Preset::ALL {
                    names.push(Json::string(preset.name()))?;
                }
                p.with("type", Json::string("string")).with("enum", names)
            }
        };
        let default = if f.name == "preset" {
            Some(Json::string(Preset::Default.name()))
        } else {
            defaults.find(f.name).cloned()
        };
        if let Some(d) = default {
            p = p.with("default", d);
        }
        props.set(f.name, p)?;
    }
    Ok(Json::object()
        .with("$schema", Json::string("https://json-schema.org/draft/2020-12/schema"))
        .with("title", Json::string("Backtest configuration"))
        .with("type", Json::string("object"))
        .with("additionalProperties", Json::boolean(false))
        .with("properties", props))
}

/// Presets as JSON: `[{name, label, config}]`, `config` in the user form.
///
/// # Errors
/// Never in practice.
pub fn presets_json() -> Result<Json> {
    let mut out = Json::array();
    for p in Preset::ALL {
        out.push(
            Json::object()
                .with("name", Json::string(p.name()))
                .with("label", Json::string(p.label()))
                .with("config", config_to_user_json(&p.config())?),
        )?;
    }
    Ok(out)
}
