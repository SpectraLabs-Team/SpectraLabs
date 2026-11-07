//! M7: copula parameters out of range are refused, not clamped.
//!
//! Student-t needs `degrees_of_freedom >= 1`; Clayton needs a finite `theta > 0`. The expected
//! numbers below were recorded on main (7911e15) before the clamps were removed, so valid inputs
//! are shown to give the same paths as before, bit for bit.

use std::collections::HashMap;

use spectra_core::Error;
use spectra_risk::copula::{simulate_correlated, CorrelatedSimulationResult, DependenceModel};
use spectra_risk::enforcement::{PortfolioSnapshot, PositionSnapshot};
use spectra_risk::Portfolio;

fn run(model: DependenceModel) -> spectra_core::Result<CorrelatedSimulationResult> {
    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.20);
    let identity = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
    simulate_correlated(&p, Some(&identity), model, 1_000, 42, 1.0 / 252.0, 0.99)
}

fn invalid_argument(res: spectra_core::Result<CorrelatedSimulationResult>) -> String {
    match res {
        Err(Error::InvalidArgument(msg)) => msg,
        other => panic!("expected Error::InvalidArgument, got {other:?}"),
    }
}

/// The losses' sum, first three and last values, printed in Rust's shortest round-trip form (so
/// equal text means equal bits), against the text recorded on main.
fn assert_same_as_main(model: DependenceModel, expected: &str) {
    let l = run(model).unwrap().portfolio_losses;
    let got = format!("{:?} {:?} {:?} {:?} {:?}", l.iter().sum::<f64>(), l[0], l[1], l[2], l[999]);
    assert_eq!(got, expected, "{model:?}: sum, first three paths, last path");
}

#[test]
fn dof_zero_is_refused_with_the_parameter_and_value() {
    let msg = invalid_argument(run(DependenceModel::TCopula { degrees_of_freedom: 0 }));
    assert_eq!(msg, "t_copula degrees_of_freedom must be at least 1, got 0");
}

#[test]
fn negative_dof_is_refused() {
    // degrees_of_freedom is a u32, so a negative value can only arrive as text.
    assert!(matches!(DependenceModel::parse("t_copula:-1"), Err(Error::InvalidArgument(_))));
    assert!(matches!(DependenceModel::parse("t_copula:-4"), Err(Error::InvalidArgument(_))));
}

#[test]
fn theta_not_positive_or_not_finite_is_refused() {
    for theta in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let msg = invalid_argument(run(DependenceModel::ClaytonCopula { theta }));
        assert_eq!(msg, format!("clayton theta must be finite and > 0, got {theta}"));
    }
}

#[test]
fn single_asset_portfolios_are_checked_too() {
    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ONLY", 10.0, 100.0, 0.20);
    let res = simulate_correlated(
        &p,
        None,
        DependenceModel::ClaytonCopula { theta: 0.0 },
        10,
        1,
        1.0 / 252.0,
        0.99,
    );
    assert!(matches!(res, Err(Error::InvalidArgument(_))), "{res:?}");
}

#[test]
fn valid_boundary_values_give_the_same_numbers_as_main() {
    // dof 1 is the smallest accepted; the old clamp never changed it.
    assert_same_as_main(
        DependenceModel::TCopula { degrees_of_freedom: 1 },
        "-39133008870003.55 -44.22991330452987 16.91165991608034 1169.6116067102157 6.400459777325068",
    );
    assert_same_as_main(
        DependenceModel::TCopula { degrees_of_freedom: 4 },
        "-730.5359272448409 -21.693249438506427 31.148462802930567 -27.18797705184386 -9.490428758461121",
    );
    // theta 0.1 was the old clamp floor, so it ran unchanged before as well.
    assert_same_as_main(
        DependenceModel::ClaytonCopula { theta: 0.1 },
        "8075.124059968966 -25.19502813880763 23.92326279852489 187.43335701143246 14.077558863097579",
    );
    assert_same_as_main(
        DependenceModel::ClaytonCopula { theta: 2.0 },
        "2439.479539271683 -25.19502813880763 23.10721378806761 63.44497106314043 12.71968399053128",
    );
}

#[test]
fn theta_below_the_old_floor_is_now_used_as_given() {
    // Before, 0 < theta < 0.1 was silently run as 0.1. Now it runs as asked, so its paths differ.
    let small = run(DependenceModel::ClaytonCopula { theta: 0.05 }).unwrap().portfolio_losses;
    let floor = run(DependenceModel::ClaytonCopula { theta: 0.1 }).unwrap().portfolio_losses;
    assert_ne!(small, floor);
}

#[test]
fn enforcement_reports_bad_theta_as_invalid_argument() {
    let mut positions = HashMap::new();
    positions
        .insert("SPY".to_string(), PositionSnapshot { qty: 100.0, price: 500.0, volatility: Some(0.20) });
    let snap = PortfolioSnapshot {
        cash: 50_000.0,
        equity: 100_000.0,
        peak_equity: 100_000.0,
        daily_start_equity: 100_000.0,
        positions,
    };
    for model in [
        DependenceModel::TCopula { degrees_of_freedom: 0 },
        DependenceModel::ClaytonCopula { theta: 0.0 },
        DependenceModel::ClaytonCopula { theta: -2.0 },
    ] {
        let err = snap.var_pct_with_model(model).unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)), "{model:?}: {err:?}");
    }
}
