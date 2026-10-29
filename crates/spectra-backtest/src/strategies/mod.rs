//! Built-in strategies, addressable by name with typed parameters.
//!
//! This is the slice's stand-in for M2's strategy versions: the API lists
//! these for the Studio form, and the backtest job builds one from
//! `(name, params)`. When M2 lands, its versions replace this list behind the
//! same shape (name, parameter schema, source hash, factory).
//!
//! Each strategy lives in its own file, and that file's text is hashed into
//! [`BuiltinStrategy::source_sha256`], so any change to a strategy's code
//! changes the `strategy_sha256` recorded with its runs.

mod buy_and_hold;
mod equal_weight;
mod sma_cross;
mod zscore_reversion;

use spectra_core::{sha256_hex, Error, Json, Result};
use spectra_engine::Strategy;
use spectra_research::ParamSet;

use crate::config::{ConfigError, FieldError};

/// The type of a parameter. All values travel as `f64` in a [`ParamSet`];
/// integers are whole numbers and booleans are 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// A whole number.
    Int,
    /// Any number.
    Float,
    /// 0 (false) or 1 (true).
    Bool,
}

impl ParamKind {
    /// Name used in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
        }
    }
}

/// One parameter of a built-in strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Name, as in [`ParamSet`] and JSON.
    pub name: &'static str,
    /// Type.
    pub kind: ParamKind,
    /// Value when not given.
    pub default: f64,
    /// Smallest allowed value (inclusive).
    pub min: f64,
    /// Largest allowed value (inclusive).
    pub max: f64,
    /// One line for the form.
    pub description: &'static str,
}

pub type BuildFn = fn(&ParamSet) -> Result<Box<dyn Strategy>>;
pub type CheckFn = fn(&ParamSet) -> Option<FieldError>;

/// A strategy shipped with the engine.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinStrategy {
    /// Stable identifier, e.g. `"sma_cross"`.
    pub name: &'static str,
    /// Label for the form.
    pub label: &'static str,
    /// What it does, in a sentence or two.
    pub description: &'static str,
    /// Parameters in display (and canonical) order.
    pub params: &'static [ParamSpec],
    source: &'static str,
    build: BuildFn,
    check: CheckFn,
}

impl BuiltinStrategy {
    /// Creates a custom or test builtin strategy definition.
    #[doc(hidden)]
    pub const fn custom(
        name: &'static str,
        label: &'static str,
        description: &'static str,
        params: &'static [ParamSpec],
        source: &'static str,
        build: BuildFn,
        check: CheckFn,
    ) -> Self {
        Self { name, label, description, params, source, build, check }
    }

    /// SHA-256 of the strategy's source file, recorded as `strategy_sha256`.
    pub fn source_sha256(&self) -> String {
        sha256_hex(self.source.as_bytes())
    }

    /// Fills in defaults and checks every parameter: unknown names, types,
    /// ranges and the strategy's own cross-parameter rules. The result lists
    /// every parameter in [`Self::params`] order, so its
    /// [`ParamSet::canonical`] form is stable.
    ///
    /// # Errors
    /// [`ConfigError`] with fields named `params.<name>`.
    pub fn resolve_params(&self, given: &ParamSet) -> std::result::Result<ParamSet, ConfigError> {
        if self.name == "dsl" {
            return Ok(given.clone());
        }
        let mut errors = Vec::new();
        let field = |n: &str| format!("params.{n}");
        for (k, _) in &given.values {
            if !self.params.iter().any(|p| p.name == k) {
                errors.push(FieldError { field: field(k), message: "unknown parameter".into() });
            }
        }
        let mut out = ParamSet::default();
        for spec in self.params {
            let v = given.values.iter().find(|(k, _)| k == spec.name).map_or(spec.default, |(_, v)| *v);
            let problem = if !v.is_finite() {
                Some("must be a finite number".to_string())
            } else if spec.kind != ParamKind::Float && v.fract() != 0.0 {
                Some(match spec.kind {
                    ParamKind::Bool => "must be true or false (1 or 0)".to_string(),
                    _ => "must be a whole number".to_string(),
                })
            } else if v < spec.min || v > spec.max {
                Some(format!("must be from {} to {} (got {v})", spec.min, spec.max))
            } else {
                None
            };
            if let Some(message) = problem {
                errors.push(FieldError { field: field(spec.name), message });
            }
            out.values.push((spec.name.to_string(), v));
        }
        if errors.is_empty() {
            if let Some(e) = (self.check)(&out) {
                errors.push(e);
            }
        }
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(ConfigError { errors })
        }
    }

    /// Parameters from a JSON object of numbers and booleans, then
    /// [`Self::resolve_params`]. `null` or a missing document means all defaults.
    ///
    /// # Errors
    /// [`ConfigError`] for a malformed document or invalid values.
    pub fn resolve_params_json(&self, doc: Option<&Json>) -> std::result::Result<ParamSet, ConfigError> {
        let mut given = ParamSet::default();
        let Some(doc) = doc.filter(|d| !d.is_null()) else { return self.resolve_params(&given) };
        let Ok(entries) = doc.entries() else {
            return Err(ConfigError {
                errors: vec![FieldError { field: "params".into(), message: "must be a JSON object".into() }],
            });
        };
        let mut errors = Vec::new();
        for (k, v) in entries {
            let x = match v {
                Json::Bool(b) => Some(f64::from(u8::from(*b))),
                other => other.as_double().ok(),
            };
            match x {
                Some(x) => given.values.push((k.clone(), x)),
                None => errors.push(FieldError {
                    field: format!("params.{k}"),
                    message: "must be a number or a boolean".into(),
                }),
            }
        }
        match self.resolve_params(&given) {
            Ok(p) if errors.is_empty() => Ok(p),
            Ok(_) => Err(ConfigError { errors }),
            Err(mut e) => {
                errors.append(&mut e.errors);
                Err(ConfigError { errors })
            }
        }
    }

    /// Builds a fresh strategy object. Parameters are checked first.
    ///
    /// # Errors
    /// [`Error::Strategy`] for invalid parameters.
    pub fn build(&self, params: &ParamSet) -> Result<Box<dyn Strategy>> {
        let p = self.resolve_params(params).map_err(|e| Error::Strategy(format!("{}: {e}", self.name)))?;
        (self.build)(&p)
    }

    /// Name, label, description, parameter schema and source hash, for the API.
    ///
    /// # Errors
    /// Never in practice (all bounds are finite).
    pub fn to_json(&self) -> Result<Json> {
        let mut params = Json::array();
        for p in self.params {
            let num = |v: f64| -> Result<Json> {
                if p.kind == ParamKind::Float {
                    Json::number(v)
                } else {
                    Ok(Json::integer(v as i64))
                }
            };
            let default =
                if p.kind == ParamKind::Bool { Json::boolean(p.default != 0.0) } else { num(p.default)? };
            let mut j = Json::object()
                .with("name", Json::string(p.name))
                .with("type", Json::string(p.kind.as_str()))
                .with("default", default);
            if p.kind != ParamKind::Bool {
                j = j.with("min", num(p.min)?).with("max", num(p.max)?);
            }
            params.push(j.with("description", Json::string(p.description)))?;
        }
        Ok(Json::object()
            .with("name", Json::string(self.name))
            .with("label", Json::string(self.label))
            .with("description", Json::string(self.description))
            .with("params", params)
            .with("strategy_sha256", Json::string(self.source_sha256())))
    }
}

const BUILTINS: [BuiltinStrategy; 4] = [
    BuiltinStrategy {
        name: "buy_and_hold",
        label: "Buy and hold",
        description:
            "Buys equal cash amounts of every instrument on the first bar and holds. A baseline to beat.",
        params: buy_and_hold::PARAMS,
        source: include_str!("buy_and_hold.rs"),
        build: buy_and_hold::build,
        check: buy_and_hold::check,
    },
    BuiltinStrategy {
        name: "equal_weight",
        label: "Equal weight",
        description: "Holds every instrument at the same share of equity, rebalanced on a schedule or when \
                      weights drift. Uses target-weight mode.",
        params: equal_weight::PARAMS,
        source: include_str!("equal_weight.rs"),
        build: equal_weight::build,
        check: equal_weight::check,
    },
    BuiltinStrategy {
        name: "sma_cross",
        label: "SMA crossover",
        description:
            "Long when the fast simple moving average of closes is above the slow one, flat (or short) \
                      otherwise. Trades every instrument independently.",
        params: sma_cross::PARAMS,
        source: include_str!("sma_cross.rs"),
        build: sma_cross::build,
        check: sma_cross::check,
    },
    BuiltinStrategy {
        name: "zscore_reversion",
        label: "Z-score mean reversion",
        description:
            "Buys when the close is far below its rolling mean (in standard deviations) and exits as it \
                      reverts; optionally the mirror image on the short side.",
        params: zscore_reversion::PARAMS,
        source: include_str!("zscore_reversion.rs"),
        build: zscore_reversion::build,
        check: zscore_reversion::check,
    },
];

static DSL_STRATEGY: BuiltinStrategy = BuiltinStrategy {
    name: "dsl",
    label: "DSL strategy",
    description: "User-authored strategy written in the Spectra restricted rule language.",
    params: &[],
    source: "// Spectra Restricted Strategy DSL",
    build: |_| Err(spectra_core::Error::Strategy("DSL strategies require dsl_code in request".into())),
    check: |_| None,
};

/// Every built-in strategy.
pub fn builtin_strategies() -> &'static [BuiltinStrategy] {
    &BUILTINS
}

/// The built-in strategy called `name`.
pub fn find_builtin(name: &str) -> Option<&'static BuiltinStrategy> {
    if name == "dsl" {
        return Some(&DSL_STRATEGY);
    }
    BUILTINS.iter().find(|s| s.name == name)
}

/// The built-in strategy called `name`, or an error listing the valid names.
///
/// # Errors
/// [`Error::Strategy`] for unknown names.
pub fn builtin(name: &str) -> Result<&'static BuiltinStrategy> {
    find_builtin(name).ok_or_else(|| {
        let names: Vec<&str> = BUILTINS.iter().map(|s| s.name).collect();
        Error::Strategy(format!("unknown strategy \"{name}\"; built-in strategies are {}", names.join(", ")))
    })
}

/// A factory for `name`, usable wherever spectra-research takes a
/// [`spectra_research::StrategyFactory`] (sweeps, walk-forward, the vault).
///
/// # Errors
/// [`Error::Strategy`] for unknown names.
pub fn builtin_factory(name: &str) -> Result<impl Fn(&ParamSet) -> Result<Box<dyn Strategy>> + Sync> {
    let s = builtin(name)?;
    Ok(move |p: &ParamSet| s.build(p))
}

/// `[{name, label, description, params, strategy_sha256}]` for every built-in.
///
/// # Errors
/// Never in practice.
pub fn builtins_json() -> Result<Json> {
    let mut out = Json::array();
    for s in &BUILTINS {
        out.push(s.to_json()?)?;
    }
    Ok(out)
}

/// Shared helpers for the strategy files.
mod util {
    use spectra_core::{Bar, Qty};

    /// Mean of the last `n` closes as an exact raw integer (truncated), the
    /// same arithmetic as the reference demos so results match them.
    pub(super) fn sma_raw(history: &[Bar], n: usize) -> i128 {
        history[history.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128
    }

    /// A quantity parameter as fixed point (parameters are validated finite and in range).
    pub(super) fn qty(v: f64) -> Qty {
        Qty::from_f64(v).unwrap_or(Qty::ZERO)
    }
}
