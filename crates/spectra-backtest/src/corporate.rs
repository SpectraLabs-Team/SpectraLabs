//! Corporate actions in a backtest request (M3 task 10).
//!
//! ```json
//! "corporate_actions": [
//!   { "symbol": "AAPL", "ex_date": "2020-08-31", "type": "split", "from": 1, "to": 4 },
//!   { "symbol": "AAPL", "ex_date": "2020-11-06", "type": "dividend", "amount": 0.205 }
//! ]
//! ```
//!
//! An action names its instrument by `symbol` (resolved against the dataset
//! when the job runs) or by `instrument` id. `ex_date` is an ISO date or
//! date-time (UTC unless it carries an offset) or nanoseconds since the epoch.
//! The engine applies them as they go ex; see [`spectra_engine::corporate`].

use spectra_core::{Json, Price, Result};
use spectra_engine::{CorporateAction, CorporateActionKind, MAX_SPLIT_SHARES};

use crate::config::FieldError;

/// Most actions in one request.
pub const MAX_ACTIONS: usize = 10_000;

/// Which instrument an action is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionTarget {
    /// A symbol of the dataset.
    Symbol(String),
    /// An instrument id.
    Instrument(u32),
}

/// One corporate action as requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestAction {
    /// Its instrument.
    pub target: ActionTarget,
    /// Ex-date, nanoseconds (UTC).
    pub ex_ts_ns: i64,
    /// The `ex_date` as given, for the stored request.
    pub ex_date: String,
    /// Split or dividend.
    pub kind: CorporateActionKind,
}

const FIELDS: [&str; 7] = ["symbol", "instrument", "ex_date", "type", "from", "to", "amount"];

fn err(errors: &mut Vec<FieldError>, field: String, message: impl Into<String>) {
    errors.push(FieldError { field, message: message.into() });
}

fn parse_ex_date(v: &Json) -> Option<(i64, String)> {
    match v {
        Json::Int(i) => Some((*i, i.to_string())),
        Json::String(s) => spectra_data::parse_timestamp(s, &spectra_data::TimeOptions::default())
            .map(|t| (t.ts_ns, s.trim().to_string())),
        _ => None,
    }
}

fn read_shares(item: &Json, name: &str, prefix: &str, errors: &mut Vec<FieldError>) -> Option<u32> {
    match item.find(name).map(Json::as_int) {
        Some(Ok(n)) if (1..=i64::from(MAX_SPLIT_SHARES)).contains(&n) => u32::try_from(n).ok(),
        Some(_) => {
            err(
                errors,
                format!("{prefix}{name}"),
                format!("must be a whole number from 1 to {MAX_SPLIT_SHARES}"),
            );
            None
        }
        None => {
            err(errors, format!("{prefix}{name}"), "is required for a split");
            None
        }
    }
}

/// Parses the `corporate_actions` list, reporting every problem under
/// `corporate_actions[i].<field>`.
pub fn parse_actions(list: &Json, errors: &mut Vec<FieldError>) -> Option<Vec<RequestAction>> {
    let Ok(items) = list.items() else {
        err(errors, "corporate_actions".into(), "must be a list");
        return None;
    };
    if items.len() > MAX_ACTIONS {
        err(errors, "corporate_actions".into(), format!("at most {MAX_ACTIONS} actions"));
        return None;
    }
    let before = errors.len();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let prefix = format!("corporate_actions[{i}].");
        let Ok(entries) = item.entries() else {
            err(errors, format!("corporate_actions[{i}]"), "must be a JSON object");
            continue;
        };
        for (k, _) in entries {
            if !FIELDS.contains(&k.as_str()) {
                err(errors, format!("{prefix}{k}"), "unknown field");
            }
        }
        let target = match (item.find("symbol"), item.find("instrument")) {
            (Some(_), Some(_)) => {
                err(errors, format!("{prefix}symbol"), "give either symbol or instrument");
                None
            }
            (Some(Json::String(s)), None) if !s.trim().is_empty() => {
                Some(ActionTarget::Symbol(s.trim().to_string()))
            }
            (Some(_), None) => {
                err(errors, format!("{prefix}symbol"), "must be a non-empty string");
                None
            }
            (None, Some(v)) => {
                let id = v.as_int().ok().and_then(|n| u32::try_from(n).ok());
                if id.is_none() {
                    err(errors, format!("{prefix}instrument"), "must be a whole number");
                }
                id.map(ActionTarget::Instrument)
            }
            (None, None) => {
                err(errors, format!("{prefix}symbol"), "is required (or give instrument)");
                None
            }
        };
        let ex = item.find("ex_date").and_then(parse_ex_date);
        if ex.is_none() {
            let message = if item.find("ex_date").is_some() {
                "must be an ISO date (2020-08-31), a date-time, or nanoseconds"
            } else {
                "is required"
            };
            err(errors, format!("{prefix}ex_date"), message);
        }
        let kind = match item.find("type").map(Json::as_str) {
            Some(Ok("split")) => {
                let (from, to) =
                    (read_shares(item, "from", &prefix, errors), read_shares(item, "to", &prefix, errors));
                if item.find("amount").is_some() {
                    err(errors, format!("{prefix}amount"), "only a dividend has an amount");
                }
                match (from, to) {
                    (Some(f), Some(t)) if f == t => {
                        err(errors, format!("{prefix}to"), "a split with from = to changes nothing");
                        None
                    }
                    (Some(from), Some(to)) => Some(CorporateActionKind::Split { from, to }),
                    _ => None,
                }
            }
            Some(Ok("dividend")) => {
                for f in ["from", "to"] {
                    if item.find(f).is_some() {
                        err(errors, format!("{prefix}{f}"), "only a split has from and to");
                    }
                }
                match item.find("amount").map(Json::as_double) {
                    Some(Ok(a)) if a.is_finite() && a > 0.0 => match Price::from_f64(a) {
                        Ok(p) if p.raw() > 0 => Some(CorporateActionKind::Dividend { per_share: p }),
                        _ => {
                            err(errors, format!("{prefix}amount"), "is out of range");
                            None
                        }
                    },
                    Some(_) => {
                        err(errors, format!("{prefix}amount"), "must be a number greater than 0");
                        None
                    }
                    None => {
                        err(errors, format!("{prefix}amount"), "is required for a dividend");
                        None
                    }
                }
            }
            Some(_) => {
                err(errors, format!("{prefix}type"), "must be \"split\" or \"dividend\"");
                None
            }
            None => {
                err(errors, format!("{prefix}type"), "is required");
                None
            }
        };
        if let (Some(target), Some((ex_ts_ns, ex_date)), Some(kind)) = (target, ex, kind) {
            out.push(RequestAction { target, ex_ts_ns, ex_date, kind });
        }
    }
    (errors.len() == before).then_some(out)
}

/// The actions with their instruments resolved against the dataset's
/// symbols (instrument id = position in `symbols`).
///
/// # Errors
/// A message naming the first action whose symbol is not in the dataset.
pub fn resolve(
    actions: &[RequestAction],
    symbols: &[String],
) -> std::result::Result<Vec<CorporateAction>, String> {
    actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let instrument = match &a.target {
                ActionTarget::Instrument(id) => *id,
                ActionTarget::Symbol(s) => symbols
                    .iter()
                    .position(|x| x == s)
                    .and_then(|k| u32::try_from(k).ok())
                    .ok_or_else(|| format!("corporate_actions[{i}].symbol: {s} is not in this dataset"))?,
            };
            Ok(CorporateAction { instrument, ex_ts_ns: a.ex_ts_ns, kind: a.kind })
        })
        .collect()
}

/// The actions as stored with the run.
///
/// # Errors
/// If a value is not finite (impossible for a parsed action).
pub fn actions_to_json(actions: &[RequestAction]) -> Result<Json> {
    let mut out = Json::array();
    for a in actions {
        let mut j = match &a.target {
            ActionTarget::Symbol(s) => Json::object().with("symbol", Json::string(s.clone())),
            ActionTarget::Instrument(id) => Json::object().with("instrument", Json::integer(i64::from(*id))),
        };
        j = j.with("ex_date", Json::string(a.ex_date.clone()));
        j = match a.kind {
            CorporateActionKind::Split { from, to } => j
                .with("type", Json::string("split"))
                .with("from", Json::integer(i64::from(from)))
                .with("to", Json::integer(i64::from(to))),
            CorporateActionKind::Dividend { per_share } => {
                j.with("type", Json::string("dividend")).with("amount", Json::number(per_share.to_f64())?)
            }
        };
        out.push(j)?;
    }
    Ok(out)
}
