//! Holdout vault: sealed evaluation data with a metered, audited access budget.
//!
//! A holdout is only a fair test while it is untouched. Every time you look at
//! it and then change your mind, it becomes a little less out-of-sample. The
//! vault makes that cost visible and bounded:
//! * [`Vault::seal`] registers a dataset (already in the `DataStore`) as a
//!   holdout with a fixed evaluation budget. The seal records only the
//!   dataset's id, bar count and fingerprint, never its contents.
//! * [`Vault::evaluate`] is the ONLY supported way to use it. It consumes one
//!   unit of budget BEFORE touching the data, so a crash, an error or a "just
//!   checking" run still costs a unit. It returns aggregate metrics and
//!   deflated-Sharpe statistics, never the equity curve, trades or bars.
//! * [`Vault::checked_get`] refuses to hand out a sealed dataset's bars, and
//!   logs the attempt.
//!
//! Every step is an event in the registry's hash-chained log, so the access
//! history is auditable and tamper-evident. The budget check and consumption
//! are one atomic step, so concurrent processes cannot overshoot it.
//!
//! This is process discipline plus an audit trail, NOT access control: anyone
//! who can read the `DataStore` directory can read the bars, and strategy code
//! running during `evaluate` sees the bars like any backtest.

use spectra_core::{Error, Json, Result};
use spectra_data::{DataStore, Dataset};
use spectra_engine::{config_fingerprint, returns_from_curve, run_backtest, BacktestConfig, Metrics};
use spectra_research::{expected_max_sharpe, moments, probabilistic_sharpe, ParamSet, StrategyFactory};

use crate::registry::{metrics_to_json, params_to_json, Event, Registry};

fn vault_err(msg: impl Into<String>) -> Error {
    Error::Vault(msg.into())
}

/// Access counts for one holdout.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HoldoutStatus {
    /// Holdout name.
    pub name: String,
    /// Sealed dataset.
    pub dataset_id: String,
    /// Free text.
    pub purpose: String,
    /// Evaluation budget.
    pub budget: usize,
    /// Evaluations that consumed budget.
    pub started: usize,
    /// Completed evaluations.
    pub completed: usize,
    /// Failed evaluations.
    pub failed: usize,
    /// Attempts refused after the budget ran out.
    pub denied: usize,
}

impl HoldoutStatus {
    /// Evaluations left.
    pub fn remaining(&self) -> usize {
        self.budget.saturating_sub(self.started)
    }
}

/// Aggregates from one holdout evaluation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HoldoutResult {
    /// Holdout name.
    pub name: String,
    /// 1-based.
    pub evaluation_no: usize,
    /// Budget.
    pub budget: usize,
    /// Evaluations left.
    pub remaining: usize,
    /// Metrics.
    pub metrics: Metrics,
    /// Per-period Sharpe.
    pub sharpe_per_period: f64,
    /// Return observations.
    pub n_obs: usize,
    /// PSR vs zero.
    pub psr_vs_zero: f64,
    /// DSR, deflated by the number of evaluations made against this holdout.
    pub dsr: f64,
    /// Benchmark used by DSR.
    pub expected_max_sharpe: f64,
    /// Evaluations so far, including this one.
    pub evaluations_used: usize,
    /// Result fingerprint.
    pub result_sha256: String,
    /// Caveats.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Seal {
    name: String,
    dataset_id: String,
    bars_sha256: String,
    purpose: String,
    budget: usize,
}

fn find_seal_payload<'a>(log: &'a [Event], name: &str) -> Result<Option<&'a Json>> {
    for e in log {
        if e.kind == "holdout.sealed" && e.payload.at("name")?.as_str()? == name {
            return Ok(Some(&e.payload));
        }
    }
    Ok(None)
}

fn to_seal(p: &Json) -> Result<Seal> {
    Ok(Seal {
        name: p.at("name")?.as_str()?.into(),
        dataset_id: p.at("dataset_id")?.as_str()?.into(),
        bars_sha256: p.at("bars_sha256")?.as_str()?.into(),
        purpose: p.at("purpose")?.as_str()?.into(),
        budget: usize::try_from(p.at("budget")?.as_int()?).unwrap_or(0),
    })
}

fn status_from(log: &[Event], seal: &Seal) -> HoldoutStatus {
    let mut st = HoldoutStatus {
        name: seal.name.clone(),
        dataset_id: seal.dataset_id.clone(),
        purpose: seal.purpose.clone(),
        budget: seal.budget,
        ..HoldoutStatus::default()
    };
    for e in log {
        if !e.kind.starts_with("holdout.") || e.kind == "holdout.sealed" {
            continue;
        }
        match e.payload.find("name") {
            Some(Json::String(n)) if *n == seal.name => {}
            _ => continue,
        }
        match e.kind.as_str() {
            "holdout.evaluation_started" => st.started += 1,
            "holdout.evaluation_completed" => st.completed += 1,
            "holdout.evaluation_failed" => st.failed += 1,
            "holdout.denied" => st.denied += 1,
            _ => {}
        }
    }
    st
}

/// The vault. See the module documentation.
#[derive(Debug)]
pub struct Vault<'a> {
    registry: &'a mut Registry,
    store: &'a DataStore,
}

impl<'a> Vault<'a> {
    /// A vault over a registry and a data store.
    pub fn new(registry: &'a mut Registry, store: &'a DataStore) -> Self {
        Self { registry, store }
    }

    /// Seals a dataset as a holdout.
    ///
    /// # Errors
    /// [`Error::Vault`] if the dataset is unknown or fails verification, the
    /// name or dataset is already sealed, or `budget < 1`.
    pub fn seal(&mut self, name: &str, dataset_id: &str, budget: usize, purpose: &str) -> Result<()> {
        if name.is_empty() {
            return Err(vault_err("holdout name must not be empty"));
        }
        if budget < 1 {
            return Err(vault_err("holdout budget must be at least 1"));
        }
        if !self.store.contains(dataset_id) {
            return Err(vault_err(format!("dataset {dataset_id} is not in the store")));
        }
        let data =
            self.store.get(dataset_id).map_err(|e| vault_err(format!("dataset failed verification: {e}")))?;
        let mut reason = String::new();
        let appended = self.registry.append_event_if("holdout.sealed", &mut |log, _| {
            for e in log.iter().filter(|e| e.kind == "holdout.sealed") {
                let existing = e.payload.at("name")?.as_str()?;
                if existing == name {
                    reason = format!("a holdout named '{name}' already exists");
                    return Ok(None);
                }
                if e.payload.at("dataset_id")?.as_str()? == dataset_id {
                    reason = format!("dataset {dataset_id} is already sealed as '{existing}'");
                    return Ok(None);
                }
            }
            Ok(Some(
                Json::object()
                    .with("name", Json::string(name))
                    .with("dataset_id", Json::string(dataset_id))
                    .with("bars_sha256", Json::string(data.info.bars_sha256.clone()))
                    .with("bar_count", Json::integer(data.info.bar_count as i64))
                    .with("budget", Json::integer(budget as i64))
                    .with("purpose", Json::string(purpose)),
            ))
        })?;
        if appended.is_none() {
            return Err(vault_err(reason));
        }
        Ok(())
    }

    /// Whether a dataset is sealed.
    ///
    /// # Errors
    /// If the registry log is broken.
    pub fn is_sealed_dataset(&mut self, dataset_id: &str) -> Result<bool> {
        for e in self.registry.events()? {
            if e.kind == "holdout.sealed" && e.payload.at("dataset_id")?.as_str()? == dataset_id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Status of one holdout.
    ///
    /// # Errors
    /// [`Error::Vault`] if unknown.
    pub fn status(&mut self, name: &str) -> Result<HoldoutStatus> {
        let log = self.registry.events()?;
        let p = find_seal_payload(log, name)?.ok_or_else(|| vault_err(format!("unknown holdout: {name}")))?;
        Ok(status_from(log, &to_seal(p)?))
    }

    /// Every holdout.
    ///
    /// # Errors
    /// If the registry log is broken.
    pub fn list(&mut self) -> Result<Vec<HoldoutStatus>> {
        let log = self.registry.events()?;
        log.iter()
            .filter(|e| e.kind == "holdout.sealed")
            .map(|e| Ok(status_from(log, &to_seal(&e.payload)?)))
            .collect()
    }

    /// `DataStore::get`, except sealed datasets are refused (and the attempt is logged).
    ///
    /// # Errors
    /// [`Error::Vault`] for a sealed dataset, otherwise as `DataStore::get`.
    pub fn checked_get(&mut self, dataset_id: &str) -> Result<Dataset> {
        if self.is_sealed_dataset(dataset_id)? {
            self.registry.append_event(
                "holdout.access_denied",
                Json::object().with("dataset_id", Json::string(dataset_id)),
            )?;
            return Err(vault_err(format!("dataset {dataset_id} is a sealed holdout; use Vault::evaluate")));
        }
        self.store.get(dataset_id)
    }

    /// Consumes one unit of budget, runs the strategy on the sealed data and
    /// returns aggregates only.
    ///
    /// # Errors
    /// [`Error::Vault`] when the holdout is unknown, the budget is exhausted
    /// (the refusal is logged) or the evaluation fails (the budget stays
    /// consumed and the failure is logged).
    #[allow(clippy::too_many_lines)]
    pub fn evaluate(
        &mut self,
        name: &str,
        factory: &StrategyFactory<'_>,
        strategy: &str,
        strategy_sha256: &str,
        params: &ParamSet,
        config: &BacktestConfig,
    ) -> Result<HoldoutResult> {
        // 1. Atomically check the budget and consume a unit. The data is not touched yet.
        let mut seal = Seal::default();
        let mut evaluation_no = 0usize;
        let mut refusal = String::new();
        let started = self.registry.append_event_if("holdout.evaluation_started", &mut |log, _| {
            let Some(sp) = find_seal_payload(log, name)? else {
                refusal = format!("unknown holdout: {name}");
                return Ok(None);
            };
            seal = to_seal(sp)?;
            let st = status_from(log, &seal);
            if st.started >= seal.budget {
                refusal = format!("holdout '{name}' has no evaluations left (budget {})", seal.budget);
                return Ok(None);
            }
            evaluation_no = st.started + 1;
            Ok(Some(
                Json::object()
                    .with("name", Json::string(name))
                    .with("evaluation_no", Json::integer(evaluation_no as i64))
                    .with("strategy", Json::string(strategy))
                    .with("strategy_sha256", Json::string(strategy_sha256))
                    .with("params", params_to_json(params)?)
                    .with("config_sha256", Json::string(config_fingerprint(config))),
            ))
        })?;
        if started.is_none() {
            if !refusal.starts_with("unknown holdout") {
                self.registry.append_event(
                    "holdout.denied",
                    Json::object()
                        .with("name", Json::string(name))
                        .with("reason", Json::string("budget exhausted"))
                        .with("strategy", Json::string(strategy))
                        .with("params", params_to_json(params)?),
                )?;
            }
            return Err(vault_err(refusal));
        }

        // 2. Load and verify the sealed data, then run. Any failure still costs the unit.
        let attempt = || -> std::result::Result<_, String> {
            let data = self.store.get(&seal.dataset_id).map_err(|e| e.to_string())?;
            if data.info.bars_sha256 != seal.bars_sha256 {
                return Err("stored data no longer matches the sealed fingerprint".into());
            }
            let mut s =
                factory(params).map_err(|e| format!("the strategy factory returned no strategy: {e}"))?;
            run_backtest(&data.bars, s.as_mut(), config).map_err(|e| e.to_string())
        };
        let result = match attempt() {
            Ok(r) => r,
            Err(why) => {
                self.registry.append_event(
                    "holdout.evaluation_failed",
                    Json::object()
                        .with("name", Json::string(name))
                        .with("evaluation_no", Json::integer(evaluation_no as i64))
                        .with("error", Json::string(why.clone())),
                )?;
                return Err(vault_err(format!(
                    "holdout evaluation {evaluation_no} failed (budget stays consumed): {why}"
                )));
            }
        };

        // 3. Statistics from this evaluation and the earlier ones against the same holdout.
        let returns = returns_from_curve(&result.equity_curve);
        let m = moments(&returns);
        let sharpe_pp = if m.stdev > 0.0 { m.mean / m.stdev } else { 0.0 };
        let mut sharpes = Vec::new();
        for e in self.registry.events()? {
            if e.kind == "holdout.evaluation_completed" && e.payload.at("name")?.as_str()? == name {
                sharpes.push(e.payload.at("sharpe_per_period")?.as_double()?);
            }
        }
        sharpes.push(sharpe_pp);
        let mut sr_variance = 0.0;
        if sharpes.len() >= 2 {
            let mean = sharpes.iter().sum::<f64>() / sharpes.len() as f64;
            let ss: f64 = sharpes.iter().map(|v| (v - mean) * (v - mean)).sum();
            sr_variance = ss / (sharpes.len() - 1) as f64;
        }
        let ems = expected_max_sharpe(sr_variance, evaluation_no);
        let mut out = HoldoutResult {
            name: name.into(),
            evaluation_no,
            budget: seal.budget,
            remaining: seal.budget - evaluation_no,
            metrics: result.metrics,
            sharpe_per_period: sharpe_pp,
            n_obs: returns.len(),
            psr_vs_zero: probabilistic_sharpe(sharpe_pp, 0.0, m.n, m.skew, m.kurt),
            dsr: probabilistic_sharpe(sharpe_pp, ems, m.n, m.skew, m.kurt),
            expected_max_sharpe: ems,
            evaluations_used: evaluation_no,
            result_sha256: result.run.result_sha256.clone(),
            warnings: Vec::new(),
        };
        out.warnings.push(format!(
            "Evaluation {evaluation_no} of {}. Once you act on a holdout result the holdout is no longer fully \
             out-of-sample.",
            seal.budget
        ));
        if evaluation_no == 1 {
            out.warnings.push("Only one evaluation so far: nothing to deflate by, so DSR equals PSR.".into());
        }
        if out.n_obs < 100 {
            out.warnings
                .push(format!("Short holdout ({} observations): estimates are unreliable.", out.n_obs));
        }
        if out.remaining == 0 {
            out.warnings.push("This was the last evaluation; the holdout is now exhausted.".into());
        }
        self.registry.append_event(
            "holdout.evaluation_completed",
            Json::object()
                .with("name", Json::string(name))
                .with("evaluation_no", Json::integer(evaluation_no as i64))
                .with("result_sha256", Json::string(out.result_sha256.clone()))
                .with("metrics", metrics_to_json(&out.metrics)?)
                .with("sharpe_per_period", Json::number(out.sharpe_per_period)?)
                .with("n_obs", Json::integer(out.n_obs as i64)),
        )?;
        Ok(out)
    }
}
