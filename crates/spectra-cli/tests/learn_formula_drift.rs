//! Learning Center formula drift guard, engine side (M22 follow-up to task 2.2).
//!
//! Every ⓘ tooltip in the Studio shows a compact formula from the metric registry in
//! `web/frontend/lib/learn/metrics.ts`. `web/frontend/scripts/test-learn.mjs` pins the formulas that
//! the frontend's `calc.ts` reimplements; this test pins the ones only the Rust engine computes.
//!
//! For each pinned metric it checks two things:
//! 1. the tooltip text in the registry is exactly the pinned text, and
//! 2. that formula, evaluated as written (independently of the engine), gives the same number as
//!    the engine function that produces the metric.
//!
//! Editing a tooltip formula, or changing the engine so it no longer matches the tooltip, fails
//! here. A new registry formula fails too, until it is pinned or listed as not yet pinned with a
//! reason.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use spectra_core::{Fill, Ledger, LedgerConfig, Money, Price, Qty, Side};
use spectra_engine::backtest::annualized_turnover;
use spectra_engine::performance::cagr;
use spectra_research::{
    compute_benchmark_metrics, expected_max_sharpe, walk_forward_efficiency, TrialLedger, TrialSummary,
    WalkForwardResult,
};
use spectra_risk::engine::paths_per_second;
use spectra_risk::portfolio::Portfolio;
use spectra_risk::{LimitUtilization, PortfolioSnapshot, PositionSnapshot, RiskLimits};

/// Formulas pinned by `web/frontend/scripts/test-learn.mjs` against `calc.ts`.
const PINNED_IN_FRONTEND: &[&str] = &[
    "total_return",
    "annualized_return",
    "cagr",
    "annualized_volatility",
    "max_drawdown",
    "ulcer_index",
    "sharpe_ratio",
    "sortino_ratio",
    "win_rate",
    "profit_factor",
    "payoff_ratio",
    "expectancy",
    // Computed in the browser (BenchmarkLab.tsx); test-learn.mjs pins the expression.
    "parallel_speedup",
];

/// Registry formulas with no check yet, each with the reason. Empty today: every tooltip formula is
/// checked. Add an entry here (rather than skipping the coverage test) when a new formula can't be
/// pinned straight away.
const NOT_YET_PINNED: &[(&str, &str)] = &[];

/// `metric id -> tooltip formula` for every registry entry that has a formula.
fn registry_formulas() -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/frontend/lib/learn/metrics.ts");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut out = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        // Entries are written as `  metric_id: {` at two spaces of indentation.
        if let Some(id) = line.strip_prefix("  ").and_then(|l| l.strip_suffix(": {")) {
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                current = Some(id.to_string());
                continue;
            }
        }
        if let (Some(id), Some(rest)) = (&current, line.trim().strip_prefix("formula: '")) {
            let formula =
                rest.strip_suffix("',").or_else(|| rest.strip_suffix('\'')).expect("formula on one line");
            out.insert(id.clone(), formula.to_string());
        }
    }
    assert!(out.len() >= 30, "parsed only {} formulas from {}", out.len(), path.display());
    out
}

/// Deterministic pseudo-random returns (no external crates).
fn returns(seed: u64, n: usize, drift: f64, scale: f64) -> Vec<f64> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let u = (x >> 11) as f64 / (1u64 << 53) as f64;
            drift + scale * (u - 0.5)
        })
        .collect()
}

fn mean(x: &[f64]) -> f64 {
    x.iter().sum::<f64>() / x.len() as f64
}

/// Sample variance (n − 1).
fn var(x: &[f64]) -> f64 {
    let m = mean(x);
    x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (x.len() as f64 - 1.0)
}

fn cov(x: &[f64], y: &[f64]) -> f64 {
    let (mx, my) = (mean(x), mean(y));
    x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>() / (x.len() as f64 - 1.0)
}

/// Inverse standard normal CDF (Acklam's rational approximation, relative error < 1.2e-9).
fn inv_norm(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] =
        [7.784_695_709_041_462e-3, 3.224_671_290_700_398e-1, 2.445_134_137_142_996, 3.754_408_661_907_416];
    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    if p < 0.024_25 {
        tail((-2.0 * p.ln()).sqrt())
    } else if p > 1.0 - 0.024_25 {
        -tail((-2.0 * (1.0 - p).ln()).sqrt())
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    }
}

/// `(pinned tooltip text, value from the formula as written, value from the engine)` per metric.
fn pinned() -> BTreeMap<&'static str, (&'static str, f64, f64)> {
    let ppy = 252.0;
    let s = returns(7, 500, 0.0006, 0.03);
    let b = returns(11, 500, 0.0004, 0.025);
    // Make the strategy partly follow the benchmark so beta is not ~0.
    let s: Vec<f64> = s.iter().zip(&b).map(|(x, y)| 0.6 * y + x).collect();
    let bm = compute_benchmark_metrics(&s, &b, ppy).expect("equal lengths");
    assert!(bm.valid);
    let beta = cov(&s, &b) / var(&b);
    let diff: Vec<f64> = s.iter().zip(&b).map(|(x, y)| x - y).collect();

    // Calmar: CAGR over the deepest peak-to-trough fall of the compounded equity.
    let (mut eq, mut peak, mut mdd) = (1.0_f64, 1.0_f64, 0.0_f64);
    for r in &s {
        eq *= 1.0 + r;
        peak = peak.max(eq);
        mdd = mdd.max((peak - eq) / peak);
    }
    let growth: f64 = s.iter().map(|r| 1.0 + r).product();
    let cagr_written = growth.powf(ppy / s.len() as f64) - 1.0;
    assert!((cagr(&s, ppy).unwrap() - cagr_written).abs() < 1e-12);

    // Positions: two longs, one short.
    let pos = [("AAA", 120.0, 50.0), ("BBB", -40.0, 210.0), ("CCC", 15.0, 980.0)];
    let mut portfolio = Portfolio::new();
    let mut positions = HashMap::new();
    for (sym, q, p) in pos {
        portfolio.add_asset(sym, q, p);
        positions.insert(sym.to_string(), PositionSnapshot { qty: q, price: p });
    }
    let equity = 40_000.0;
    let snap = PortfolioSnapshot { equity, positions, ..PortfolioSnapshot::default() };
    let gross: f64 = pos.iter().map(|(_, q, p)| (q * p).abs()).sum();
    let net: f64 = pos.iter().map(|(_, q, p)| q * p).sum();
    let hhi: f64 = pos.iter().map(|(_, q, p)| ((q * p).abs() / gross).powi(2)).sum();

    // Risk Center limit utilization (spectra-api risk summary and alerts use LimitUtilization).
    let (peak_equity, day_start) = (46_000.0, 41_500.0);
    // Net short, so |net exposure| differs from net exposure.
    let risk_pos: [(&str, f64, f64); 2] = [("AAA", 50.0, 100.0), ("BBB", -120.0, 210.0)];
    let risk_gross: f64 = risk_pos.iter().map(|(_, q, p)| (q * p).abs()).sum();
    let risk_net: f64 = risk_pos.iter().map(|(_, q, p)| q * p).sum();
    assert!(risk_net < 0.0);
    let risk = PortfolioSnapshot {
        equity,
        peak_equity,
        daily_start_equity: day_start,
        positions: risk_pos
            .iter()
            .map(|(sym, q, p)| ((*sym).to_string(), PositionSnapshot { qty: *q, price: *p }))
            .collect(),
        ..PortfolioSnapshot::default()
    };
    let limits = RiskLimits {
        max_gross_exposure: 60_000.0,
        max_net_exposure: 20_000.0,
        max_leverage: 2.5,
        max_drawdown_pct: 0.2,
        max_daily_loss: 2_000.0,
        max_var_pct: 0.05,
        ..RiskLimits::default()
    };
    let var_pct = 0.031; // 1-day 99% VaR as a fraction of equity (a simulation output; fixed here)
    let util = LimitUtilization::new(
        risk.gross_exposure(),
        risk.net_exposure(),
        risk.leverage(),
        risk.drawdown_pct(),
        risk.daily_loss(),
        var_pct,
        &limits,
    );

    // Walk-forward efficiency.
    let (mean_is, mean_oos) = (0.84, 0.51);
    let wf = WalkForwardResult {
        mean_in_sample_objective: mean_is,
        mean_out_of_sample_objective: mean_oos,
        ..WalkForwardResult::default()
    };
    assert_eq!(wf.efficiency(), walk_forward_efficiency(mean_is, mean_oos));

    // Trial ledger for the Sharpe variance across trials.
    let srs = [0.031, 0.044, 0.012, 0.058, 0.027, 0.039];
    let mut ledger = TrialLedger::default();
    for (i, sr) in srs.iter().enumerate() {
        ledger.add_summary(TrialSummary {
            id: i as u64 + 1,
            sharpe_per_period: *sr,
            ..TrialSummary::default()
        });
    }
    let v = var(&srs);
    let n = 40.0_f64;
    let gamma = 0.577_215_664_901_532_9;
    let emax_written = v.sqrt()
        * ((1.0 - gamma) * inv_norm(1.0 - 1.0 / n) + gamma * inv_norm(1.0 - 1.0 / (n * std::f64::consts::E)));

    // Ledger books: a long built in two buys and partly sold, a short with a borrow fee, and a
    // dividend on the long. Values are read off the books, not a backtest.
    let money = |v: f64| Money::from_f64(v).unwrap();
    let price = |v: f64| Price::from_f64(v).unwrap();
    let qty = |v: f64| Qty::from_f64(v).unwrap();
    let mut books = Ledger::new(LedgerConfig::reference(true, false));
    books.deposit(money(100_000.0), 1).unwrap();
    let fills = [
        (0, Side::Buy, 100.0, 50.0, 1.0),
        (0, Side::Buy, 30.0, 54.0, 1.0),
        (0, Side::Sell, 40.0, 57.0, 1.0),
        (1, Side::Sell, 20.0, 210.0, 1.5),
    ];
    for (i, (instrument, side, q, p, fee)) in fills.into_iter().enumerate() {
        let f = Fill { instrument, side, qty: qty(q), price: price(p), fee: money(fee), ts_ns: 2 + i as i64 };
        books.apply_fill(&f).unwrap();
    }
    let (last_long, last_short) = (56.5, 200.0);
    books.mark(0, price(last_long)).unwrap();
    books.mark(1, price(last_short)).unwrap();
    let (borrow, dividend) = (3.25, 12.0);
    books.charge_borrow_fee(1, money(borrow), 10).unwrap();
    books.apply_dividend(0, money(dividend), 11).unwrap();

    // Long: 130 bought at an average of (100 × 50 + 30 × 54) / 130, then 40 sold at 57.
    let avg_long = (100.0 * 50.0 + 30.0 * 54.0) / 130.0;
    let (q_long, q_short) = (90.0, -20.0);
    let cash = 100_000.0 - 100.0 * 50.0 - 30.0 * 54.0 + 40.0 * 57.0 + 20.0 * 210.0 - 4.5 - borrow + dividend;
    let unrealized_long = q_long * (last_long - avg_long);
    let unrealized_short = q_short * (last_short - 210.0);
    let realized_long = 40.0 * (57.0 - avg_long);
    let long = books.position(0);
    let engine_net_long = spectra_backtest::output::net_pnl(
        long.realized,
        books.position_value(0).unwrap().checked_sub(long.cost).unwrap(),
        long.fees,
        Money::ZERO,
        money(dividend),
    )
    .unwrap()
    .to_f64();

    // Turnover on a short equity series.
    let traded = 100.0 * 50.0 + 30.0 * 54.0 + 40.0 * 57.0 + 20.0 * 210.0;
    let equity_series = [100_000.0, 100_400.0, 99_800.0, 100_900.0, 101_300.0];
    let periods = (equity_series.len() - 1) as f64;
    let mean_equity = equity_series.iter().sum::<f64>() / equity_series.len() as f64;

    BTreeMap::from([
        (
            "final_equity",
            (
                "cash + Σ qty × last close",
                cash + q_long * last_long + q_short * last_short,
                books.equity().unwrap().to_f64(),
            ),
        ),
        (
            "unrealized_pnl",
            (
                "qty × (price − average cost)",
                unrealized_long + unrealized_short,
                books.unrealized_pnl().unwrap().to_f64(),
            ),
        ),
        (
            "net_pnl",
            (
                "realized + unrealized − fees − borrow + dividends",
                realized_long + unrealized_long - 3.0 - 0.0 + dividend,
                engine_net_long,
            ),
        ),
        (
            "turnover",
            (
                "Σ |notional| / mean equity × ppy / n",
                traded / mean_equity * 252.0 / periods,
                annualized_turnover(traded, &equity_series, 252.0),
            ),
        ),
        (
            "paths_per_second",
            (
                "paths × 1000 / median latency (ms)",
                250_000.0 * 1000.0 / 412.5,
                paths_per_second(250_000, 412.5),
            ),
        ),
        ("beta", ("cov(r_s, r_b) / var(r_b)", beta, bm.beta)),
        (
            "alpha",
            ("(mean r_s − β · mean r_b) × ppy", (mean(&s) - beta * mean(&b)) * ppy, bm.alpha_annualized),
        ),
        ("tracking_error", ("σ(rₛ − r_b) · √ppy", var(&diff).sqrt() * ppy.sqrt(), bm.tracking_error)),
        (
            "calmar_ratio",
            (
                "CAGR / max drawdown",
                cagr_written / mdd,
                spectra_engine::performance::calmar(&s, ppy).unwrap(),
            ),
        ),
        ("hhi", ("Σ wᵢ², where wᵢ = |vᵢ| / Σ|vⱼ|", hhi, portfolio.hhi().unwrap())),
        ("gross_exposure", ("Σ |quantity × price|", gross, snap.gross_exposure())),
        ("net_exposure", ("Σ qᵢ pᵢ (longs positive, shorts negative)", net, snap.net_exposure())),
        ("gross_leverage", ("Σ |position value| / equity", gross / equity, snap.leverage())),
        (
            "gross_exposure_utilization",
            ("gross exposure ÷ max gross exposure", risk_gross / 60_000.0, util.gross_exposure),
        ),
        (
            "net_exposure_utilization",
            ("|net exposure| ÷ max net exposure", risk_net.abs() / 20_000.0, util.net_exposure),
        ),
        (
            "leverage_utilization",
            ("(gross exposure ÷ equity) ÷ max leverage", (risk_gross / equity) / 2.5, util.leverage),
        ),
        (
            "drawdown_utilization",
            (
                "((peak equity − equity) ÷ peak equity) ÷ max drawdown",
                ((peak_equity - equity) / peak_equity) / 0.2,
                util.drawdown,
            ),
        ),
        (
            "daily_loss_utilization",
            (
                "(start-of-day equity − equity) ÷ max daily loss",
                (day_start - equity) / 2_000.0,
                util.daily_loss,
            ),
        ),
        ("var_utilization", ("1-day 99% VaR (% of equity) ÷ max VaR", var_pct / 0.05, util.var)),
        (
            "walk_forward_efficiency",
            ("mean OOS objective / mean IS objective", mean_oos / mean_is, wf.efficiency()),
        ),
        (
            "market_value",
            ("quantity × price", -40.0 * 210.0, PositionSnapshot { qty: -40.0, price: 210.0 }.value()),
        ),
        ("sharpe_variance", ("V = Σ (SRᵢ − mean SR)² / (N − 1)", v, ledger.sharpe_variance())),
        (
            "expected_max_sharpe",
            (
                "√V × ((1−γ)·Φ⁻¹(1−1/N) + γ·Φ⁻¹(1−1/(N·e)))",
                emax_written,
                expected_max_sharpe(ledger.sharpe_variance(), n as usize),
            ),
        ),
    ])
}

#[test]
fn engine_tooltip_formulas_match_the_engine() {
    let registry = registry_formulas();
    for (id, (formula, written, engine)) in pinned() {
        let shown = registry.get(id).unwrap_or_else(|| panic!("{id}: no formula in the metric registry"));
        assert_eq!(
            shown, formula,
            "{id}: tooltip formula changed; update the engine and this check together"
        );
        assert!(engine.is_finite() && written.is_finite(), "{id}: non-finite value");
        let tol = 1e-7 * engine.abs().max(1.0);
        assert!(
            (written - engine).abs() <= tol,
            "{id}: tooltip formula gives {written}, the engine gives {engine}"
        );
    }
}

#[test]
fn every_registry_formula_is_pinned_somewhere_or_listed() {
    let registry: BTreeSet<String> = registry_formulas().into_keys().collect();
    let rust: BTreeSet<String> = pinned().into_keys().map(String::from).collect();
    let frontend: BTreeSet<String> = PINNED_IN_FRONTEND.iter().map(|s| (*s).to_string()).collect();
    let listed: BTreeSet<String> = NOT_YET_PINNED.iter().map(|(s, _)| (*s).to_string()).collect();

    for id in &registry {
        assert!(
            rust.contains(id) || frontend.contains(id) || listed.contains(id),
            "{id} has a tooltip formula but no drift check: pin it here or in test-learn.mjs, \
             or add it to NOT_YET_PINNED with a reason"
        );
    }
    for id in rust.iter().chain(&frontend).chain(&listed) {
        assert!(registry.contains(id), "{id} is listed here but has no formula in the registry");
    }
    assert!(rust.is_disjoint(&frontend) && rust.is_disjoint(&listed) && frontend.is_disjoint(&listed));
}
