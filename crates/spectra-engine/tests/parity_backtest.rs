//! C++ <-> Rust parity for the backtester: the same strategies, markets and
//! configurations as `parity/cpp/backtest_golden.cpp`, run with the reference
//! ledger policy, must reproduce every fingerprint, balance and metric of the
//! frozen C++ engine (`parity/golden/backtest.golden`) exactly.

// Bit-exact parity relies on glibc's libm (exp, log, pow): the golden files were produced on
// Linux x86-64 with glibc, and other C libraries may round transcendental functions differently.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::fmt::Write as _;
use std::path::PathBuf;

use spectra_core::cfmt::fmt_g17;
use spectra_core::*;
use spectra_engine::*;

fn random_market(seed: u64, instruments: usize, timestamps: i64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut last: Vec<i64> = (0..instruments).map(|_| rng.range(20 * SCALE, 300 * SCALE)).collect();
    let mut bars = Vec::new();
    for t in 1..=timestamps {
        for (k, prev) in last.iter_mut().enumerate() {
            if rng.range(0, 19) == 0 {
                continue;
            }
            let open = (*prev + rng.range(-SCALE, SCALE)).max(SCALE);
            let close = (open + rng.range(-2 * SCALE, 2 * SCALE)).max(SCALE);
            let high = open.max(close) + rng.range(0, SCALE);
            let low = (open.min(close) - rng.range(0, SCALE)).max(1);
            let volume = if rng.range(0, 9) == 0 { 0 } else { rng.range(1, 1000) * SCALE };
            bars.push(Bar {
                ts_ns: t * 60_000_000_000,
                known_at_ns: t * 60_000_000_000,
                instrument: k as InstrumentId,
                open: Price::from_raw(open),
                high: Price::from_raw(high),
                low: Price::from_raw(low),
                close: Price::from_raw(close),
                volume: Qty::from_raw(volume),
            });
            *prev = close;
        }
    }
    bars
}

struct RandomStrategy {
    rng: Rng,
    last: OrderId,
}

impl Strategy for RandomStrategy {
    fn on_start(&mut self, ctx: &mut Context<'_>) {
        if self.rng.range(0, 1) == 0 {
            let q = Qty::from_raw(self.rng.range(1, 10 * SCALE));
            self.last = ctx.market(0, Side::Buy, q);
        }
    }
    fn on_bar(&mut self, ctx: &mut Context<'_>, group: &[Bar]) {
        for b in group {
            let action = self.rng.range(0, 11);
            let side = if self.rng.range(0, 1) == 0 { Side::Buy } else { Side::Sell };
            let qty = Qty::from_raw(self.rng.range(-SCALE / 10, 50 * SCALE));
            if action < 3 {
                self.last = ctx.market(b.instrument, side, qty);
            } else if action < 7 {
                let d = (b.close.raw() / 30).max(1);
                let px = (b.close.raw() + self.rng.range(-d, d)).max(1);
                self.last = ctx.limit(b.instrument, side, qty, Price::from_raw(px));
            } else if action == 7 && self.last != 0 {
                ctx.cancel(self.last);
            } else if action == 8 {
                let id = self.rng.range(0, 400) as OrderId;
                ctx.cancel(id);
            } else if action == 9 {
                let pos = ctx.position(b.instrument);
                if !pos.is_zero() {
                    let side = if pos.raw() > 0 { Side::Sell } else { Side::Buy };
                    ctx.market(b.instrument, side, pos.checked_abs().unwrap());
                }
            }
        }
    }
    fn on_fill(&mut self, ctx: &mut Context<'_>, f: &Fill) {
        if self.rng.range(0, 9) == 0 {
            let side = if f.side == Side::Buy { Side::Sell } else { Side::Buy };
            self.last = ctx.limit(f.instrument, side, f.qty, Price::from_raw(f.price.raw() + 1));
        }
    }
}

struct SmaCross {
    fast: usize,
    slow: usize,
}

impl Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let id = bars[0].instrument;
        let h = ctx.history(id);
        if h.len() < self.slow {
            return;
        }
        let sma =
            |n: usize| h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128;
        let bullish = sma(self.fast) > sma(self.slow);
        let pos = ctx.position(id);
        if bullish && pos.is_zero() {
            ctx.market(id, Side::Buy, Qty::from_raw(100 * SCALE));
        } else if !bullish && pos.raw() > 0 {
            ctx.market(id, Side::Sell, pos);
        }
    }
}

fn print(out: &mut String, name: &str, r: &BacktestResult) {
    let mut states = [0; 5];
    let mut reasons = [0; 5];
    for o in &r.orders {
        states[match o.state {
            OrderState::Pending => 0,
            OrderState::PartiallyFilled => 1,
            OrderState::Filled => 2,
            OrderState::Cancelled => 3,
            OrderState::Rejected => 4,
            OrderState::Expired => unreachable!("the reference engine has no time in force"),
        }] += 1;
        reasons[match o.reject_reason {
            None => 0,
            Some(Reject::InvalidAmount) => 1,
            Some(Reject::InvalidFill) => 2,
            Some(Reject::InsufficientCash) => 3,
            Some(Reject::ShortNotAllowed) => 4,
            Some(Reject::LeverageExceeded | Reject::RiskLimitExceeded) => {
                panic!("not possible under the reference policy")
            }
        }] += 1;
    }
    writeln!(
        out,
        "{name}\n  dataset {}\n  config  {}\n  result  {}",
        r.run.dataset_sha256, r.run.config_sha256, r.run.result_sha256
    )
    .unwrap();
    writeln!(
        out,
        "  bars {} fills {} orders {} states {} {} {} {} {} reasons {} {} {} {} {}",
        r.run.bar_count,
        r.fill_count(),
        r.orders.len(),
        states[0],
        states[1],
        states[2],
        states[3],
        states[4],
        reasons[0],
        reasons[1],
        reasons[2],
        reasons[3],
        reasons[4]
    )
    .unwrap();
    writeln!(
        out,
        "  equity {} cash {} fees {} realized {}",
        r.ledger.equity().unwrap(),
        r.ledger.cash(),
        r.ledger.total_fees(),
        r.ledger.realized_pnl()
    )
    .unwrap();
    let m = &r.metrics;
    writeln!(
        out,
        "  metrics {} {} {} {} {} {} {}",
        u8::from(m.valid),
        m.periods,
        fmt_g17(m.total_return),
        fmt_g17(m.annualized_return),
        fmt_g17(m.annualized_volatility),
        fmt_g17(m.sharpe),
        fmt_g17(m.max_drawdown)
    )
    .unwrap();
}

#[test]
fn backtester_matches_cpp_reference() {
    let mut out = String::new();

    // 1. The SMA demo exactly as shipped.
    let mut rng = Rng::new(2026);
    let mut bars = Vec::new();
    let mut px = 100 * SCALE;
    for i in 0..1000i64 {
        let open = px;
        let close = (open + rng.range(-SCALE * 3 / 2, SCALE * 3 / 2 + SCALE / 20)).max(SCALE);
        let high = open.max(close) + rng.range(0, SCALE / 2);
        let low = (open.min(close) - rng.range(0, SCALE / 2)).max(1);
        let volume = rng.range(100_000, 1_000_000) * SCALE;
        bars.push(Bar {
            ts_ns: (i + 1) * 86_400_000_000_000,
            known_at_ns: (i + 1) * 86_400_000_000_000,
            instrument: 1,
            open: Price::from_raw(open),
            high: Price::from_raw(high),
            low: Price::from_raw(low),
            close: Price::from_raw(close),
            volume: Qty::from_raw(volume),
        });
        px = close;
    }
    for (fast, slow) in [(10, 30), (3, 15), (20, 80)] {
        let mut cfg = BacktestConfig {
            initial_cash: Money::from_raw(100_000 * SCALE),
            ledger: LedgerConfig::reference(false, false),
            seed: 2026,
            ..BacktestConfig::default()
        };
        cfg.costs.slippage = Bps::from_f64(2.0).unwrap();
        cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0).unwrap();
        let r = run_backtest(&bars, &mut SmaCross { fast, slow }, &cfg).unwrap();
        print(&mut out, &format!("sma {fast}/{slow}"), &r);
    }

    // 2. Random strategies on random markets across 12 configurations.
    for c in 0..12i64 {
        let cfg = BacktestConfig {
            initial_cash: Money::from_raw((50_000 + 25_000 * c) * SCALE),
            ledger: LedgerConfig::reference(c % 2 == 1, c % 4 >= 2),
            costs: CostModel {
                commission: CommissionModel {
                    per_unit: Money::from_raw(if c % 3 == 0 { 1_000_000 } else { 0 }),
                    bps_of_notional: Bps::from_raw(if c % 3 == 1 { 3 * SCALE } else { 0 }),
                    min_fee: Money::from_raw(if c % 5 == 0 { SCALE } else { 0 }),
                },
                slippage: Bps::from_raw(c * 2 * SCALE + 12345),
                ..CostModel::default()
            },
            exec: ExecutionConfig {
                delay_bars: (c % 4) as usize,
                max_participation: Qty::from_raw(if c % 3 == 2 { SCALE / 20 } else { 0 }),
            },
            seed: c as u64 * 977 + 3,
            periods_per_year: if c % 2 == 0 { 252.0 } else { 98_280.0 },
            risk_free_rate: if c % 3 == 0 { 0.0 } else { 0.035 },
            financing: spectra_engine::FinancingConfig::default(), // the reference engine had none
            impact: spectra_engine::ImpactConfig::default(),
            exchange: None,
            risk_limits: None,
            kill_switch: false,
        };
        for seed in 1..=3u64 {
            let bars = random_market(seed * 101 + c as u64, 1 + (c % 4) as usize, 300);
            let mut s = RandomStrategy { rng: Rng::new(seed * 7 + c as u64), last: 0 };
            let r = run_backtest(&bars, &mut s, &cfg).unwrap();
            print(&mut out, &format!("random config {c} seed {seed}"), &r);
        }
    }

    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity/golden/backtest.golden");
    let golden = std::fs::read_to_string(golden_path).unwrap();
    if out != golden {
        let (n, (a, g)) = out
            .lines()
            .zip(golden.lines())
            .enumerate()
            .find(|(_, (a, g))| a != g)
            .unwrap_or((0, ("<length>", "")));
        panic!("backtest parity: first difference at line {}\n  rust: {a}\n  c++ : {g}", n + 1);
    }
}
