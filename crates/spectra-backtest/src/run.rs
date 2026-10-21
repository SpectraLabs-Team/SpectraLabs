//! A backtest request as data, and running one.
//!
//! The request document is what `POST /api/v1/backtests` will accept and what
//! the backtest job stores:
//!
//! ```json
//! {
//!   "strategy": "sma_cross",
//!   "params": { "fast": 10, "slow": 30 },
//!   "config": { "preset": "default", "initial_cash": 250000 }
//! }
//! ```
//!
//! `params` and `config` are optional (all defaults). The dataset is not part
//! of the request: the job loads bars from the dataset store and passes them in.
//!
//! Several strategies can share one account instead, each with a share of the
//! starting cash (see [`spectra_engine::run_multi_backtest`]):
//!
//! ```json
//! {
//!   "strategies": [
//!     { "strategy": "sma_cross", "params": { "fast": 10, "slow": 30 }, "weight": 0.6 },
//!     { "strategy": "zscore_reversion", "weight": 0.4 }
//!   ],
//!   "config": { "preset": "default" }
//! }
//! ```

use spectra_core::{Bar, Json, Ratio, Result, SCALE};
use spectra_engine::{
    run_backtest_with_actions, run_multi_backtest_with_actions, Allocation, BacktestConfig, BacktestResult,
    RunControl, RunError,
};
use spectra_research::ParamSet;

use crate::config::{config_to_user_json, parse_config, stored_config_json, ConfigError, FieldError};
use crate::corporate::{actions_to_json, parse_actions, resolve, RequestAction};
use crate::strategies::{builtin, BuiltinStrategy};

/// A validated request: a known strategy, complete parameters and a configuration.
#[derive(Debug, Clone)]
pub struct BacktestRequest {
    /// The strategy to run; the first one in a multi-strategy request.
    pub strategy: &'static BuiltinStrategy,
    /// Every parameter of `strategy`, defaults filled in, in the strategy's order.
    pub params: ParamSet,
    /// Engine configuration.
    pub config: BacktestConfig,
    /// Every strategy of a multi-strategy request, in order; empty for a
    /// single-strategy request.
    pub strategies: Vec<StrategyAllocation>,
    /// Corporate actions to apply during the run (unadjusted prices); empty
    /// for none. See [`crate::corporate`].
    pub corporate_actions: Vec<RequestAction>,
    /// Optional point-in-time snapshot timestamp (nanoseconds since Unix epoch).
    /// When set, bars or revisions known after `as_of` are excluded from the backtest.
    pub as_of: Option<i64>,
}

/// One strategy of a multi-strategy request.
#[derive(Debug, Clone)]
pub struct StrategyAllocation {
    /// The strategy.
    pub strategy: &'static BuiltinStrategy,
    /// Every parameter, defaults filled in.
    pub params: ParamSet,
    /// Share of the starting cash, in (0, 1].
    pub weight: f64,
}

/// Most strategies in one request.
pub const MAX_REQUEST_STRATEGIES: usize = 10;

const REQUEST_FIELDS: [&str; 6] =
    ["strategy", "params", "strategies", "config", "corporate_actions", "as_of"];
const ENTRY_FIELDS: [&str; 3] = ["strategy", "params", "weight"];

fn field_error(field: impl Into<String>, message: impl Into<String>) -> FieldError {
    FieldError { field: field.into(), message: message.into() }
}

/// Reads `strategy` from an object, recording a problem under `prefix`.
fn read_strategy(doc: &Json, prefix: &str, errors: &mut Vec<FieldError>) -> Option<&'static BuiltinStrategy> {
    let field = format!("{prefix}strategy");
    match doc.find("strategy").map(Json::as_str) {
        Some(Ok(name)) => builtin(name).map_err(|e| errors.push(field_error(&field, e.to_string()))).ok(),
        Some(Err(_)) => {
            errors.push(field_error(field, "must be a string"));
            None
        }
        None => {
            errors.push(field_error(field, "is required"));
            None
        }
    }
}

/// Parses the `strategies` list, reporting every problem.
fn read_strategies(list: &Json, errors: &mut Vec<FieldError>) -> Option<Vec<StrategyAllocation>> {
    let Ok(items) = list.items() else {
        errors.push(field_error("strategies", "must be a list"));
        return None;
    };
    if items.is_empty() || items.len() > MAX_REQUEST_STRATEGIES {
        errors.push(field_error("strategies", format!("must list 1 to {MAX_REQUEST_STRATEGIES} strategies")));
        return None;
    }
    let before = errors.len();
    let mut out = Vec::with_capacity(items.len());
    let mut total: i128 = 0;
    for (i, item) in items.iter().enumerate() {
        let prefix = format!("strategies[{i}].");
        let Ok(entries) = item.entries() else {
            errors.push(field_error(format!("strategies[{i}]"), "must be a JSON object"));
            continue;
        };
        for (k, _) in entries {
            if !ENTRY_FIELDS.contains(&k.as_str()) {
                errors.push(field_error(format!("{prefix}{k}"), "unknown field"));
            }
        }
        let strategy = read_strategy(item, &prefix, errors);
        let params = strategy.and_then(|s| {
            s.resolve_params_json(item.find("params"))
                .map_err(|e| {
                    errors.extend(
                        e.errors.into_iter().map(|f| field_error(format!("{prefix}{}", f.field), f.message)),
                    );
                })
                .ok()
        });
        let weight = match item.find("weight").map(Json::as_double) {
            Some(Ok(w)) if w.is_finite() && w > 0.0 && w <= 1.0 => match Ratio::from_f64(w) {
                Ok(r) if r.raw() > 0 => {
                    total += i128::from(r.raw());
                    Some(w)
                }
                _ => {
                    errors.push(field_error(format!("{prefix}weight"), "is too small"));
                    None
                }
            },
            Some(Ok(w)) => {
                errors.push(field_error(
                    format!("{prefix}weight"),
                    format!("must be greater than 0 and at most 1 (got {w})"),
                ));
                None
            }
            Some(Err(_)) => {
                errors.push(field_error(format!("{prefix}weight"), "must be a number"));
                None
            }
            None => {
                errors.push(field_error(format!("{prefix}weight"), "is required"));
                None
            }
        };
        if let (Some(strategy), Some(params), Some(weight)) = (strategy, params, weight) {
            out.push(StrategyAllocation { strategy, params, weight });
        }
    }
    if total > i128::from(SCALE) {
        errors.push(field_error("strategies", "weights add up to more than 1"));
    }
    (errors.len() == before).then_some(out)
}

impl BacktestRequest {
    /// Builds a request in code, validating the parameters.
    ///
    /// # Errors
    /// [`ConfigError`] for an unknown strategy or invalid parameters.
    pub fn new(
        strategy: &str,
        params: &ParamSet,
        config: BacktestConfig,
    ) -> std::result::Result<Self, ConfigError> {
        let s = builtin(strategy).map_err(|e| ConfigError {
            errors: vec![FieldError { field: "strategy".into(), message: e.to_string() }],
        })?;
        Ok(Self {
            strategy: s,
            params: s.resolve_params(params)?,
            config,
            strategies: Vec::new(),
            corporate_actions: Vec::new(),
            as_of: None,
        })
    }

    /// Builds a multi-strategy request in code: `(strategy, params, weight)`
    /// for each strategy, validated like [`Self::from_json`].
    ///
    /// # Errors
    /// [`ConfigError`] for unknown strategies, invalid parameters or weights.
    pub fn multi(
        strategies: &[(&str, ParamSet, f64)],
        config: BacktestConfig,
    ) -> std::result::Result<Self, ConfigError> {
        let mut list = Json::array();
        for (name, params, weight) in strategies {
            let w = Json::number(*weight).map_err(|_| ConfigError {
                errors: vec![field_error("strategies", "weights must be finite")],
            })?;
            let p = spectra_registry::params_to_json(params)
                .map_err(|e| ConfigError { errors: vec![field_error("strategies", e.to_string())] })?;
            list.push(
                Json::object().with("strategy", Json::string(*name)).with("params", p).with("weight", w),
            )
            .map_err(|e| ConfigError { errors: vec![field_error("strategies", e.to_string())] })?;
        }
        let mut errors = Vec::new();
        match read_strategies(&list, &mut errors) {
            Some(entries) if errors.is_empty() => Ok(Self {
                strategy: entries[0].strategy,
                params: entries[0].params.clone(),
                config,
                strategies: entries,
                corporate_actions: Vec::new(),
                as_of: None,
            }),
            _ => Err(ConfigError { errors }),
        }
    }

    /// True for a request with a `strategies` list.
    pub fn is_multi(&self) -> bool {
        !self.strategies.is_empty()
    }

    /// The strategy's source hash; for a multi-strategy request, the hash of
    /// every strategy's name, source hash and weight, in order.
    pub fn strategy_sha256(&self) -> String {
        if !self.is_multi() {
            return self.strategy.source_sha256();
        }
        let mut text = String::from("spectra-strategies-v1;");
        for a in &self.strategies {
            let w = Ratio::from_f64(a.weight).map_or(0, spectra_core::Fixed::raw);
            text.push_str(&format!("{}:{}:{};", a.strategy.name, a.strategy.source_sha256(), w));
        }
        spectra_core::sha256_hex(text.as_bytes())
    }

    /// Parses a request document (see the module documentation), reporting
    /// every problem in the strategy, parameters and configuration at once.
    ///
    /// # Errors
    /// [`ConfigError`]; configuration fields are prefixed `config.`.
    pub fn from_json(doc: &Json) -> std::result::Result<Self, ConfigError> {
        let mut errors = Vec::new();
        let Ok(entries) = doc.entries() else {
            return Err(ConfigError {
                errors: vec![FieldError {
                    field: String::new(),
                    message: "request must be a JSON object".into(),
                }],
            });
        };
        for (k, _) in entries {
            if !REQUEST_FIELDS.contains(&k.as_str()) {
                errors.push(FieldError { field: k.clone(), message: "unknown field".into() });
            }
        }

        let multi = match doc.find("strategies") {
            None | Some(Json::Null) => None,
            Some(list) => {
                for field in ["strategy", "params"] {
                    if doc.find(field).is_some() {
                        errors.push(field_error(field, "give either strategy and params, or strategies"));
                    }
                }
                Some(read_strategies(list, &mut errors))
            }
        };
        let strategy = if multi.is_some() { None } else { read_strategy(doc, "", &mut errors) };

        let params = strategy.map(|s| s.resolve_params_json(doc.find("params")));
        let actions = match doc.find("corporate_actions") {
            None | Some(Json::Null) => Some(Vec::new()),
            Some(list) => parse_actions(list, &mut errors),
        };
        let config = match doc.find("config") {
            None | Some(Json::Null) => Ok(crate::config::Preset::Default.config()),
            Some(c) => parse_config(c),
        };

        let params = match params {
            Some(Ok(p)) => Some(p),
            Some(Err(mut e)) => {
                errors.append(&mut e.errors);
                None
            }
            None => None,
        };
        let config = match config {
            Ok(c) => Some(c),
            Err(e) => {
                errors.extend(e.errors.into_iter().map(|f| FieldError {
                    field: if f.field.is_empty() { "config".into() } else { format!("config.{}", f.field) },
                    message: f.message,
                }));
                None
            }
        };

        let as_of = match doc.find("as_of") {
            None | Some(Json::Null) => None,
            Some(Json::Int(ts)) => {
                if *ts < 0 {
                    errors.push(field_error("as_of", "must be a non-negative timestamp"));
                    None
                } else {
                    Some(*ts)
                }
            }
            Some(Json::String(s)) => {
                let trimmed = s.trim();
                if let Some(ts) = spectra_data::parse_timestamp(
                    trimmed,
                    &spectra_data::TimeOptions {
                        format: spectra_data::TimeFormat::Iso8601,
                        naive_utc_offset_minutes: 0,
                        timezone: None,
                    },
                ) {
                    Some(ts.ts_ns)
                } else if let Ok(val) = trimmed.parse::<i64>() {
                    if val < 0 {
                        errors.push(field_error("as_of", "must be a non-negative timestamp"));
                        None
                    } else {
                        Some(val)
                    }
                } else {
                    errors.push(field_error(
                        "as_of",
                        format!("cannot parse timestamp '{trimmed}'; expected ISO 8601 or epoch integer"),
                    ));
                    None
                }
            }
            Some(_) => {
                errors.push(field_error("as_of", "must be an ISO 8601 string or integer timestamp"));
                None
            }
        };

        match (multi, strategy, params, config, actions) {
            (Some(Some(entries)), _, _, Some(config), Some(corporate_actions)) if errors.is_empty() => {
                Ok(Self {
                    strategy: entries[0].strategy,
                    params: entries[0].params.clone(),
                    config,
                    strategies: entries,
                    corporate_actions,
                    as_of,
                })
            }
            (None, Some(strategy), Some(params), Some(config), Some(corporate_actions))
                if errors.is_empty() =>
            {
                Ok(Self { strategy, params, config, strategies: Vec::new(), corporate_actions, as_of })
            }
            _ => Err(ConfigError { errors }),
        }
    }

    /// The request as it should be stored with the run: strategy name and
    /// source hash, the complete parameters, and the configuration in both its
    /// user form and its exact stored form.
    ///
    /// # Errors
    /// If a value is not finite (impossible for a validated request).
    pub fn to_json(&self) -> Result<Json> {
        let mut doc = self.to_json_without_actions()?;
        if let Some(ts) = self.as_of {
            doc = doc.with("as_of", Json::integer(ts));
        }
        if self.corporate_actions.is_empty() {
            return Ok(doc);
        }
        Ok(doc.with("corporate_actions", actions_to_json(&self.corporate_actions)?))
    }

    fn to_json_without_actions(&self) -> Result<Json> {
        if self.is_multi() {
            let mut list = Json::array();
            for a in &self.strategies {
                list.push(
                    Json::object()
                        .with("strategy", Json::string(a.strategy.name))
                        .with("strategy_sha256", Json::string(a.strategy.source_sha256()))
                        .with("params", spectra_registry::params_to_json(&a.params)?)
                        .with("weight", Json::number(a.weight)?),
                )?;
            }
            return Ok(Json::object()
                .with("strategies", list)
                .with("strategy_sha256", Json::string(self.strategy_sha256()))
                .with("config", config_to_user_json(&self.config)?)
                .with("config_exact", stored_config_json(&self.config)?));
        }
        Ok(Json::object()
            .with("strategy", Json::string(self.strategy.name))
            .with("strategy_sha256", Json::string(self.strategy.source_sha256()))
            .with("params", spectra_registry::params_to_json(&self.params)?)
            .with("config", config_to_user_json(&self.config)?)
            .with("config_exact", stored_config_json(&self.config)?))
    }
}

/// A finished run and what produced it.
#[derive(Debug, Clone)]
pub struct BacktestOutput {
    /// The request that was run.
    pub request: BacktestRequest,
    /// The engine's result.
    pub result: BacktestResult,
    /// Equal-weight buy-and-hold returns of the same bars, one per period of
    /// the equity curve (see [`spectra_research::benchmark_returns`]).
    pub benchmark: Vec<f64>,
    /// Symbol of each instrument id, when known (for per-instrument output).
    pub symbols: Vec<String>,
}

/// Builds the strategy and runs it under `ctl`, on a dataset whose symbols
/// are unknown (corporate actions must then name instruments by id).
///
/// # Errors
/// As [`run_request_on`].
pub fn run_request(
    bars: &[Bar],
    request: &BacktestRequest,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestOutput, RunError> {
    run_request_on(bars, &[], request, ctl)
}

/// Builds the strategy and runs it under `ctl`. `symbols` (instrument id =
/// position) resolves corporate actions given by symbol and labels the
/// per-instrument output.
///
/// # Errors
/// [`RunError`] as from [`spectra_engine::run_backtest_controlled`]; strategy
/// construction problems and corporate actions naming a symbol the dataset
/// does not have come back as [`RunError::Engine`].
pub fn run_request_on(
    bars: &[Bar],
    symbols: &[String],
    request: &BacktestRequest,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestOutput, RunError> {
    let actions = resolve(&request.corporate_actions, symbols)
        .map_err(|m| RunError::Engine(spectra_core::Error::InvalidArgument(m)))?;
    let result = if request.is_multi() {
        let mut built = Vec::with_capacity(request.strategies.len());
        for a in &request.strategies {
            built.push(a.strategy.build(&a.params)?);
        }
        let mut allocations: Vec<Allocation<'_>> = built
            .iter_mut()
            .zip(&request.strategies)
            .map(|(s, a)| Allocation { strategy: s.as_mut(), weight: a.weight })
            .collect();
        run_multi_backtest_with_actions(bars, &actions, &mut allocations, &request.config, ctl)?
    } else {
        let mut strategy = request.strategy.build(&request.params)?;
        run_backtest_with_actions(bars, &actions, strategy.as_mut(), &request.config, ctl)?
    };
    let benchmark = spectra_research::benchmark_returns(bars);
    Ok(BacktestOutput { request: request.clone(), result, benchmark, symbols: symbols.to_vec() })
}
