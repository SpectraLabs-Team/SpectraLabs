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

use spectra_core::{Bar, Json, Result};
use spectra_engine::{run_backtest_controlled, BacktestConfig, BacktestResult, RunControl, RunError};
use spectra_research::ParamSet;

use crate::config::{config_to_user_json, parse_config, stored_config_json, ConfigError, FieldError};
use crate::strategies::{builtin, BuiltinStrategy};

/// A validated request: a known strategy, complete parameters and a configuration.
#[derive(Debug, Clone)]
pub struct BacktestRequest {
    /// The strategy to run.
    pub strategy: &'static BuiltinStrategy,
    /// Every parameter, defaults filled in, in the strategy's order.
    pub params: ParamSet,
    /// Engine configuration.
    pub config: BacktestConfig,
}

const REQUEST_FIELDS: [&str; 3] = ["strategy", "params", "config"];

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
        Ok(Self { strategy: s, params: s.resolve_params(params)?, config })
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

        let strategy = match doc.find("strategy").map(Json::as_str) {
            Some(Ok(name)) => builtin(name)
                .map_err(|e| errors.push(FieldError { field: "strategy".into(), message: e.to_string() }))
                .ok(),
            Some(Err(_)) => {
                errors.push(FieldError { field: "strategy".into(), message: "must be a string".into() });
                None
            }
            None => {
                errors.push(FieldError { field: "strategy".into(), message: "is required".into() });
                None
            }
        };

        let params = strategy.map(|s| s.resolve_params_json(doc.find("params")));
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

        match (strategy, params, config) {
            (Some(strategy), Some(params), Some(config)) if errors.is_empty() => {
                Ok(Self { strategy, params, config })
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
}

/// Builds the strategy and runs it under `ctl`.
///
/// # Errors
/// [`RunError`] as from [`run_backtest_controlled`]; strategy construction
/// problems come back as [`RunError::Engine`].
pub fn run_request(
    bars: &[Bar],
    request: &BacktestRequest,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestOutput, RunError> {
    let mut strategy = request.strategy.build(&request.params)?;
    let result = run_backtest_controlled(bars, strategy.as_mut(), &request.config, ctl)?;
    Ok(BacktestOutput { request: request.clone(), result })
}
