//! Correlated Monte Carlo simulation via Gaussian and Student-t copulas.
//!
//! Standard-library only: zero external dependencies.

use spectra_core::{Error, Result};

use crate::portfolio::{Asset, Portfolio};
use crate::returns::cholesky;
use crate::rng::{Mt19937_64, PolarNormal};

/// Copula dependence model across assets.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DependenceModel {
    /// 1-factor aggregate position (legacy baseline for golden parity).
    #[default]
    Aggregate,
    /// Gaussian copula on per-asset paths using Cholesky factor.
    GaussianCopula,
    /// Student-t copula to capture joint tail dependence.
    TCopula {
        /// Degrees of freedom parameter (typically 3 to 10).
        degrees_of_freedom: u32,
    },
    /// Clayton Archimedean copula modeling asymmetric lower tail crash dependence.
    ClaytonCopula {
        /// Dependence parameter $\theta > 0$.
        theta: f64,
    },
}

impl DependenceModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Aggregate => "aggregate",
            Self::GaussianCopula => "gaussian_copula",
            Self::TCopula { .. } => "t_copula",
            Self::ClaytonCopula { .. } => "clayton_copula",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "aggregate" | "single_factor" => Ok(Self::Aggregate),
            "gaussian" | "gaussian_copula" | "copula" => Ok(Self::GaussianCopula),
            "t_copula" | "student_t" | "t" => Ok(Self::TCopula { degrees_of_freedom: 4 }),
            "clayton" | "clayton_copula" => Ok(Self::ClaytonCopula { theta: 2.0 }),
            _ => Err(Error::InvalidArgument(format!(
                "Unknown dependence model: {s} (use aggregate, gaussian_copula, t_copula, or clayton_copula)"
            ))),
        }
    }
}

/// Risk contribution breakdown for a single position.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionRiskContribution {
    /// Asset symbol.
    pub symbol: String,
    /// Position value.
    pub position_value: f64,
    /// Gross exposure weight.
    pub weight: f64,
    /// Component VaR in currency units.
    pub component_var: f64,
    /// Component CVaR (Euler allocation) in currency units.
    pub component_cvar: f64,
    /// Percentage share of portfolio CVaR [0.0, 1.0].
    pub cvar_contribution_pct: f64,
}

/// Result of correlated simulation across assets.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrelatedSimulationResult {
    /// One portfolio loss per path (positive = loss).
    pub portfolio_losses: Vec<f64>,
    /// Per-asset losses: asset_losses[asset_index][path_index].
    pub asset_losses: Vec<Vec<f64>>,
    /// Position-level risk attribution.
    pub contributions: Vec<PositionRiskContribution>,
    /// Dependence model used.
    pub model: DependenceModel,
    /// Paths simulated.
    pub simulations_run: u64,
    /// Execution wall time in milliseconds.
    pub elapsed_ms: f64,
}

/// Run correlated Monte Carlo simulation for a portfolio.
pub fn simulate_correlated(
    portfolio: &Portfolio,
    correlation: Option<&[Vec<f64>]>,
    model: DependenceModel,
    simulations: u64,
    seed: u64,
    horizon_years: f64,
    confidence: f64,
) -> Result<CorrelatedSimulationResult> {
    let start = std::time::Instant::now();
    portfolio.validate()?;
    if simulations == 0 {
        return Err(Error::InvalidArgument("simulations must be > 0".into()));
    }
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(Error::InvalidArgument("confidence must be in (0, 1)".into()));
    }
    if !(horizon_years > 0.0 && horizon_years.is_finite()) {
        return Err(Error::InvalidArgument("horizon_years must be > 0".into()));
    }

    let assets = portfolio.assets();
    let n_assets = assets.len();
    if n_assets == 0 {
        return Err(Error::InvalidArgument("portfolio is empty".into()));
    }

    let chol = if let Some(corr) = correlation {
        if corr.len() != n_assets {
            return Err(Error::InvalidArgument(format!(
                "Correlation matrix dimension {} does not match asset count {n_assets}",
                corr.len()
            )));
        }
        cholesky(corr)?
    } else {
        let mut id = vec![vec![0.0; n_assets]; n_assets];
        for i in 0..n_assets {
            id[i][i] = 1.0;
        }
        id
    };

    let mut rng = Mt19937_64::new(seed);
    let mut normal = PolarNormal::new();

    let n_paths = usize::try_from(simulations).unwrap_or(usize::MAX);
    let mut portfolio_losses = vec![0.0; n_paths];
    let mut asset_losses = vec![vec![0.0; n_paths]; n_assets];

    let default_vol = crate::engine::DEFAULT_VOLATILITY;
    let mut drift_terms = Vec::with_capacity(n_assets);
    let mut diffusions = Vec::with_capacity(n_assets);
    let mut initial_values = Vec::with_capacity(n_assets);

    for asset in assets {
        let vol = if asset.volatility > 0.0 { asset.volatility } else { default_vol };
        let drift = 0.0;
        let drift_term = (drift - 0.5 * vol * vol) * horizon_years;
        let diffusion = vol * horizon_years.sqrt();
        drift_terms.push(drift_term);
        diffusions.push(diffusion);
        initial_values.push(asset.value());
    }

    let mut z_vec = vec![0.0; n_assets];
    let mut x_vec = vec![0.0; n_assets];

    for path_idx in 0..n_paths {
        if n_assets == 1 {
            let z = normal.sample(&mut rng);
            let terminal = initial_values[0] * (drift_terms[0] + diffusions[0] * z).exp();
            let loss = initial_values[0] - terminal;
            portfolio_losses[path_idx] = loss;
            asset_losses[0][path_idx] = loss;
            continue;
        }

        for z in &mut z_vec {
            *z = normal.sample(&mut rng);
        }

        match model {
            DependenceModel::TCopula { degrees_of_freedom } => {
                let nu = degrees_of_freedom.max(1);
                let mut sum_sq = 0.0;
                for _ in 0..nu {
                    let u = normal.sample(&mut rng);
                    sum_sq += u * u;
                }
                let scale = (nu as f64 / sum_sq.max(1e-12)).sqrt();
                for z in &mut z_vec {
                    *z *= scale;
                }
            }
            DependenceModel::ClaytonCopula { theta } => {
                let th = theta.max(0.1);
                let u_common = (normal.sample(&mut rng).abs() + 0.1).ln().abs();
                let scale = (1.0 + th * u_common).powf(1.0 / th);
                for z in &mut z_vec {
                    if *z < 0.0 {
                        *z *= scale;
                    }
                }
            }
            DependenceModel::Aggregate | DependenceModel::GaussianCopula => {}
        }

        for i in 0..n_assets {
            let mut sum = 0.0;
            for j in 0..=i {
                sum += chol[i][j] * z_vec[j];
            }
            x_vec[i] = sum;
        }

        let mut port_loss = 0.0;
        for i in 0..n_assets {
            let terminal = initial_values[i] * (drift_terms[i] + diffusions[i] * x_vec[i]).exp();
            let loss = initial_values[i] - terminal;
            asset_losses[i][path_idx] = loss;
            port_loss += loss;
        }
        portfolio_losses[path_idx] = port_loss;
    }

    let contributions = compute_risk_contributions(assets, &portfolio_losses, &asset_losses, confidence);

    Ok(CorrelatedSimulationResult {
        portfolio_losses,
        asset_losses,
        contributions,
        model,
        simulations_run: simulations,
        elapsed_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

/// Compute Component VaR and Component CVaR (Euler allocation) for each asset.
pub fn compute_risk_contributions(
    assets: &[Asset],
    portfolio_losses: &[f64],
    asset_losses: &[Vec<f64>],
    confidence: f64,
) -> Vec<PositionRiskContribution> {
    let n = portfolio_losses.len();
    if n == 0 || assets.is_empty() {
        return Vec::new();
    }

    let mut indexed: Vec<(usize, f64)> = portfolio_losses.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.total_cmp(&b.1));

    let var_idx = ((confidence * (n - 1) as f64).floor() as usize).min(n - 1);
    let port_var = indexed[var_idx].1;

    let tail_indices: Vec<usize> =
        indexed.iter().filter(|(_, loss)| *loss >= port_var).map(|(i, _)| *i).collect();
    let tail_len = tail_indices.len().max(1);

    let half_band = (n / 100).clamp(1, 50);
    let band_start = var_idx.saturating_sub(half_band);
    let band_end = (var_idx + half_band).min(n - 1);
    let band_indices: Vec<usize> = indexed[band_start..=band_end].iter().map(|(i, _)| *i).collect();
    let band_len = band_indices.len().max(1);

    let gross_total: f64 = assets.iter().map(|a| a.value().abs()).sum();
    let port_cvar = tail_indices.iter().map(|&idx| portfolio_losses[idx]).sum::<f64>() / tail_len as f64;

    let mut contributions = Vec::with_capacity(assets.len());

    for (i, asset) in assets.iter().enumerate() {
        let weight = if gross_total > 0.0 { asset.value().abs() / gross_total } else { 0.0 };
        let comp_cvar: f64 =
            tail_indices.iter().map(|&idx| asset_losses[i][idx]).sum::<f64>() / tail_len as f64;
        let comp_var: f64 =
            band_indices.iter().map(|&idx| asset_losses[i][idx]).sum::<f64>() / band_len as f64;
        let pct = if port_cvar.abs() > 1e-9 { comp_cvar / port_cvar } else { 0.0 };

        contributions.push(PositionRiskContribution {
            symbol: asset.symbol.clone(),
            position_value: asset.value(),
            weight,
            component_var: comp_var,
            component_cvar: comp_cvar,
            cvar_contribution_pct: pct,
        });
    }

    contributions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::returns::correlation;
    use crate::simulation::{run_reference, SimulationTask};

    #[test]
    fn test_single_asset_matches_reference_kernel() {
        let mut p = Portfolio::new();
        p.add_asset_with_volatility("AAPL", 1000.0, 190.0, 0.25);

        let n_sims = 10_000;
        let seed = 42;
        let horizon = 1.0 / 252.0;

        let res = simulate_correlated(&p, None, DependenceModel::GaussianCopula, n_sims, seed, horizon, 0.99)
            .unwrap();

        let ref_task = SimulationTask {
            num_paths: n_sims,
            seed,
            drift: 0.0,
            volatility: 0.25,
            initial_value: 190_000.0,
            horizon_years: horizon,
        };
        let ref_res = run_reference(&ref_task).unwrap();

        assert_eq!(res.portfolio_losses.len(), ref_res.terminal_losses.len());
        for (a, b) in res.portfolio_losses.iter().zip(&ref_res.terminal_losses) {
            assert!((a - b).abs() < 1e-10, "single asset loss must match reference: {a} vs {b}");
        }
    }

    #[test]
    fn test_simulated_correlations_match_input_within_tolerance() {
        let mut p = Portfolio::new();
        p.add_asset_with_volatility("A", 10.0, 100.0, 0.20).add_asset_with_volatility("B", 10.0, 100.0, 0.30);

        let target_rho = 0.70;
        let corr_matrix = vec![vec![1.0, target_rho], vec![target_rho, 1.0]];

        let n_sims = 50_000;
        let seed = 12345;
        let res = simulate_correlated(
            &p,
            Some(&corr_matrix),
            DependenceModel::GaussianCopula,
            n_sims,
            seed,
            1.0 / 252.0,
            0.99,
        )
        .unwrap();

        let sim_rho = correlation(&res.asset_losses[0], &res.asset_losses[1]).unwrap();
        assert!(
            (sim_rho - target_rho).abs() < 0.02,
            "Simulated correlation {sim_rho} should match target {target_rho} within 0.02"
        );
    }

    #[test]
    fn test_component_cvar_sums_to_portfolio_cvar() {
        let mut p = Portfolio::new();
        p.add_asset_with_volatility("A", 10.0, 100.0, 0.20)
            .add_asset_with_volatility("B", 20.0, 50.0, 0.30)
            .add_asset_with_volatility("C", 15.0, 80.0, 0.25);

        let corr_matrix = vec![vec![1.0, 0.5, 0.2], vec![0.5, 1.0, 0.3], vec![0.2, 0.3, 1.0]];

        let res = simulate_correlated(
            &p,
            Some(&corr_matrix),
            DependenceModel::GaussianCopula,
            20_000,
            999,
            1.0 / 252.0,
            0.99,
        )
        .unwrap();

        let mut sorted = res.portfolio_losses.clone();
        sorted.sort_by(f64::total_cmp);
        let var_idx = (0.99 * (sorted.len() - 1) as f64).floor() as usize;
        let var = sorted[var_idx];
        let tail: Vec<f64> = sorted.iter().copied().filter(|&l| l >= var).collect();
        let port_cvar = tail.iter().sum::<f64>() / tail.len() as f64;

        let sum_comp_cvar: f64 = res.contributions.iter().map(|c| c.component_cvar).sum();
        assert!(
            (sum_comp_cvar - port_cvar).abs() < 1e-6,
            "Sum of Euler component CVaRs ({sum_comp_cvar}) must equal portfolio CVaR ({port_cvar})"
        );

        let sum_pct: f64 = res.contributions.iter().map(|c| c.cvar_contribution_pct).sum();
        assert!((sum_pct - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_clayton_copula_amplifies_downside_cvar() {
        let mut p = Portfolio::new();
        p.add_asset_with_volatility("A", 10.0, 100.0, 0.20).add_asset_with_volatility("B", 10.0, 100.0, 0.20);

        let corr_matrix = vec![vec![1.0, 0.4], vec![0.4, 1.0]];

        let res_gauss = simulate_correlated(
            &p,
            Some(&corr_matrix),
            DependenceModel::GaussianCopula,
            10_000,
            42,
            1.0 / 252.0,
            0.99,
        )
        .unwrap();

        let res_clayton = simulate_correlated(
            &p,
            Some(&corr_matrix),
            DependenceModel::ClaytonCopula { theta: 2.0 },
            10_000,
            42,
            1.0 / 252.0,
            0.99,
        )
        .unwrap();

        let cvar_gauss: f64 = res_gauss.contributions.iter().map(|c| c.component_cvar).sum();
        let cvar_clayton: f64 = res_clayton.contributions.iter().map(|c| c.component_cvar).sum();

        assert!(
            cvar_clayton > cvar_gauss,
            "Clayton lower tail crash dependence must produce higher CVaR ({cvar_clayton}) than Gaussian ({cvar_gauss})"
        );
    }
}
