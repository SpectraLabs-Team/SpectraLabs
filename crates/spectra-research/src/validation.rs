//! A transparent validation report for a selected trial. Every number is
//! shown with its inputs and the assumptions behind it; nothing is collapsed
//! into a single opaque score.

use spectra_core::cfmt::{fmt_e, fmt_f};

use crate::pbo::PboResult;
use crate::stats::{expected_max_sharpe, min_track_record_length, moments, probabilistic_sharpe};
use crate::sweep::{Trial, TrialLedger};

/// What can honestly be claimed about a selected trial.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// Every trial recorded in the ledger.
    pub n_trials: usize,
    /// Return observations of the selected trial.
    pub n_obs: usize,
    /// Per-period Sharpe.
    pub sharpe_per_period: f64,
    /// Annualized Sharpe.
    pub sharpe_annualized: f64,
    /// Skewness.
    pub skew: f64,
    /// Kurtosis (not excess).
    pub kurt: f64,
    /// Variance of per-period Sharpe across trials.
    pub sr_variance_across_trials: f64,
    /// Per-period benchmark the selection must beat.
    pub expected_max_sharpe: f64,
    /// P(true Sharpe > 0), ignoring multiple testing.
    pub psr_vs_zero: f64,
    /// P(true Sharpe > expected max of `n_trials` random trials).
    pub dsr: f64,
    /// Observations needed for PSR(vs the benchmark) at `confidence`.
    pub min_track_record_obs: f64,
    /// Confidence level.
    pub confidence: f64,
    /// Whether a PBO estimate was supplied.
    pub pbo_available: bool,
    /// PBO, if supplied.
    pub pbo: f64,
    /// Things that weaken or qualify the result.
    pub warnings: Vec<String>,
    /// Always listed.
    pub assumptions: Vec<String>,
}

/// Validates the selection of `selected` out of everything in `ledger`.
pub fn validate_selection(
    selected: &Trial,
    ledger: &TrialLedger,
    periods_per_year: f64,
    pbo: Option<&PboResult>,
    confidence: f64,
) -> ValidationReport {
    let m = moments(&selected.returns);
    let sharpe_pp = if m.stdev > 0.0 { m.mean / m.stdev } else { 0.0 };
    let sr_var = ledger.sharpe_variance();
    let n_trials = ledger.count();
    let ems = expected_max_sharpe(sr_var, n_trials);
    let mut r = ValidationReport {
        n_trials,
        n_obs: selected.returns.len(),
        sharpe_per_period: sharpe_pp,
        sharpe_annualized: sharpe_pp * periods_per_year.sqrt(),
        skew: m.skew,
        kurt: m.kurt,
        sr_variance_across_trials: sr_var,
        expected_max_sharpe: ems,
        psr_vs_zero: probabilistic_sharpe(sharpe_pp, 0.0, m.n, m.skew, m.kurt),
        dsr: probabilistic_sharpe(sharpe_pp, ems, m.n, m.skew, m.kurt),
        min_track_record_obs: min_track_record_length(sharpe_pp, ems, m.skew, m.kurt, confidence),
        confidence,
        pbo_available: pbo.is_some(),
        pbo: pbo.map_or(0.0, |p| p.pbo),
        warnings: Vec::new(),
        assumptions: vec![
            "Returns are treated as serially independent (no autocorrelation adjustment).".into(),
            "PSR/DSR use only the first four moments of the returns.".into(),
            "The multiple-testing benchmark treats trials as independent; grid neighbours are correlated, so the \
             effective number of trials is smaller than the count."
                .into(),
            "Sharpe ratios in PSR/DSR are per-period, not annualized.".into(),
        ],
    };

    if r.n_obs < 100 {
        r.warnings.push(format!(
            "Short sample ({} observations): Sharpe, skew and kurtosis estimates are unreliable.",
            r.n_obs
        ));
    }
    if r.n_trials <= 1 {
        r.warnings.push(
            "Only one trial recorded: no multiple-testing adjustment was applied, so DSR equals PSR. If other \
             variants were tried, add them to the ledger."
                .into(),
        );
    }
    if r.dsr.is_nan() || r.psr_vs_zero.is_nan() {
        r.warnings
            .push("PSR/DSR undefined for this return distribution (variance term not positive).".into());
    } else if r.dsr < confidence {
        r.warnings.push(format!(
            "DSR {} is below the {} confidence level: the result cannot be told apart, at that confidence, from \
             the best of {} random trials.",
            fmt_f(r.dsr, 2),
            fmt_f(confidence, 2),
            r.n_trials
        ));
    }
    if m.kurt > 10.0 {
        r.warnings.push(format!(
            "Heavy tails (kurtosis {}): PSR/DSR confidence intervals are likely too narrow.",
            fmt_f(m.kurt, 6)
        ));
    }
    if r.min_track_record_obs.is_infinite() {
        r.warnings.push(
            "Observed Sharpe does not exceed the benchmark: no track record length is long enough.".into(),
        );
    } else if !r.min_track_record_obs.is_nan() && r.min_track_record_obs > r.n_obs as f64 {
        r.warnings
            .push("Track record is shorter than the minimum needed for the requested confidence.".into());
    }
    if r.pbo_available && r.pbo >= 0.5 {
        r.warnings
            .push("PBO is 0.5 or higher: in-sample selection did not predict out-of-sample rank.".into());
    }
    if r.pbo_available {
        r.warnings.push(
            "PBO from a single dataset is a noisy estimate (standard deviation about 0.2 on pure noise); treat it \
             as an indicator, not a precise probability."
                .into(),
        );
    }
    r
}

/// Plain-text rendering for logs and reports.
pub fn format_report(r: &ValidationReport) -> String {
    let mut out = String::from("Validation report\n-----------------\n");
    out += &format!("trials recorded            {}\n", r.n_trials);
    out += &format!("observations               {}\n", r.n_obs);
    out += &format!("Sharpe (annualized)        {}\n", fmt_f(r.sharpe_annualized, 3));
    out += &format!("Sharpe (per period)        {}\n", fmt_f(r.sharpe_per_period, 5));
    out += &format!("skew / kurtosis            {} / {}\n", fmt_f(r.skew, 3), fmt_f(r.kurt, 3));
    out += &format!("Sharpe variance, trials    {}\n", fmt_e(r.sr_variance_across_trials, 3));
    out += &format!("expected max Sharpe (N)    {} per period\n", fmt_f(r.expected_max_sharpe, 5));
    out += &format!("PSR (vs 0)                 {}\n", fmt_f(r.psr_vs_zero, 3));
    out += &format!("DSR                        {}\n", fmt_f(r.dsr, 3));
    out += &format!("min track record (obs)     {}\n", fmt_f(r.min_track_record_obs, 0));
    if r.pbo_available {
        out += &format!("PBO                        {}\n", fmt_f(r.pbo, 3));
    }
    if !r.warnings.is_empty() {
        out += "\nWarnings\n";
        for w in &r.warnings {
            out += &format!("  - {w}\n");
        }
    }
    out += "\nAssumptions\n";
    for a in &r.assumptions {
        out += &format!("  - {a}\n");
    }
    out
}
